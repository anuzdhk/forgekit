//! The companion GNOME Shell extension (bundled in this binary): install,
//! remove, check its state, and write the settings file it reads.

use std::path::PathBuf;
use std::process::Command;

use forgekit_core::tools::shelf;
use gtk::glib;

pub const UUID: &str = "forgekit@anuzdhk.github.io";
const METADATA: &str = include_str!("../data/gnome-extension/metadata.json");
const EXTENSION: &str = include_str!("../data/gnome-extension/extension.js");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    NotInstalled,
    /// Files are in place but GNOME Shell hasn't loaded them (Wayland needs a new login).
    NeedsRelogin,
    Disabled,
    Active,
    /// Installed but GNOME reports an error (e.g. an unsupported Shell version).
    Error,
}

fn dir() -> PathBuf {
    glib::user_data_dir().join("gnome-shell/extensions").join(UUID)
}

pub fn state() -> State {
    if !dir().join("extension.js").exists() {
        return State::NotInstalled;
    }
    let Ok(out) = Command::new("gnome-extensions").args(["info", UUID]).output() else { return State::NeedsRelogin };
    let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
    if !out.status.success() || text.is_empty() {
        return State::NeedsRelogin;
    }
    let field = |name: &str| {
        text.lines().find_map(|l| l.trim().strip_prefix(name).map(|v| v.trim_start_matches(':').trim().to_string()))
    };
    match field("state").as_deref() {
        Some("active" | "enabled") => State::Active,
        Some("error" | "out of date") => State::Error,
        Some("initialized") if field("enabled").as_deref() == Some("yes") => State::NeedsRelogin,
        _ => State::Disabled,
    }
}

/// Copy the bundled extension into place and switch it on. Returns whether
/// it's already running (otherwise a new login loads it).
pub fn install() -> Result<bool, String> {
    let d = dir();
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    std::fs::write(d.join("metadata.json"), METADATA).map_err(|e| e.to_string())?;
    std::fs::write(d.join("extension.js"), EXTENSION).map_err(|e| e.to_string())?;
    write_config();
    let _ = Command::new("gnome-extensions").args(["enable", UUID]).status();
    // gnome-extensions can't enable what the Shell hasn't loaded yet; add it
    // to the enabled list so it starts at the next login.
    if state() != State::Active {
        enable_on_next_login();
    }
    Ok(state() == State::Active)
}

fn enable_on_next_login() {
    let Ok(out) = Command::new("gsettings").args(["get", "org.gnome.shell", "enabled-extensions"]).output() else {
        return;
    };
    let current = String::from_utf8_lossy(&out.stdout).to_string();
    if current.contains(UUID) {
        return;
    }
    let list = current
        .trim()
        .trim_start_matches("@as")
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim()
        .to_string();
    let new = if list.is_empty() { format!("['{UUID}']") } else { format!("[{list}, '{UUID}']") };
    let _ = Command::new("gsettings").args(["set", "org.gnome.shell", "enabled-extensions", &new]).status();
}

pub fn remove() -> Result<(), String> {
    let _ = Command::new("gnome-extensions").args(["disable", UUID]).status();
    std::fs::remove_dir_all(dir()).map_err(|e| e.to_string())
}

/// `~/.config/forgekit/shell.json`: the app's path and the shelf settings.
pub fn write_config() {
    let s = shelf::Settings::load().unwrap_or_default();
    let app = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    let json = format!(
        "{{\"app\": {app:?}, \"auto_show\": {}, \"files_only\": {}, \"corner\": {:?}, \"delay_ms\": 350}}\n",
        s.auto_show, s.files_only, s.corner
    );
    let path = forgekit_core::paths::config_dir().join("shell.json");
    let _ = std::fs::create_dir_all(forgekit_core::paths::config_dir());
    let _ = std::fs::write(path, json);
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundled_metadata_matches() {
        assert!(super::METADATA.contains(super::UUID));
        assert!(super::EXTENSION.contains("export default class"));
    }
}
