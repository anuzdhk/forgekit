//! Nightowl: keep the PC awake.
//!
//! Two inhibitors are taken, because desktops disagree about which one they
//! honour: the Inhibit portal (screen blanking and idle on GNOME/KDE) and a
//! logind `sleep:idle` lock (suspend everywhere systemd runs). Both are held
//! for as long as the [`Inhibitor`] lives.

use std::os::fd::OwnedFd;
use std::time::Duration;

use anyhow::Context;
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedObjectPath, Value};

pub struct Inhibitor {
    _logind: Option<OwnedFd>,
    _portal: Option<(Connection, OwnedObjectPath)>,
}

impl Inhibitor {
    pub fn acquire(reason: &str) -> anyhow::Result<Self> {
        let logind = logind(reason).ok();
        let portal = portal(reason).ok();
        if logind.is_none() && portal.is_none() {
            anyhow::bail!("Neither logind nor the Inhibit portal accepted the request");
        }
        Ok(Inhibitor { _logind: logind, _portal: portal })
    }

    /// Which inhibitors are actually held, for display.
    pub fn describe(&self) -> &'static str {
        match (&self._logind, &self._portal) {
            (Some(_), Some(_)) => "sleep and screen blanking blocked",
            (Some(_), None) => "sleep blocked (the screen may still blank)",
            (None, Some(_)) => "idle blocked via the desktop portal",
            (None, None) => unreachable!(),
        }
    }
}

fn logind(reason: &str) -> anyhow::Result<OwnedFd> {
    let conn = Connection::system()?;
    let fd: zbus::zvariant::OwnedFd = conn
        .call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "Inhibit",
            &("sleep:idle", "Forgekit Nightowl", reason, "block"),
        )
        .context("logind Inhibit")?
        .body()
        .deserialize()?;
    Ok(fd.into())
}

fn portal(reason: &str) -> anyhow::Result<(Connection, OwnedObjectPath)> {
    const SUSPEND: u32 = 4;
    const IDLE: u32 = 8;
    let conn = Connection::session()?;
    let mut opts = std::collections::HashMap::<&str, Value>::new();
    opts.insert("reason", reason.into());
    let handle: OwnedObjectPath = conn
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.Inhibit"),
            "Inhibit",
            &("", SUSPEND | IDLE, opts),
        )
        .context("portal Inhibit")?
        .body()
        .deserialize()?;
    // The inhibition lasts until the request is closed or this connection drops.
    Ok((conn, handle))
}

/// Parse durations like `90`, `90s`, `45m`, `2h`, `1h30m`.
pub fn parse_duration(s: &str) -> anyhow::Result<Duration> {
    let s = s.trim();
    if let Ok(secs) = s.parse::<u64>() {
        return Ok(Duration::from_secs(secs * 60));
    }
    let mut total = 0u64;
    let mut num = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let n: u64 = num.parse().with_context(|| format!("bad duration '{s}'"))?;
        num.clear();
        total += n * match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86400,
            _ => anyhow::bail!("bad duration unit '{c}' in '{s}'"),
        };
    }
    anyhow::ensure!(num.is_empty() && total > 0, "bad duration '{s}' (try 30m, 2h, 1h30m)");
    Ok(Duration::from_secs(total))
}

pub fn process_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("30").unwrap(), Duration::from_secs(1800));
        assert_eq!(parse_duration("1h30m").unwrap(), Duration::from_secs(5400));
        assert_eq!(parse_duration("45s").unwrap(), Duration::from_secs(45));
        assert!(parse_duration("1x").is_err());
        assert!(parse_duration("1h3").is_err());
    }
}
