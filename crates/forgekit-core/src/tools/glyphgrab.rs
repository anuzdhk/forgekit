//! Glyphgrab: let the user select an area through the Screenshot portal, then
//! read its text with Tesseract.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

/// Interactive screenshot via the portal; returns the saved image path.
pub async fn capture() -> anyhow::Result<PathBuf> {
    super::screen::capture(true).await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidies_tesseract_output() {
        assert_eq!(tidy("hello\n\n\u{c}"), "hello");
    }
}
