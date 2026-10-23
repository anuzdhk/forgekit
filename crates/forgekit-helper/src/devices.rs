//! Keyboards in, virtual keyboard out.
//!
//! The controller thread owns the engine and the uinput device. While the
//! config is active it grabs every physical keyboard (one reader thread each,
//! re-scanning for hot-plugged ones); while inactive it grabs nothing.
//! Key events never leave this process.

use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, Device, EventType, InputEvent, KeyCode};
use forgekit_keys::HelperConfig;
use forgekit_keys::engine::{Engine, Out};
use serde::Serialize;

pub const VIRTUAL_NAME: &str = "Forgekit virtual keyboard";

pub enum Msg {
    Key { code: u16, value: i32 },
    Gone(PathBuf),
    SetConfig(HelperConfig),
    Resume,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    pub version: &'static str,
    /// Keyboards are grabbed and remapped right now.
    pub active: bool,
    pub keyboards: Vec<String>,
    /// The emergency chord was pressed; remapping is paused until resumed.
    pub paused: bool,
    pub error: Option<String>,
}

struct Reader {
    stop: Arc<AtomicBool>,
}

pub fn run(cfg: HelperConfig, rx: Receiver<Msg>, tx: Sender<Msg>, status: Arc<Mutex<Status>>) {
    let start = Instant::now();
    let mut cfg = cfg;
    let mut engine = Engine::new(&cfg);
    let mut paused = false;
    let mut virt: Option<VirtualDevice> = None;
    let mut readers: HashMap<PathBuf, Reader> = HashMap::new();
    let mut next_scan = Instant::now();

    loop {
        let want = Engine::is_active(&cfg) && !paused;
        if want && virt.is_none() {
            match virtual_keyboard() {
                Ok(v) => virt = Some(v),
                Err(e) => set_error(&status, format!("can't create the virtual keyboard: {e}")),
            }
        }
        if !want {
            stop_all(&mut readers);
            virt = None;
        }
        if want && virt.is_some() && Instant::now() >= next_scan {
            scan(&mut readers, &tx);
            next_scan = Instant::now() + Duration::from_secs(2);
        }
        {
            let mut s = status.lock().unwrap();
            s.active = want && virt.is_some() && !readers.is_empty();
            s.paused = paused;
            s.keyboards = readers.keys().map(device_name).collect();
            s.keyboards.sort();
        }

        let msg = match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(m) => m,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match msg {
            Msg::Key { code, value } => {
                let Some(v) = virt.as_mut() else { continue };
                let now = start.elapsed().as_millis() as u64;
                let mut batch = Vec::new();
                for out in engine.handle(code, value, now) {
                    match out {
                        Out::Key(k, val) => batch.push(InputEvent::new(EventType::KEY.0, k, val)),
                        Out::Panic => paused = true,
                    }
                }
                // One SYN per output event keeps chords ordered for the compositor.
                for ev in batch {
                    let _ = v.emit(&[ev]);
                }
            }
            Msg::Gone(p) => {
                readers.remove(&p);
            }
            Msg::SetConfig(c) => {
                release(&mut engine, virt.as_mut());
                cfg = c;
                engine = Engine::new(&cfg);
                paused = false;
                set_error(&status, None::<String>);
            }
            Msg::Resume => {
                release(&mut engine, virt.as_mut());
                engine = Engine::new(&cfg);
                paused = false;
            }
        }
    }
    stop_all(&mut readers);
}

fn release(engine: &mut Engine, virt: Option<&mut VirtualDevice>) {
    let outs = engine.release_all();
    if let Some(v) = virt {
        for o in outs {
            if let Out::Key(k, val) = o {
                let _ = v.emit(&[InputEvent::new(EventType::KEY.0, k, val)]);
            }
        }
    }
}

fn set_error(status: &Mutex<Status>, e: impl Into<Option<String>>) {
    status.lock().unwrap().error = e.into();
}

fn virtual_keyboard() -> std::io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<KeyCode>::new();
    for code in 1..=248u16 {
        keys.insert(KeyCode::new(code));
    }
    VirtualDevice::builder()?.name(VIRTUAL_NAME).with_keys(&keys)?.build()
}

fn is_keyboard(d: &Device) -> bool {
    if d.name().is_some_and(|n| n.contains("Forgekit")) {
        return false;
    }
    d.supported_keys().is_some_and(|k| {
        [KeyCode::KEY_A, KeyCode::KEY_Z, KeyCode::KEY_SPACE, KeyCode::KEY_ENTER].iter().all(|c| k.contains(*c))
    })
}

fn device_name(p: &PathBuf) -> String {
    Device::open(p).ok().and_then(|d| d.name().map(str::to_string)).unwrap_or_else(|| p.display().to_string())
}

fn scan(readers: &mut HashMap<PathBuf, Reader>, tx: &Sender<Msg>) {
    for (path, mut dev) in evdev::enumerate() {
        if readers.contains_key(&path) || !is_keyboard(&dev) {
            continue;
        }
        if dev.grab().is_err() {
            continue;
        }
        let stop = Arc::new(AtomicBool::new(false));
        readers.insert(path.clone(), Reader { stop: stop.clone() });
        let tx = tx.clone();
        std::thread::spawn(move || read_keyboard(path, dev, stop, tx));
    }
}

fn read_keyboard(path: PathBuf, mut dev: Device, stop: Arc<AtomicBool>, tx: Sender<Msg>) {
    let _ = dev.set_nonblocking(true);
    let fd = dev.as_raw_fd();
    while !stop.load(Ordering::Relaxed) {
        let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd.
        let r = unsafe { libc::poll(&mut p, 1, 250) };
        if r < 0 || p.revents & (libc::POLLERR | libc::POLLHUP) != 0 {
            break;
        }
        if r == 0 {
            continue;
        }
        match dev.fetch_events() {
            Ok(events) => {
                for ev in events {
                    if ev.event_type() == EventType::KEY {
                        let _ = tx.send(Msg::Key { code: ev.code(), value: ev.value() });
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
    }
    let _ = dev.ungrab();
    if !stop.load(Ordering::Relaxed) {
        let _ = tx.send(Msg::Gone(path));
    }
}

fn stop_all(readers: &mut HashMap<PathBuf, Reader>) {
    for (_, r) in readers.drain() {
        r.stop.store(true, Ordering::Relaxed);
    }
}
