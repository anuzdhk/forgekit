//! Update checks against GitHub releases.
//!
//! One anonymous request to the GitHub API, at most once a day, and only when
//! the user leaves "Check for updates" on. Nothing about the user is sent.

use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

const API: &str = "https://api.github.com/repos/anuzdhk/forgekit/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/anuzdhk/forgekit/releases";
const DAY: u64 = 24 * 3600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    UpToDate,
    /// The project hasn't published any release yet.
    NoReleases,
    Available(Release),
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    checked_at: u64,
    latest: Option<Release>,
}

fn cache_file() -> std::path::PathBuf {
    crate::paths::data_dir("updates").join("last-check.json")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `v0.10.2` → (0, 10, 2). Pre-release suffixes are ignored.
pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.trim().trim_start_matches(['v', 'V']).split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next().flatten().unwrap_or(0), it.next().flatten().unwrap_or(0)))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((parse_version(candidate), parse_version(current)), (Some(a), Some(b)) if a > b)
}

/// Fetch with curl (or wget). Returns (HTTP status, body).
fn fetch(url: &str) -> anyhow::Result<(u16, String)> {
    let ua = format!("Forgekit/{}", env!("CARGO_PKG_VERSION"));
    if crate::paths::which("curl").is_some() {
        let out = Command::new("curl")
            .args([
                "-sSL",
                "--max-time",
                "10",
                "-H",
                "Accept: application/vnd.github+json",
                "-A",
                &ua,
                "-w",
                "\n%{http_code}",
                url,
            ])
            .output()
            .context("running curl")?;
        if !out.status.success() {
            bail!("couldn't reach GitHub: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let (body, code) = text.rsplit_once('\n').unwrap_or(("", "0"));
        return Ok((code.trim().parse().unwrap_or(0), body.to_string()));
    }
    if crate::paths::which("wget").is_some() {
        let out = Command::new("wget")
            .args(["-q", "-O", "-", "--timeout=10", "-U", &ua, url])
            .output()
            .context("running wget")?;
        // wget exits 8 for HTTP errors; treat as "not found" for our purposes.
        return Ok((if out.status.success() { 200 } else { 404 }, String::from_utf8_lossy(&out.stdout).to_string()));
    }
    bail!("install curl or wget to check for updates")
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
}

/// Ask GitHub now (ignores the daily limit) and remember the answer.
pub fn check_now(current: &str) -> anyhow::Result<Outcome> {
    let (code, body) = fetch(API)?;
    let latest = match code {
        200 => {
            let r: GhRelease = serde_json::from_str(&body).context("unexpected reply from GitHub")?;
            Some(Release {
                version: r.tag_name.trim_start_matches('v').to_string(),
                url: r.html_url,
                notes: r.body.unwrap_or_default(),
            })
        }
        404 => None,
        403 | 429 => bail!("GitHub is rate-limiting update checks; try again later"),
        c => bail!("GitHub answered with HTTP {c}"),
    };
    let cache = Cache { checked_at: now(), latest: latest.clone() };
    if let Some(dir) = cache_file().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(cache_file(), serde_json::to_string(&cache)?);
    Ok(outcome(latest, current))
}

fn outcome(latest: Option<Release>, current: &str) -> Outcome {
    match latest {
        None => Outcome::NoReleases,
        Some(r) if is_newer(&r.version, current) => Outcome::Available(r),
        Some(_) => Outcome::UpToDate,
    }
}

/// The daily check: uses the cached answer if it's less than a day old.
pub fn check_daily(current: &str) -> anyhow::Result<Outcome> {
    let cache: Option<Cache> = std::fs::read_to_string(cache_file()).ok().and_then(|s| serde_json::from_str(&s).ok());
    match cache {
        Some(c) if now().saturating_sub(c.checked_at) < DAY => Ok(outcome(c.latest, current)),
        _ => check_now(current),
    }
}

/// When the last check happened, for display.
pub fn last_checked() -> Option<Duration> {
    let c: Cache = serde_json::from_str(&std::fs::read_to_string(cache_file()).ok()?).ok()?;
    Some(Duration::from_secs(now().saturating_sub(c.checked_at)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v0.10.2"), Some((0, 10, 2)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("0.4.0-beta.1"), Some((0, 4, 0)));
        assert_eq!(parse_version("nightly"), None);
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("garbage", "0.3.0"));
        let r = Release { version: "0.4.0".into(), url: String::new(), notes: String::new() };
        assert_eq!(outcome(Some(r.clone()), "0.3.0"), Outcome::Available(r));
        assert_eq!(outcome(None, "0.3.0"), Outcome::NoReleases);
    }
}
