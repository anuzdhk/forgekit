//! Glyphgrab: let the user select an area through the Screenshot portal, then
//! read its text with Tesseract.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

/// Interactive screenshot via the portal; returns the saved image path.
pub async fn capture() -> anyhow::Result<PathBuf> {
    let shot = ashpd::desktop::screenshot::Screenshot::request()
        .interactive(true)
        .modal(true)
        .send()
        .await?
        .response()
        .context("screenshot cancelled")?;
    let uri = shot.uri().as_str();
    let path = uri.strip_prefix("file://").with_context(|| format!("unexpected screenshot URI {uri}"))?;
    Ok(PathBuf::from(percent_decode(path)))
}

/// OCR an image. `lang` is a Tesseract language code such as `eng` or `eng+deu`.
pub fn ocr(image: &Path, lang: &str) -> anyhow::Result<String> {
    let out = Command::new("tesseract")
        .arg(image)
        .arg("stdout")
        .args(["-l", lang])
        .output()
        .context("running tesseract (is tesseract-ocr installed?)")?;
    if !out.status.success() {
        bail!("tesseract failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(tidy(&String::from_utf8_lossy(&out.stdout)))
}

/// Tesseract ends output with form feeds and blank lines; trim them.
fn tidy(s: &str) -> String {
    s.replace('\u{c}', "").trim().to_string()
}

fn percent_decode(s: &str) -> String {
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
        assert_eq!(tidy("hello\n\n\u{c}"), "hello");
    }
}
