//! Roam: one mouse and keyboard across several PCs.
//!
//! Sending input over the network is security-critical and needs a mature
//! protocol, so Roam sets up and launches Deskflow (the open-source project
//! behind Synergy, with Wayland support through the desktop portals) or
//! Input Leap, instead of shipping its own.

use std::process::Command;

use anyhow::{Context, bail};

pub const DESKFLOW_FLATPAK: &str = "org.deskflow.deskflow";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub name: &'static str,
    /// argv to start it.
    pub launch: Vec<String>,
}

fn flatpak_has(app: &str) -> bool {
    Command::new("flatpak").args(["info", app]).output().is_ok_and(|o| o.status.success())
}

/// Deskflow or Input Leap, if installed natively or as a Flatpak.
pub fn find() -> Option<Found> {
    if crate::paths::which("deskflow").is_some() {
        return Some(Found { name: "Deskflow", launch: vec!["deskflow".into()] });
    }
    if crate::paths::which("flatpak").is_some() && flatpak_has(DESKFLOW_FLATPAK) {
        return Some(Found { name: "Deskflow", launch: vec!["flatpak".into(), "run".into(), DESKFLOW_FLATPAK.into()] });
    }
    if crate::paths::which("input-leap").is_some() {
        return Some(Found { name: "Input Leap", launch: vec!["input-leap".into()] });
    }
    None
}

pub fn launch(f: &Found) -> anyhow::Result<()> {
    Command::new(&f.launch[0]).args(&f.launch[1..]).spawn().with_context(|| format!("starting {}", f.name))?;
    Ok(())
}

/// The command that installs Deskflow for this user from Flathub.
pub fn install_command() -> anyhow::Result<Vec<String>> {
    if crate::paths::which("flatpak").is_none() {
        bail!("install Flatpak first, or Deskflow from your distribution's packages");
    }
    Ok(["flatpak", "install", "--user", "-y", "--noninteractive", "flathub", DESKFLOW_FLATPAK]
        .map(String::from)
        .to_vec())
}

/// Make sure the user has the Flathub remote (needed by `install_command`).
pub fn ensure_flathub() -> anyhow::Result<()> {
    let st = Command::new("flatpak")
        .args(["remote-add", "--user", "--if-not-exists", "flathub", "https://dl.flathub.org/repo/flathub.flatpakrepo"])
        .status()?;
    if !st.success() {
        bail!("couldn't add the Flathub remote");
    }
    Ok(())
}
