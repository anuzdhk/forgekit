//! Bootpilot: what starts when you log in. Autostart entries (the XDG
//! standard every desktop follows) and your systemd user services.
//!
//! System entries in /etc/xdg/autostart are never edited: switching one off
//! writes a same-named copy in ~/.config/autostart that turns it off, which
//! is how the spec lets users override them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// File name, e.g. `org.gnome.Software.desktop`.
    pub id: String,
    pub name: String,
    pub comment: String,
    pub icon: String,
    pub enabled: bool,
    /// Comes from /etc/xdg/autostart (a user copy may override it).
    pub system: bool,
    /// Only applies to other desktops (OnlyShowIn/NotShowIn).
    pub other_desktop: bool,
}

pub fn user_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("autostart")
}

fn system_dirs() -> Vec<PathBuf> {
    std::env::var("XDG_CONFIG_DIRS")
        .unwrap_or_else(|_| "/etc/xdg".into())
        .split(':')
        .filter(|s| !s.is_empty())
        .map(|d| Path::new(d).join("autostart"))
        .collect()
}

/// `[Desktop Entry]` keys of a .desktop file.
fn keys(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some((k, v)) = line.split_once('=') {
            out.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
        }
    }
    out
}

fn is_enabled(k: &BTreeMap<String, String>) -> bool {
    k.get("Hidden").is_none_or(|v| v != "true") && k.get("X-GNOME-Autostart-enabled").is_none_or(|v| v != "false")
}

pub fn list() -> Vec<Entry> {
    let desktops: Vec<String> =
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().split(':').map(str::to_string).collect();
    let mut by_id: BTreeMap<String, (PathBuf, bool)> = BTreeMap::new();
    for dir in system_dirs().into_iter().rev() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if e.path().extension().is_some_and(|x| x == "desktop") {
                by_id.insert(e.file_name().to_string_lossy().into_owned(), (e.path(), true));
            }
        }
    }
    let mut out = Vec::new();
    let user = user_dir();
    for e in std::fs::read_dir(&user).into_iter().flatten().flatten() {
        if e.path().extension().is_some_and(|x| x == "desktop") {
            let id = e.file_name().to_string_lossy().into_owned();
            let system = by_id.contains_key(&id);
            by_id.insert(id, (e.path(), system));
        }
    }
    for (id, (path, system)) in by_id {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let k = keys(&text);
        let list = |key: &str| {
            k.get(key)
                .map(|v| v.split(';').filter(|s| !s.is_empty()).map(str::to_string).collect::<Vec<_>>())
                .unwrap_or_default()
        };
        let only = list("OnlyShowIn");
        let other_desktop = (!only.is_empty() && !only.iter().any(|d| desktops.contains(d)))
            || list("NotShowIn").iter().any(|d| desktops.contains(d));
        out.push(Entry {
            name: k.get("Name").cloned().unwrap_or_else(|| id.trim_end_matches(".desktop").to_string()),
            comment: k.get("Comment").cloned().unwrap_or_default(),
            icon: k.get("Icon").cloned().unwrap_or_default(),
            enabled: is_enabled(&k),
            system,
            other_desktop,
            id,
        });
    }
    out.sort_by(|a, b| a.other_desktop.cmp(&b.other_desktop).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

fn source(id: &str) -> Option<PathBuf> {
    let user = user_dir().join(id);
    if user.exists() {
        return Some(user);
    }
    system_dirs().into_iter().map(|d| d.join(id)).find(|p| p.exists())
}

/// Switch an entry on or off (writing a user copy for system entries).
pub fn set_enabled(id: &str, on: bool) -> anyhow::Result<()> {
    validate_id(id)?;
    let src = source(id).with_context(|| format!("no autostart entry {id}"))?;
    let text = std::fs::read_to_string(&src)?;
    let mut out = String::new();
    let mut in_entry = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            if in_entry && !on {
                out.push_str("X-GNOME-Autostart-enabled=false\nHidden=true\n");
            }
            in_entry = t == "[Desktop Entry]";
        }
        if in_entry && (t.starts_with("Hidden=") || t.starts_with("X-GNOME-Autostart-enabled=")) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if in_entry && !on {
        out.push_str("X-GNOME-Autostart-enabled=false\nHidden=true\n");
    }
    std::fs::create_dir_all(user_dir())?;
    std::fs::write(user_dir().join(id), out)?;
    Ok(())
}

/// Start an app at login: copy its launcher into the autostart folder.
pub fn add(desktop_file: &Path) -> anyhow::Result<String> {
    let id = desktop_file.file_name().and_then(|n| n.to_str()).context("bad launcher path")?.to_string();
    validate_id(&id)?;
    std::fs::create_dir_all(user_dir())?;
    std::fs::copy(desktop_file, user_dir().join(&id))?;
    set_enabled(&id, true)?;
    Ok(id)
}

/// Remove a user entry. System entries can only be switched off.
pub fn remove(id: &str) -> anyhow::Result<()> {
    validate_id(id)?;
    if system_dirs().iter().any(|d| d.join(id).exists()) {
        bail!("{id} comes with the system; switch it off instead");
    }
    std::fs::remove_file(user_dir().join(id)).with_context(|| format!("removing {id}"))
}

fn validate_id(id: &str) -> anyhow::Result<()> {
    if id.contains('/') || !id.ends_with(".desktop") {
        bail!("'{id}' isn't an autostart entry name");
    }
    Ok(())
}

// ------------------------------------------------------------------ user services

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserService {
    pub unit: String,
    pub enabled: bool,
}

/// systemd user services that are enabled or disabled (not static ones).
pub fn user_services() -> Vec<UserService> {
    let Ok(out) = Command::new("systemctl")
        .args(["--user", "list-unit-files", "--type=service", "--state=enabled,disabled", "--no-legend", "--no-pager"])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (unit, state) = (f.next()?, f.next()?);
            (!unit.contains('@')).then(|| UserService { unit: unit.to_string(), enabled: state == "enabled" })
        })
        .collect()
}

pub fn set_user_service(unit: &str, on: bool) -> anyhow::Result<()> {
    if !unit.ends_with(".service") || unit.contains('/') {
        bail!("'{unit}' isn't a service name");
    }
    let st = Command::new("systemctl").args(["--user", if on { "enable" } else { "disable" }, unit]).status()?;
    if !st.success() {
        bail!("systemctl couldn't {} {unit}", if on { "enable" } else { "disable" });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_flags() {
        let k = keys("[Desktop Entry]\nName=A\nX-GNOME-Autostart-enabled=false\n[Desktop Action x]\nHidden=true\n");
        assert!(!is_enabled(&k));
        assert!(is_enabled(&keys("[Desktop Entry]\nName=A\n")));
        assert!(!is_enabled(&keys("[Desktop Entry]\nHidden=true\n")));
        assert!(validate_id("../x.desktop").is_err());
    }
}
