//! `forgekit-helper` — the one part of Forgekit that runs as root.
//!
//! It exists because only root can grab keyboards (evdev) and create a
//! virtual keyboard (uinput), which Keysmith and Umlaut need on Wayland.
//! Rules it follows:
//! - It never logs, stores or sends key events; nothing about keystrokes
//!   crosses D-Bus.
//! - It grabs keyboards only while a remap or Umlaut is switched on.
//! - Changing its config is checked with polkit (`org.forgekit.helper.configure`).
//! - Ctrl+Alt+Shift+Esc always hands the keyboard back.
//! - systemd sandboxes it: no network, no capabilities, read-only system.

mod devices;
mod install;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use forgekit_keys::HelperConfig;
use zbus::zvariant::Value;
use zbus::{fdo, interface, message::Header};

use devices::{Msg, Status};

const NAME: &str = "org.forgekit.Helper";
const PATH: &str = "/org/forgekit/Helper";
const ACTION: &str = "org.forgekit.helper.configure";

fn state_dir() -> PathBuf {
    std::env::var_os("STATE_DIRECTORY").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/var/lib/forgekit-helper"))
}

fn config_path() -> PathBuf {
    state_dir().join("config.json")
}

fn load_config() -> HelperConfig {
    std::fs::read_to_string(config_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_config(cfg: &HelperConfig) -> anyhow::Result<()> {
    let p = config_path();
    std::fs::create_dir_all(p.parent().unwrap())?;
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(cfg)?)?;
    std::fs::rename(tmp, p)?;
    Ok(())
}

struct Helper {
    tx: Mutex<Sender<Msg>>,
    status: Arc<Mutex<Status>>,
    config: Mutex<HelperConfig>,
}

/// Ask polkit whether the caller may change the config (it prompts if needed).
async fn authorized(conn: &zbus::Connection, header: &Header<'_>) -> fdo::Result<()> {
    let sender = header.sender().ok_or_else(|| fdo::Error::AccessDenied("no sender".into()))?.to_string();
    let subject: (&str, HashMap<&str, Value>) =
        ("system-bus-name", HashMap::from([("name", Value::from(sender.as_str()))]));
    const ALLOW_USER_INTERACTION: u32 = 1;
    let reply = conn
        .call_method(
            Some("org.freedesktop.PolicyKit1"),
            "/org/freedesktop/PolicyKit1/Authority",
            Some("org.freedesktop.PolicyKit1.Authority"),
            "CheckAuthorization",
            &(subject, ACTION, HashMap::<&str, &str>::new(), ALLOW_USER_INTERACTION, ""),
        )
        .await
        .map_err(|e| fdo::Error::AccessDenied(format!("polkit: {e}")))?;
    let (ok, _challenge, _details): (bool, bool, HashMap<String, String>) =
        reply.body().deserialize().map_err(|e| fdo::Error::Failed(e.to_string()))?;
    if ok { Ok(()) } else { Err(fdo::Error::AccessDenied("not authorized".into())) }
}

#[interface(name = "org.forgekit.Helper1")]
impl Helper {
    /// Version, whether keyboards are grabbed, their names, and pause state (JSON).
    fn status(&self) -> String {
        let mut s = self.status.lock().unwrap().clone();
        s.version = env!("CARGO_PKG_VERSION");
        serde_json::to_string(&s).unwrap_or_default()
    }

    /// The current rules and Umlaut settings (JSON).
    fn config(&self) -> String {
        serde_json::to_string(&*self.config.lock().unwrap()).unwrap_or_default()
    }

    /// Replace the config. Validated, saved, applied immediately.
    async fn set_config(
        &self,
        json: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> fdo::Result<()> {
        authorized(conn, &header).await?;
        let cfg: HelperConfig = serde_json::from_str(&json).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        cfg.validate().map_err(fdo::Error::InvalidArgs)?;
        save_config(&cfg).map_err(|e| fdo::Error::Failed(format!("{e:#}")))?;
        *self.config.lock().unwrap() = cfg.clone();
        self.tx.lock().unwrap().send(Msg::SetConfig(cfg)).map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    /// Start remapping again after the emergency chord.
    async fn resume(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> fdo::Result<()> {
        authorized(conn, &header).await?;
        self.tx.lock().unwrap().send(Msg::Resume).map_err(|e| fdo::Error::Failed(e.to_string()))
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = args
        .iter()
        .position(|a| a == "--root")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| "/".into());
    match args.first().map(String::as_str) {
        Some("--install") => {
            for p in install::install(&root)? {
                println!("installed {}", p.display());
            }
            return Ok(());
        }
        Some("--uninstall") => {
            install::uninstall(&root)?;
            println!("uninstalled");
            return Ok(());
        }
        Some("--version") => {
            println!("forgekit-helper {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some(other) => anyhow::bail!("unknown argument {other} (try --install, --uninstall, --version)"),
        None => {}
    }
    serve()
}

#[tokio::main(flavor = "current_thread")]
async fn serve() -> anyhow::Result<()> {
    let cfg = load_config();
    let status = Arc::new(Mutex::new(Status::default()));
    let (tx, rx) = std::sync::mpsc::channel();
    {
        let (cfg, tx, status) = (cfg.clone(), tx.clone(), status.clone());
        std::thread::spawn(move || devices::run(cfg, rx, tx, status));
    }
    let helper = Helper { tx: Mutex::new(tx), status, config: Mutex::new(cfg) };
    let _conn = zbus::connection::Builder::system()?
        .name(NAME)?
        .serve_at(PATH, helper)?
        .build()
        .await
        .context("claiming org.forgekit.Helper on the system bus")?;
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
    Ok(())
}
