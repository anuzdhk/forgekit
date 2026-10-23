//! `forgekitd` — the per-user daemon. Owns long-lived state (Nightowl's
//! inhibitor, Clipstack's clipboard watcher, Dusk's schedule) and serves it on
//! the session bus as `org.forgekit.Daemon1` and `org.forgekit.Clipstack1`.

mod clipstack;
mod dusk;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use forgekit_core::settings::General;
use forgekit_core::tools::nightowl::Inhibitor;
use forgekit_core::{CATALOG, Capabilities};
use zbus::{fdo, interface, object_server::SignalEmitter};

const NAME: &str = "org.forgekit.Daemon";
const PATH: &str = "/org/forgekit/Daemon";

#[derive(Default)]
struct Nightowl {
    inhibitor: Option<Inhibitor>,
    reason: String,
    /// Bumped on every start/stop so a stale timer can't end a newer session.
    generation: u64,
}

#[derive(Clone, Default)]
struct Daemon {
    nightowl: Arc<Mutex<Nightowl>>,
}

#[interface(name = "org.forgekit.Daemon1")]
impl Daemon {
    #[zbus(property)]
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    /// The detected system, as JSON.
    async fn capabilities(&self) -> fdo::Result<String> {
        let caps = detect().await?;
        serde_json::to_string(&caps).map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    /// Every tool with its enabled flag and status, as JSON.
    async fn tools(&self) -> fdo::Result<String> {
        let caps = detect().await?;
        let general = General::load().map_err(|e| fdo::Error::Failed(e.to_string()))?;
        let v: Vec<_> = CATALOG
            .iter()
            .map(|t| serde_json::json!({ "id": t.id, "name": t.name, "enabled": general.is_enabled(t.id), "status": t.status(&caps) }))
            .collect();
        serde_json::to_string(&v).map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    /// Keep the PC awake. `seconds` = 0 means until `NightowlStop`.
    async fn nightowl_start(
        &self,
        reason: String,
        seconds: u32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let reason = if reason.is_empty() { "Keeping awake".to_string() } else { reason };
        let r = reason.clone();
        let inhibitor = tokio::task::spawn_blocking(move || Inhibitor::acquire(&r))
            .await
            .map_err(|e| fdo::Error::Failed(e.to_string()))?
            .map_err(|e| fdo::Error::Failed(format!("{e:#}")))?;
        let generation = {
            let mut n = self.nightowl.lock().unwrap();
            n.generation += 1;
            n.inhibitor = Some(inhibitor);
            n.reason = reason;
            n.generation
        };
        if seconds > 0 {
            let state = self.nightowl.clone();
            let emitter = emitter.to_owned();
            let this = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(seconds.into())).await;
                let stopped = {
                    let mut n = state.lock().unwrap();
                    n.generation == generation && n.inhibitor.take().is_some()
                };
                if stopped {
                    let _ = this.nightowl_active_changed(&emitter).await;
                }
            });
        }
        self.nightowl_active_changed(&emitter).await?;
        Ok(())
    }

    async fn nightowl_stop(&self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        {
            let mut n = self.nightowl.lock().unwrap();
            n.generation += 1;
            n.inhibitor = None;
        }
        self.nightowl_active_changed(&emitter).await?;
        Ok(())
    }

    #[zbus(property)]
    fn nightowl_active(&self) -> bool {
        self.nightowl.lock().unwrap().inhibitor.is_some()
    }

    #[zbus(property)]
    fn nightowl_reason(&self) -> String {
        let n = self.nightowl.lock().unwrap();
        if n.inhibitor.is_some() { n.reason.clone() } else { String::new() }
    }
}

/// Detection makes blocking D-Bus and Wayland calls; keep them off the async workers.
async fn detect() -> fdo::Result<Capabilities> {
    tokio::task::spawn_blocking(Capabilities::detect).await.map_err(|e| fdo::Error::Failed(e.to_string()))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let conn = zbus::connection::Builder::session()?.serve_at(PATH, Daemon::default())?.build().await?;
    let clip = clipstack::Clipstack::new(conn.clone());
    conn.object_server().at(clipstack::PATH, clip.clone()).await?;
    // Claim the name last so callers never see a half-registered daemon.
    conn.request_name(NAME).await?;
    eprintln!("forgekitd {} on {NAME}", env!("CARGO_PKG_VERSION"));

    clip.sync().await;
    tokio::spawn(dusk::run());

    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
    Ok(())
}
