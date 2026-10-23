pub mod eyedrop;
pub mod glyphgrab;
pub mod hostbook;
pub mod lockpick;
pub mod nightowl;
pub mod relabel;
pub mod shrinkray;
pub mod spark;
pub mod teku;

use std::io::Write;
use std::process::{Command, Stdio};

use crate::Capabilities;

/// Put text on the clipboard with whatever helper the session has.
/// Returns `false` when no helper is installed.
pub fn copy_to_clipboard(caps: &Capabilities, text: &str) -> anyhow::Result<bool> {
    let Some(cmd) = caps.clipboard_cmd() else {
        return Ok(false);
    };
    let mut child = Command::new(cmd[0]).args(&cmd[1..]).stdin(Stdio::piped()).spawn()?;
    child.stdin.take().unwrap().write_all(text.as_bytes())?;
    // wl-copy/xclip fork to serve the selection; the parent exits quickly.
    child.wait()?;
    Ok(true)
}
