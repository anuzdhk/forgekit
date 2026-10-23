//! Teku integration. Teku stays its own program; Forgekit drives its CLI.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, bail};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detection {
    pub file: PathBuf,
    /// `deb`, `rpm`, `appimage`, `flatpakref`, `unknown`, …
    pub kind: String,
    pub os: String,
    pub route: Route,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "lowercase")]
pub enum Route {
    /// Teku can install it this way (e.g. `apt`, `appimage`).
    Install { how: String },
    /// Teku can't, with a title and a plain explanation.
    Error { title: String, detail: String },
}

/// `teku --detect FILE…`
pub fn detect(files: &[PathBuf]) -> anyhow::Result<Vec<Detection>> {
    let out = teku().arg("--detect").args(files).output().context("running teku")?;
    if !out.status.success() {
        bail!("teku --detect failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    String::from_utf8_lossy(&out.stdout).lines().map(parse_detect).collect()
}

fn parse_detect(line: &str) -> anyhow::Result<Detection> {
    let mut fields = line.split('\t');
    let file = PathBuf::from(fields.next().unwrap_or_default());
    let (mut kind, mut os, mut route) = (String::new(), String::new(), String::new());
    for f in fields {
        match f.split_once('=') {
            Some(("type", v)) => kind = v.into(),
            Some(("os", v)) => os = v.into(),
            Some(("route", v)) => route = v.into(),
            _ => {}
        }
    }
    let route = match route.strip_prefix("err:") {
        Some(err) => {
            let (title, detail) = err.split_once('|').unwrap_or((err, ""));
            Route::Error { title: title.into(), detail: detail.into() }
        }
        None if route.is_empty() => bail!("unexpected teku output: {line}"),
        None => Route::Install { how: route },
    };
    Ok(Detection { file, kind, os, route })
}

/// Install with `TEKU_NO_GUI=1 teku -y FILE`, handing each output line to `on_line`.
pub fn install(file: &Path, mut on_line: impl FnMut(&str)) -> anyhow::Result<()> {
    let mut child =
        teku().arg("-y").arg(file).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().context("running teku")?;
    // Merge stderr into the same stream so the user sees errors in order-ish.
    let stderr = child.stderr.take().unwrap();
    let err_thread =
        std::thread::spawn(move || BufReader::new(stderr).lines().map_while(Result::ok).collect::<Vec<_>>());
    for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
        on_line(&line);
    }
    for line in err_thread.join().unwrap_or_default() {
        on_line(&line);
    }
    let status = child.wait()?;
    if !status.success() {
        bail!("Teku couldn't install {}", file.display());
    }
    Ok(())
}

pub fn uninstall_appimage(file: &Path) -> anyhow::Result<()> {
    let status = teku().arg("-y").arg("--uninstall-appimage").arg(file).status()?;
    if !status.success() {
        bail!("Teku couldn't remove {}", file.display());
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryEntry {
    pub when: String,
    pub ok: bool,
    pub kind: String,
    pub file: PathBuf,
}

pub fn history_path() -> PathBuf {
    dirs::state_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".local/state"))
        .join("teku/history.log")
}

/// Newest first.
pub fn history() -> Vec<HistoryEntry> {
    let text = std::fs::read_to_string(history_path()).unwrap_or_default();
    let mut v: Vec<_> = text.lines().filter_map(parse_history).collect();
    v.reverse();
    v
}

fn parse_history(line: &str) -> Option<HistoryEntry> {
    let mut f = line.splitn(4, '\t');
    Some(HistoryEntry { when: f.next()?.into(), ok: f.next()? == "ok", kind: f.next()?.into(), file: f.next()?.into() })
}

fn teku() -> Command {
    let mut c = Command::new("teku");
    c.env("TEKU_NO_GUI", "1");
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_detect_lines() {
        let d = parse_detect(
            "a.AppImage\ttype=unknown\tos=debian\troute=err:Unsupported file type|Teku can install .deb files.",
        )
        .unwrap();
        assert_eq!(d.kind, "unknown");
        assert_eq!(
            d.route,
            Route::Error { title: "Unsupported file type".into(), detail: "Teku can install .deb files.".into() }
        );
        let ok = parse_detect("x.deb\ttype=deb\tos=debian\troute=apt").unwrap();
        assert_eq!(ok.route, Route::Install { how: "apt".into() });
    }

    #[test]
    fn parses_history_lines() {
        let h = parse_history("2026-10-22T21:20:39+05:45\tok\tappimage\t/home/u/a b.AppImage").unwrap();
        assert!(h.ok);
        assert_eq!(h.file, PathBuf::from("/home/u/a b.AppImage"));
        assert!(parse_history("garbage").is_none());
    }
}
