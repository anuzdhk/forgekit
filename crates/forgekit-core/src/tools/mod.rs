pub mod bootpilot;
pub mod cheatsheet;
pub mod clipstack;
pub mod envy;
pub mod eyedrop;
pub mod glance;
pub mod glyphgrab;
pub mod hostbook;
pub mod lockpick;
pub mod mosaic;
pub mod nightowl;
pub mod relabel;
pub mod roam;
pub mod scenes;
pub mod screen;
pub mod servicedeck;
pub mod settings_explorer;
pub mod shelf;
pub mod shrinkray;
pub mod spark;
pub mod sweep;
pub mod teku;

use std::io::Write;
use std::process::{Command, Stdio};

use crate::Capabilities;

/// Put text on the clipboard: through wl-copy/xclip/xsel when installed,
/// otherwise through forgekitd's clipboard backend. Returns `false` when
/// neither is available.
pub fn copy_to_clipboard(caps: &Capabilities, text: &str) -> anyhow::Result<bool> {
    if let Some(cmd) = caps.clipboard_cmd() {
        let mut child = Command::new(cmd[0]).args(&cmd[1..]).stdin(Stdio::piped()).spawn()?;
        child.stdin.take().unwrap().write_all(text.as_bytes())?;
        // wl-copy/xclip fork to serve the selection; the parent exits quickly.
        child.wait()?;
        return Ok(true);
    }
    let Ok(conn) = zbus::blocking::Connection::session() else { return Ok(false) };
    Ok(conn
        .call_method(
            Some("org.forgekit.Daemon"),
            "/org/forgekit/Clipstack",
            Some("org.forgekit.Clipstack1"),
            "SetText",
            &text,
        )
        .is_ok())
}

/// A desktop notification (for actions started from a keyboard shortcut,
/// where there's no terminal to print to).
pub fn notify(summary: &str, body: &str) {
    let Ok(conn) = zbus::blocking::Connection::session() else { return };
    let hints = std::collections::HashMap::<&str, zbus::zvariant::Value>::new();
    let _ = conn.call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "Notify",
        &("Forgekit", 0u32, "io.github.anuzdhk.Forgekit", summary, body, Vec::<&str>::new(), hints, 4000i32),
    );
}
