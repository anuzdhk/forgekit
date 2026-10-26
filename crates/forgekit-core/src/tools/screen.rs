//! Screenshots through the xdg-desktop-portal (the only way on GNOME and
//! sandboxed apps), shared by Glyphgrab, Stage and Caliper.

use std::path::PathBuf;

use anyhow::Context;

/// Take a screenshot. `interactive` lets the user pick an area or window;
/// otherwise it's the whole screen (the desktop may ask for permission once).
/// The portal saves the image and this returns its path.
pub async fn capture(interactive: bool) -> anyhow::Result<PathBuf> {
    let shot = ashpd::desktop::screenshot::Screenshot::request()
        .interactive(interactive)
        .modal(interactive)
        .send()
        .await?
        .response()
        .context("screenshot cancelled or not allowed")?;
    let uri = shot.uri().as_str();
    let path = uri.strip_prefix("file://").with_context(|| format!("unexpected screenshot URI {uri}"))?;
    Ok(PathBuf::from(percent_decode(path)))
}

/// Blocking wrapper for code that isn't async (the GTK app, the CLI).
pub fn capture_blocking(interactive: bool) -> anyhow::Result<PathBuf> {
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(capture(interactive))
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode() {
        assert_eq!(
            percent_decode("/home/u/Pictures/Screenshot%20from%202026.png"),
            "/home/u/Pictures/Screenshot from 2026.png"
        );
        assert_eq!(percent_decode("/a%2"), "/a%2");
    }
}
