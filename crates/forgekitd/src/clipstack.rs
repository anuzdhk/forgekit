//! Clipstack in the daemon: runs the clipboard backend on its own thread,
//! records clips, and serves `org.forgekit.Clipstack1`.

use std::os::fd::OwnedFd;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use forgekit_core::Capabilities;
use forgekit_core::settings::General;
use forgekit_core::tools::clipstack::{self, Command, Settings, store::Store};
use zbus::{fdo, interface, object_server::SignalEmitter};

pub const PATH: &str = "/org/forgekit/Clipstack";

struct Running {
    backend: &'static str,
    commands: Sender<Command>,
    wake: OwnedFd,
}

#[derive(Clone)]
pub struct Clipstack {
    running: Arc<Mutex<Option<Running>>>,
    conn: zbus::Connection,
}

fn failed(e: impl std::fmt::Display) -> fdo::Error {
    fdo::Error::Failed(e.to_string())
}

impl Clipstack {
    pub fn new(conn: zbus::Connection) -> Self {
        Clipstack { running: Arc::default(), conn }
    }

    /// Start or stop the backend to match the user's on/off switch.
    pub async fn sync(&self) {
        let enabled = General::load().unwrap_or_default().is_enabled("clipstack");
        let is_running = self.running.lock().unwrap().is_some();
        if enabled && !is_running {
            if let Err(e) = self.start().await {
                eprintln!("clipstack: {e:#}");
            }
        } else if !enabled && is_running {
            self.stop();
        }
    }

    async fn start(&self) -> anyhow::Result<()> {
        let caps = tokio::task::spawn_blocking(Capabilities::detect).await?;
        let Some(backend) = clipstack::backend_for(&caps) else {
            anyhow::bail!("no clipboard backend for this session");
        };
        let name = backend.name();
        let store = Store::open_default()?;
        let (tx, rx) = std::sync::mpsc::channel();
        let (wake_read, wake_write) = clipstack::wake_pipe()?;
        let (changed_tx, mut changed_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        let on_clip = Box::new(move |clip: clipstack::Clip| {
            let settings = Settings::load().unwrap_or_default();
            match store.add(&clip, &settings) {
                Ok(Some(_)) => {
                    let _ = changed_tx.send(());
                }
                Ok(None) => {}
                Err(e) => eprintln!("clipstack: saving clip: {e:#}"),
            }
        });
        let running = self.running.clone();
        std::thread::spawn(move || {
            if let Err(e) = backend.run(on_clip, rx, wake_read) {
                eprintln!("clipstack: {name} backend stopped: {e:#}");
            }
            running.lock().unwrap().take();
        });

        let conn = self.conn.clone();
        tokio::spawn(async move {
            while changed_rx.recv().await.is_some() {
                if let Ok(emitter) = SignalEmitter::new(&conn, PATH) {
                    let _ = Clipstack::changed(&emitter).await;
                }
            }
        });

        *self.running.lock().unwrap() = Some(Running { backend: name, commands: tx, wake: wake_write });
        eprintln!("clipstack: watching the clipboard ({name})");
        Ok(())
    }

    fn stop(&self) {
        if let Some(r) = self.running.lock().unwrap().take() {
            let _ = r.commands.send(Command::Stop);
            clipstack::wake(&r.wake);
        }
    }
}

#[interface(name = "org.forgekit.Clipstack1")]
impl Clipstack {
    /// Put a history item back on the clipboard.
    async fn copy(&self, id: i64, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        let store = Store::open_default().map_err(failed)?;
        let clip = store.get(id).map_err(failed)?.ok_or_else(|| fdo::Error::InvalidArgs(format!("no item {id}")))?;
        {
            let guard = self.running.lock().unwrap();
            let r = guard.as_ref().ok_or_else(|| failed("Clipstack isn't running"))?;
            r.commands.send(Command::Set(clip)).map_err(failed)?;
            clipstack::wake(&r.wake);
        }
        store.touch(id).map_err(failed)?;
        Clipstack::changed(&emitter).await?;
        Ok(())
    }

    /// Recent items as JSON (pinned first), for the top-bar menu. Images come
    /// with `thumb`, a cached copy of the picture on disk.
    async fn history(&self, limit: u32) -> fdo::Result<String> {
        let store = Store::open_default().map_err(failed)?;
        let items = store.list("", limit.clamp(1, 100) as usize).map_err(failed)?;
        let dir = forgekit_core::paths::cache_dir().join("clip-thumbs");
        let out: Vec<serde_json::Value> = items
            .iter()
            .map(|it| {
                let mut v = serde_json::to_value(it).unwrap_or_default();
                if it.is_image() {
                    let path = dir.join(format!("{}.{}", it.id, it.mime.trim_start_matches("image/")));
                    if !path.exists()
                        && let Ok(Some(clip)) = store.get(it.id)
                    {
                        let _ = std::fs::create_dir_all(&dir);
                        let _ = std::fs::write(&path, &clip.data);
                    }
                    v["thumb"] = path.display().to_string().into();
                }
                v
            })
            .collect();
        Ok(serde_json::Value::from(out).to_string())
    }

    async fn set_pinned(
        &self,
        id: i64,
        pinned: bool,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        Store::open_default().and_then(|s| s.set_pinned(id, pinned)).map_err(failed)?;
        Clipstack::changed(&emitter).await?;
        Ok(())
    }

    async fn delete(&self, id: i64, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        Store::open_default().and_then(|s| s.delete(id)).map_err(failed)?;
        Clipstack::changed(&emitter).await?;
        Ok(())
    }

    /// Remove everything but pinned items. Returns how many went.
    async fn clear(&self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<u32> {
        let n = Store::open_default().and_then(|s| s.clear()).map_err(failed)?;
        let _ = std::fs::remove_dir_all(forgekit_core::paths::cache_dir().join("clip-thumbs"));
        Clipstack::changed(&emitter).await?;
        Ok(n as u32)
    }

    /// Put text on the clipboard (used by the CLI when no clipboard tool is installed).
    async fn set_text(&self, text: String) -> fdo::Result<()> {
        let guard = self.running.lock().unwrap();
        let r = guard.as_ref().ok_or_else(|| failed("Clipstack isn't running"))?;
        r.commands.send(Command::Set(clipstack::Clip::text(&text))).map_err(failed)?;
        clipstack::wake(&r.wake);
        Ok(())
    }

    /// Put any clip (e.g. a PNG) on the clipboard; it stays after the caller exits.
    async fn set_clip(&self, mime: String, data: Vec<u8>) -> fdo::Result<()> {
        let guard = self.running.lock().unwrap();
        let r = guard.as_ref().ok_or_else(|| failed("Clipstack isn't running"))?;
        r.commands.send(Command::Set(clipstack::Clip { mime, data })).map_err(failed)?;
        clipstack::wake(&r.wake);
        Ok(())
    }

    /// Record a clip from an outside source (e.g. a desktop extension).
    async fn add(
        &self,
        mime: String,
        data: Vec<u8>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let store = Store::open_default().map_err(failed)?;
        let clip = clipstack::Clip { mime: clipstack::storage_mime(&mime), data };
        if store.add(&clip, &Settings::load().unwrap_or_default()).map_err(failed)?.is_some() {
            Clipstack::changed(&emitter).await?;
        }
        Ok(())
    }

    /// Start or stop after the on/off switch changed.
    async fn reload(&self) {
        self.sync().await;
    }

    #[zbus(property)]
    fn backend(&self) -> String {
        self.running.lock().unwrap().as_ref().map(|r| r.backend.to_string()).unwrap_or_default()
    }

    #[zbus(property)]
    fn private(&self) -> bool {
        Settings::load().unwrap_or_default().private
    }

    #[zbus(property)]
    fn set_private(&mut self, on: bool) -> fdo::Result<()> {
        let mut s = Settings::load().map_err(failed)?;
        s.private = on;
        s.save().map_err(failed)
    }

    /// History changed (new item, restored item).
    #[zbus(signal)]
    async fn changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}
