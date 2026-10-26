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

/// Task-based grouping used by the app's sidebar. Tiers describe what a tool
/// needs from the system; categories describe what people use it for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Everyday,
    Files,
    System,
    Input,
    Windows,
    Extras,
}

impl Category {
    pub const ALL: [Category; 6] =
        [Category::Everyday, Category::Files, Category::System, Category::Input, Category::Windows, Category::Extras];

    pub fn title(self) -> &'static str {
        match self {
            Category::Everyday => "Everyday",
            Category::Files => "Files",
            Category::System => "System",
            Category::Input => "Keyboard",
            Category::Windows => "Windows",
            Category::Extras => "Maintenance",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Tool {
    /// CLI name: `forgekit <id>`.
    pub id: &'static str,
    pub name: &'static str,
    pub tier: Tier,
    pub category: Category,
    /// Tabler icon name (the app ships these as `tabler-<icon>-symbolic`).
    pub icon: &'static str,
    pub summary: &'static str,
    /// Release that ships the tool. Tools from later releases report as unsupported.
    pub since: &'static str,
    #[serde(skip)]
    check: fn(&Capabilities) -> Status,
}

/// The release this build implements.
pub const CURRENT: &str = "0.7";

impl Tool {
    /// Not in this build yet.
    pub fn is_planned(&self) -> bool {
        version_key(self.since) > version_key(CURRENT)
    }

    pub fn status(&self, caps: &Capabilities) -> Status {
        if self.is_planned() {
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
    CATALOG.iter().find(|t| {
        t.id.eq_ignore_ascii_case(id)
            || t.name.eq_ignore_ascii_case(id)
            || t.name.replace(' ', "-").eq_ignore_ascii_case(id)
    })
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
    // GNOME has no data-control protocol, but its XWayland clipboard bridge
    // lets the X11 backend see and set the Wayland clipboard.
    if c.has_data_control() || std::env::var_os("DISPLAY").is_some() {
        Status::Supported
    } else if c.desktop == Desktop::Gnome {
        Status::NeedsSetup("Clipboard history uses XWayland on GNOME; it isn't running in this session".into())
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

fn umlaut(c: &Capabilities) -> Status {
    match helper(c) {
        Status::Supported if !c.ibus && c.desktop != Desktop::Gnome => {
            Status::NeedsSetup("Umlaut types accents with Ctrl+Shift+U, which needs IBus running".into())
        }
        s => s,
    }
}

fn window_backend(c: &Capabilities) -> Status {
    match c.desktop {
        Desktop::Gnome if c.forgekit_shell => Status::Supported,
        Desktop::Gnome => Status::NeedsSetup(
            "Needs the Forgekit GNOME extension: install it from the Mosaic, Pin or Bundle page in Forgekit, then log out and back in once".into(),
        ),
        Desktop::Kde => Status::Supported,
        _ => Status::Unsupported("Works on GNOME and KDE Plasma so far".into()),
    }
}

fn roam(_: &Capabilities) -> Status {
    if crate::tools::roam::find().is_some() {
        Status::Supported
    } else {
        Status::NeedsSetup("Uses Deskflow: install it with one click on the Roam page".into())
    }
}

fn scenes(c: &Capabilities) -> Status {
    match c.desktop {
        Desktop::Gnome if c.forgekit_shell => Status::Supported,
        Desktop::Gnome => Status::NeedsSetup(
            "Needs the Forgekit GNOME extension: install it from the Scenes page, then log out and back in once".into(),
        ),
        _ => Status::Unsupported("Works on GNOME so far".into()),
    }
}

fn servicedeck(_: &Capabilities) -> Status {
    if std::path::Path::new("/run/systemd/system").exists() {
        Status::Supported
    } else {
        Status::Unsupported("Needs systemd".into())
    }
}

fn settings_explorer(_: &Capabilities) -> Status {
    if crate::paths::which("gsettings").is_some() {
        Status::Supported
    } else {
        Status::Unsupported("Needs GSettings (GNOME, Cinnamon, Budgie and similar desktops)".into())
    }
}

macro_rules! tool {
    ($id:literal, $name:literal, $tier:ident, $cat:ident, $icon:literal, $since:literal, $check:expr, $summary:literal) => {
        Tool {
            id: $id,
            name: $name,
            tier: Tier::$tier,
            category: Category::$cat,
            icon: $icon,
            since: $since,
            check: $check,
            summary: $summary,
        }
    };
}

pub static CATALOG: &[Tool] = &[
    tool!(
        "spark",
        "Spark",
        Universal,
        Everyday,
        "bolt",
        "0.1",
        needs_graphical,
        "Launcher and command bar: apps, calculator, web search"
    ),
    tool!(
        "clipstack",
        "Clipboard",
        Universal,
        Everyday,
        "clipboard-list",
        "0.2",
        clipstack,
        "Clipboard history for text and images"
    ),
    tool!(
        "shelf",
        "Bundle",
        Universal,
        Everyday,
        "stack-2",
        "0.2",
        needs_graphical,
        "Hold files while you move between apps"
    ),
    tool!(
        "eyedrop",
        "Eyedrop",
        Universal,
        Everyday,
        "color-picker",
        "0.1",
        eyedrop,
        "Pick any color on screen and copy it as HEX/RGB/HSL"
    ),
    tool!(
        "glyphgrab",
        "Glyphgrab",
        Universal,
        Everyday,
        "text-scan-2",
        "0.1",
        glyphgrab,
        "Copy the text in any area of the screen (OCR)"
    ),
    tool!("glance", "Glance", Universal, Everyday, "eye", "0.2", needs_graphical, "Quick preview of any file"),
    tool!(
        "relabel",
        "Relabel",
        Universal,
        Files,
        "forms",
        "0.1",
        always,
        "Bulk-rename files with find/replace, regex and numbering"
    ),
    tool!("shrinkray", "Shrinkray", Universal, Files, "photo-scan", "0.1", always, "Bulk-resize and convert images"),
    tool!(
        "lockpick",
        "Lockpick",
        Universal,
        Files,
        "lock-open",
        "0.1",
        lockpick,
        "Show which process is holding a file, and close it"
    ),
    tool!(
        "teku",
        "Teku",
        Universal,
        Files,
        "package",
        "0.1",
        teku,
        "Install .deb, .rpm, .AppImage and .flatpakref files"
    ),
    tool!(
        "nightowl",
        "Nightowl",
        Universal,
        System,
        "moon-stars",
        "0.1",
        nightowl,
        "Keep the PC awake, for a while or while a process runs"
    ),
    tool!(
        "hostbook",
        "Hostbook",
        Universal,
        System,
        "world-www",
        "0.1",
        hostbook,
        "Safe /etc/hosts editor with backups and per-entry toggles"
    ),
    tool!("envy", "Envy", Universal, System, "variable", "0.2", always, "Edit user and system environment variables"),
    tool!(
        "keysmith",
        "Keysmith",
        Helper,
        Input,
        "keyboard",
        "0.3",
        helper,
        "Remap keys and shortcuts, or switch keys off"
    ),
    tool!("umlaut", "Umlaut", Helper, Input, "language", "0.3", umlaut, "Hold a letter to pick an accented version"),
    tool!(
        "cheatsheet",
        "Cheatsheet",
        Universal,
        Input,
        "keyboard-show",
        "0.5",
        always,
        "See every keyboard shortcut your desktop has"
    ),
    tool!(
        "mosaic",
        "Mosaic",
        Desktop,
        Windows,
        "layout-grid",
        "0.4",
        window_backend,
        "Custom window zones and layouts"
    ),
    tool!("pin", "Pin", Desktop, Windows, "pinned", "0.4", window_backend, "Keep any window always on top"),
    tool!(
        "scenes",
        "Scenes",
        Desktop,
        Windows,
        "layout-board-split",
        "0.6",
        scenes,
        "Save how your apps are arranged and restore it in one click"
    ),
    tool!("roam", "Roam", Desktop, Windows, "devices", "0.6", roam, "Share one mouse and keyboard across PCs"),
    tool!(
        "sweep",
        "Sweep",
        LinuxExtra,
        Extras,
        "vacuum-cleaner",
        "0.6",
        always,
        "Clean package caches, old runtimes, logs and thumbnails"
    ),
    tool!(
        "bootpilot",
        "Startup Apps",
        LinuxExtra,
        Extras,
        "rocket",
        "0.6",
        always,
        "Choose which apps and services start when you log in"
    ),
    tool!(
        "servicedeck",
        "Services",
        LinuxExtra,
        Extras,
        "server-cog",
        "0.6",
        servicedeck,
        "Start, stop and inspect systemd services"
    ),
    tool!(
        "settings-explorer",
        "Settings Explorer",
        LinuxExtra,
        Extras,
        "adjustments-horizontal",
        "0.6",
        settings_explorer,
        "Search, change and reset every GNOME setting, hidden ones too"
    ),
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
        assert_eq!(find("Bundle").unwrap().id, "shelf");
        assert_eq!(find("startup-apps").unwrap().id, "bootpilot");
    }

    #[test]
    fn every_planned_tool_has_shipped() {
        assert!(CATALOG.iter().all(|t| !t.is_planned()));
        assert!(version_key("0.10") > version_key("0.9"));
    }
}
