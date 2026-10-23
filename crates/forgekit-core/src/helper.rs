//! Talking to `forgekit-helper` (the privileged input helper) from the user's side.
//!
//! The user's desired Keysmith/Umlaut settings live in
//! `~/.config/forgekit/input.json`; [`push`] sends them to the helper, which
//! asks polkit (a password prompt, cached for a few minutes) before applying.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, bail};
pub use forgekit_keys::HelperConfig;
use serde::Deserialize;

const BUS_NAME: &str = "org.forgekit.Helper";
const PATH: &str = "/org/forgekit/Helper";
const IFACE: &str = "org.forgekit.Helper1";

fn user_file() -> PathBuf {
    crate::paths::config_dir().join("input.json")
}

/// The user's settings (what they asked for), whether or not the helper runs.
pub fn load() -> HelperConfig {
    std::fs::read_to_string(user_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(cfg: &HelperConfig) -> anyhow::Result<()> {
    cfg.validate().map_err(anyhow::Error::msg)?;
    std::fs::create_dir_all(crate::paths::config_dir())?;
    std::fs::write(user_file(), serde_json::to_vec_pretty(cfg)?)?;
    Ok(())
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Status {
    pub version: String,
    pub active: bool,
    pub keyboards: Vec<String>,
    pub paused: bool,
    pub error: Option<String>,
}

fn call<B: serde::Serialize + zbus::zvariant::DynamicType>(
    method: &str,
    body: &B,
) -> anyhow::Result<zbus::message::Message> {
    let conn = zbus::blocking::Connection::system()?;
    conn.call_method(Some(BUS_NAME), PATH, Some(IFACE), method, body).map_err(|e| match e {
        zbus::Error::MethodError(name, msg, _) if name.as_str().ends_with("AccessDenied") => {
            anyhow::anyhow!("not authorized{}", msg.map(|m| format!(" ({m})")).unwrap_or_default())
        }
        e => e.into(),
    })
}

/// `None` when the helper isn't installed or isn't running.
pub fn status() -> Option<Status> {
    let m = call("Status", &()).ok()?;
    let json: String = m.body().deserialize().ok()?;
    serde_json::from_str(&json).ok()
}

/// Save the settings and apply them (the helper may ask for a password).
pub fn push(cfg: &HelperConfig) -> anyhow::Result<()> {
    save(cfg)?;
    call("SetConfig", &serde_json::to_string(cfg)?).context("applying the settings")?;
    Ok(())
}

pub fn resume() -> anyhow::Result<()> {
    call("Resume", &())?;
    Ok(())
}

/// The helper binary to install from: next to this program, on PATH, or already installed.
pub fn binary() -> Option<PathBuf> {
    let sibling = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("forgekit-helper")));
    sibling
        .filter(|p| p.exists())
        .or_else(|| crate::paths::which("forgekit-helper"))
        .or_else(|| Some(PathBuf::from("/usr/libexec/forgekit-helper")).filter(|p| p.exists()))
}

/// Install (or remove) the helper system-wide via `pkexec` (one password prompt).
pub fn install(remove: bool) -> anyhow::Result<()> {
    let bin = binary().context("forgekit-helper isn't available to install")?;
    crate::privileged::check_can_elevate()?;
    let st = Command::new("pkexec")
        .arg(&bin)
        .arg(if remove { "--uninstall" } else { "--install" })
        .status()
        .context("running pkexec")?;
    if !st.success() {
        bail!("cancelled or not permitted");
    }
    if !remove {
        // Give it a moment to claim its bus name, then send the saved settings.
        for _ in 0..20 {
            if status().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    }
    Ok(())
}
