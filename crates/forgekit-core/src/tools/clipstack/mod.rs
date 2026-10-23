//! Clipstack: clipboard history for text and images.
//!
//! The daemon owns a [`Backend`] that watches the clipboard and can set it:
//! X11 (XFixes, also used through XWayland), wlroots/KDE (`wlr-data-control`),
//! or the GNOME Shell extension, which talks to the daemon over D-Bus.
//! Everything ends up in the SQLite [`store`], which the app and CLI read directly.

pub mod store;
pub mod wayland;
pub mod x11;

use serde::{Deserialize, Serialize};

/// One clipboard payload in one format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    pub mime: String,
    pub data: Vec<u8>,
}

impl Clip {
    pub fn text(s: &str) -> Self {
        Clip { mime: "text/plain;charset=utf-8".into(), data: s.as_bytes().to_vec() }
    }

    pub fn is_image(&self) -> bool {
        self.mime.starts_with("image/")
    }
}

/// Formats that mean "this came from a password manager; don't keep it".
pub const SECRET_MIMES: &[&str] = &["x-kde-passwordManagerHint", "application/x-nspasswd"];

/// Pick the one format worth storing from what the owner offers.
/// Images win over text (copying an image usually also offers a URL or HTML).
/// Returns `None` for secrets and for offers with nothing we can store.
pub fn choose_format<S: AsRef<str>>(offered: &[S]) -> Option<String> {
    let has = |m: &str| offered.iter().any(|o| o.as_ref() == m);
    if SECRET_MIMES.iter().any(|s| has(s)) {
        return None;
    }
    const PREFERENCE: &[&str] = &[
        "image/png",
        "image/jpeg",
        "image/webp",
        "image/bmp",
        "text/plain;charset=utf-8",
        "UTF8_STRING",
        "text/plain",
        "STRING",
        "TEXT",
    ];
    PREFERENCE.iter().find(|m| has(m)).map(|m| m.to_string())
}

/// Normalise X11 text targets to a MIME type for storage.
pub fn storage_mime(mime: &str) -> String {
    match mime {
        "UTF8_STRING" | "STRING" | "TEXT" | "text/plain" => "text/plain;charset=utf-8".into(),
        m => m.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub max_items: usize,
    pub max_text_kb: usize,
    pub max_image_kb: usize,
    /// 0 = never.
    pub expire_days: u32,
    /// Pause recording.
    pub private: bool,
    /// Ignore text that is only whitespace.
    pub skip_blank: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            max_items: 200,
            max_text_kb: 1024,
            max_image_kb: 5120,
            expire_days: 30,
            private: false,
            skip_blank: true,
        }
    }
}

impl Settings {
    pub fn load() -> anyhow::Result<Self> {
        crate::settings::load("clipstack")
    }

    pub fn save(&self) -> anyhow::Result<()> {
        crate::settings::save("clipstack", self)
    }

    /// Whether a clip should be recorded at all.
    pub fn accepts(&self, clip: &Clip) -> bool {
        if self.private || clip.data.is_empty() {
            return false;
        }
        let kb = clip.data.len() / 1024;
        if clip.is_image() {
            kb <= self.max_image_kb
        } else {
            kb <= self.max_text_kb
                && std::str::from_utf8(&clip.data).is_ok_and(|s| !(self.skip_blank && s.trim().is_empty()))
        }
    }
}

/// Events a backend reports and commands it accepts.
pub enum Command {
    /// Put this on the clipboard (we become the owner).
    Set(Clip),
    Stop,
}

/// Something a backend thread can run: watch the clipboard, report new clips,
/// and obey commands until told to stop.
pub trait Backend: Send {
    fn name(&self) -> &'static str;
    fn run(
        self: Box<Self>,
        on_clip: Box<dyn FnMut(Clip) + Send>,
        commands: std::sync::mpsc::Receiver<Command>,
        wake: std::os::fd::OwnedFd,
    ) -> anyhow::Result<()>;
}

/// A pipe used to wake a backend's poll loop when a command is queued.
pub fn wake_pipe() -> std::io::Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: pipe2 writes two fds into the array on success.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    use std::os::fd::FromRawFd;
    // SAFETY: both fds were just created and are owned by nobody else.
    Ok(unsafe { (std::os::fd::OwnedFd::from_raw_fd(fds[0]), std::os::fd::OwnedFd::from_raw_fd(fds[1])) })
}

pub fn wake(fd: &std::os::fd::OwnedFd) {
    use std::os::fd::AsRawFd;
    // SAFETY: writing one byte from a valid buffer to a valid fd.
    unsafe { libc::write(fd.as_raw_fd(), [1u8].as_ptr().cast(), 1) };
}

pub(crate) fn drain(fd: &std::os::fd::OwnedFd) {
    use std::os::fd::AsRawFd;
    let mut buf = [0u8; 64];
    // SAFETY: reading into a valid local buffer from a valid non-blocking fd.
    while unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
}

/// Wait until one of `fds` is readable (or `timeout_ms` passes). Returns readiness per fd.
pub(crate) fn poll(fds: &[i32], timeout_ms: i32) -> std::io::Result<Vec<bool>> {
    let mut p: Vec<libc::pollfd> =
        fds.iter().map(|&fd| libc::pollfd { fd, events: libc::POLLIN, revents: 0 }).collect();
    // SAFETY: `p` is a valid array of pollfd for its length.
    let r = unsafe { libc::poll(p.as_mut_ptr(), p.len() as libc::nfds_t, timeout_ms) };
    if r < 0 {
        let e = std::io::Error::last_os_error();
        if e.kind() == std::io::ErrorKind::Interrupted {
            return Ok(vec![false; fds.len()]);
        }
        return Err(e);
    }
    Ok(p.iter().map(|x| x.revents != 0).collect())
}

/// The best backend for this session, if any. GNOME on Wayland has no
/// data-control protocol; there the Shell extension feeds the daemon instead,
/// and XWayland's clipboard bridge is used as a fallback.
pub fn backend_for(caps: &crate::Capabilities) -> Option<Box<dyn Backend>> {
    use crate::capability::Session;
    if caps.session == Session::Wayland && caps.has_global("zwlr_data_control_manager_v1") {
        return Some(Box::new(wayland::WlrBackend));
    }
    if std::env::var_os("DISPLAY").is_some() {
        return Some(Box::new(x11::X11Backend));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_choice() {
        assert_eq!(choose_format(&["TARGETS", "UTF8_STRING", "image/png"]).as_deref(), Some("image/png"));
        assert_eq!(choose_format(&["text/html", "text/plain"]).as_deref(), Some("text/plain"));
        assert_eq!(choose_format(&["text/plain", "x-kde-passwordManagerHint"]), None);
        assert_eq!(choose_format(&["application/x-foo"]), None);
        assert_eq!(storage_mime("UTF8_STRING"), "text/plain;charset=utf-8");
    }

    #[test]
    fn settings_filter() {
        let s = Settings::default();
        assert!(s.accepts(&Clip::text("hi")));
        assert!(!s.accepts(&Clip::text("   \n")));
        assert!(!s.accepts(&Clip { mime: "text/plain".into(), data: vec![0xff, 0xfe] }));
        assert!(!s.accepts(&Clip { mime: "image/png".into(), data: vec![0; 6 * 1024 * 1024] }));
        assert!(!Settings { private: true, ..s }.accepts(&Clip::text("hi")));
    }
}
