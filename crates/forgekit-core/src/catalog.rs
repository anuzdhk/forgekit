//! Every Forgekit tool, its tier, and how to decide whether it works here.

use serde::Serialize;

use crate::capability::{Capabilities, Desktop, Session, Status};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    Universal,
    Helper,
    Desktop,
    LinuxExtra,
}

impl Tier {
    pub fn title(self) -> &'static str {
        match self {
            Tier::Universal => "Universal",
            Tier::Helper => "Needs the privileged helper",
            Tier::Desktop => "Per-desktop",
            Tier::LinuxExtra => "Linux extras",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Tool {
    /// CLI name: `forgekit <id>`.
    pub id: &'static str,
    pub name: &'static str,
    pub tier: Tier,
    pub summary: &'static str,
    /// Release that ships the tool. Tools from later releases report as unsupported.
    pub since: &'static str,
    #[serde(skip)]
    check: fn(&Capabilities) -> Status,
}

/// The release this build implements.
pub const CURRENT: &str = "0.1";

impl Tool {
    pub fn status(&self, caps: &Capabilities) -> Status {
        if version_key(self.since) > version_key(CURRENT) {
            return Status::Unsupported(format!("Coming in v{}", self.since));
        }
        (self.check)(caps)
    }
}

fn version_key(v: &str) -> (u32, u32) {
    let mut it = v.split('.').map(|p| p.parse().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0))
}

pub fn find(id: &str) -> Option<&'static Tool> {
    CATALOG.iter().find(|t| t.id.eq_ignore_ascii_case(id) || t.name.eq_ignore_ascii_case(id))
}

fn always(_: &Capabilities) -> Status {
    Status::Supported
}

fn needs_graphical(c: &Capabilities) -> Status {
    match c.session {
        Session::Wayland | Session::X11 => Status::Supported,
        _ => Status::Unsupported("Needs a graphical session".into()),
    }
}

fn screenshot_portal(c: &Capabilities) -> Status {
    if c.portal("Screenshot").is_some() {
        needs_graphical(c)
    } else {
        Status::NeedsSetup("Install xdg-desktop-portal and your desktop's portal backend".into())
    }
}

fn eyedrop(c: &Capabilities) -> Status {
    match c.portal("Screenshot") {
        Some(v) if v >= 2 => needs_graphical(c),
        Some(_) => Status::Unsupported("The Screenshot portal here is too old for PickColor (needs v2)".into()),
        None => screenshot_portal(c),
    }
}

fn glyphgrab(c: &Capabilities) -> Status {
    match screenshot_portal(c) {
        Status::Supported if !c.has_bin("tesseract") => {
            Status::NeedsSetup("Install tesseract-ocr (and a language pack, e.g. tesseract-ocr-eng)".into())
        }
        s => s,
    }
}

fn nightowl(_: &Capabilities) -> Status {
    if std::path::Path::new("/run/systemd/system").exists() {
        Status::Supported
    } else {
        Status::Unsupported("Needs systemd-logind for sleep inhibitors".into())
    }
}

fn hostbook(c: &Capabilities) -> Status {
    if c.has_bin("pkexec") {
        Status::Supported
    } else {
        Status::NeedsSetup("Install polkit (pkexec) to save /etc/hosts".into())
    }
}

fn lockpick(_: &Capabilities) -> Status {
    if std::path::Path::new("/proc/self/fd").exists() {
        Status::Supported
    } else {
        Status::Unsupported("Needs /proc".into())
    }
}

fn teku(c: &Capabilities) -> Status {
    if c.has_bin("teku") {
        Status::Supported
    } else {
        Status::NeedsSetup("Install Teku: https://github.com/anuzdhk/teku".into())
    }
}

fn clipstack(c: &Capabilities) -> Status {
    if c.has_data_control() {
        Status::Supported
    } else if c.desktop == Desktop::Gnome {
        Status::NeedsSetup("Enable the Forgekit GNOME Shell extension".into())
    } else {
        Status::Unsupported("This compositor has no clipboard data-control protocol".into())
    }
}

fn helper(c: &Capabilities) -> Status {
    if c.helper_installed {
        Status::Supported
    } else {
        Status::NeedsSetup("Install the Forgekit helper (one-time password prompt)".into())
    }
}

fn window_backend(c: &Capabilities) -> Status {
    match (c.session, c.desktop) {
        (Session::X11, _) => Status::Supported,
        (_, Desktop::Gnome) => Status::NeedsSetup("Enable the Forgekit GNOME Shell extension".into()),
        (_, Desktop::Kde | Desktop::Hyprland | Desktop::Sway) => Status::Supported,
        _ => Status::Unsupported("No window-management backend for this desktop yet".into()),
    }
}

fn overlay(c: &Capabilities) -> Status {
    if c.session == Session::X11 || c.has_layer_shell() {
        Status::Supported
    } else if c.desktop == Desktop::Gnome {
        Status::NeedsSetup("Enable the Forgekit GNOME Shell extension".into())
    } else {
        Status::Unsupported("Needs layer-shell, which this compositor lacks".into())
    }
}

fn roam(c: &Capabilities) -> Status {
    if c.session == Session::X11 || (c.portal("InputCapture").is_some() && c.portal("RemoteDesktop").is_some()) {
        Status::Supported
    } else {
        Status::Unsupported("Needs the InputCapture and RemoteDesktop portals".into())
    }
}

macro_rules! tool {
    ($id:literal, $name:literal, $tier:ident, $since:literal, $check:expr, $summary:literal) => {
        Tool { id: $id, name: $name, tier: Tier::$tier, since: $since, check: $check, summary: $summary }
    };
}

pub static CATALOG: &[Tool] = &[
    tool!(
        "spark",
        "Spark",
        Universal,
        "0.1",
        needs_graphical,
        "Launcher and command bar: apps, calculator, web search"
    ),
    tool!("eyedrop", "Eyedrop", Universal, "0.1", eyedrop, "Pick any color on screen and copy it as HEX/RGB/HSL"),
    tool!("nightowl", "Nightowl", Universal, "0.1", nightowl, "Keep the PC awake, for a while or while a process runs"),
    tool!("glyphgrab", "Glyphgrab", Universal, "0.1", glyphgrab, "Copy the text in any area of the screen (OCR)"),
    tool!("relabel", "Relabel", Universal, "0.1", always, "Bulk-rename files with find/replace, regex and numbering"),
    tool!("shrinkray", "Shrinkray", Universal, "0.1", always, "Bulk-resize and convert images"),
    tool!("glance", "Glance", Universal, "0.2", needs_graphical, "Quick preview of any file with a key press"),
    tool!(
        "hostbook",
        "Hostbook",
        Universal,
        "0.1",
        hostbook,
        "Safe /etc/hosts editor with backups and per-entry toggles"
    ),
    tool!("envy", "Envy", Universal, "0.2", always, "Edit user and system environment variables"),
    tool!("lockpick", "Lockpick", Universal, "0.1", lockpick, "Show which process is holding a file, and close it"),
    tool!("caliper", "Caliper", Universal, "0.5", screenshot_portal, "Measure pixel distances on screen"),
    tool!("dusk", "Dusk", Universal, "0.2", needs_graphical, "Automatic light/dark theme by time or sunset"),
    tool!("clipstack", "Clipstack", Universal, "0.2", clipstack, "Clipboard history for text and images"),
    tool!("shelf", "DropShelf", Universal, "0.2", needs_graphical, "Hold files while you move between apps"),
    tool!("teku", "Teku", Universal, "0.1", teku, "Install .deb, .rpm, .AppImage and .flatpakref files"),
    tool!("keysmith", "Keysmith", Helper, "0.3", helper, "Remap keys, shortcuts and key sequences"),
    tool!("umlaut", "Umlaut", Helper, "0.3", helper, "Hold a letter to pick an accented version"),
    tool!("mosaic", "Mosaic", Desktop, "0.4", window_backend, "Custom window zones and layouts"),
    tool!("pin", "Pin", Desktop, "0.4", window_backend, "Keep any window always on top"),
    tool!("scenes", "Scenes", Desktop, "0.6", window_backend, "Launch and arrange a set of apps in one click"),
    tool!("cheatsheet", "Cheatsheet", Desktop, "0.5", overlay, "Overlay that shows keyboard shortcuts"),
    tool!("stage", "Stage", Desktop, "0.5", overlay, "Zoom, draw on screen and present"),
    tool!("beacon", "Beacon", Desktop, "0.5", overlay, "Find and highlight the mouse cursor"),
    tool!("roam", "Roam", Desktop, "0.6", roam, "Share one mouse and keyboard across PCs"),
    tool!("sweep", "Sweep", LinuxExtra, "0.6", always, "Clean package caches, old runtimes, logs and thumbnails"),
    tool!("bootpilot", "Bootpilot", LinuxExtra, "0.6", always, "Manage startup apps and systemd user units"),
    tool!("servicedeck", "Servicedeck", LinuxExtra, "0.6", always, "Start, stop and inspect systemd services"),
    tool!("settings-explorer", "Settings Explorer", LinuxExtra, "0.6", always, "Browse dconf/gsettings and KDE config"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_unique_and_findable() {
        let mut ids: Vec<_> = CATALOG.iter().map(|t| t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
        assert_eq!(find("DropShelf").unwrap().id, "shelf");
    }

    #[test]
    fn future_tools_report_their_release() {
        let caps = Capabilities::detect();
        let s = find("mosaic").unwrap().status(&caps);
        assert_eq!(s, Status::Unsupported("Coming in v0.4".into()));
    }
}
