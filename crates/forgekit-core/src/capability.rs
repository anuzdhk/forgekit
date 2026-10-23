//! Startup capability detection: session type, desktop, portals, Wayland
//! protocols, external binaries and the privileged helper.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;

use crate::paths::which;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "reason", rename_all = "kebab-case")]
pub enum Status {
    Supported,
    /// Works after the user does something (install a package, the helper…).
    NeedsSetup(String),
    Unsupported(String),
}

impl Status {
    pub fn is_supported(&self) -> bool {
        matches!(self, Status::Supported)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Status::Supported => "supported",
            Status::NeedsSetup(_) => "needs setup",
            Status::Unsupported(_) => "unsupported",
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Status::Supported => None,
            Status::NeedsSetup(r) | Status::Unsupported(r) => Some(r),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Session {
    Wayland,
    X11,
    Tty,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Desktop {
    Gnome,
    Kde,
    Hyprland,
    Sway,
    Wlroots,
    Xfce,
    Cinnamon,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub session: Session,
    pub desktop: Desktop,
    pub desktop_raw: String,
    /// Portal interface name (without the `org.freedesktop.portal.` prefix) → version.
    pub portals: BTreeMap<String, u32>,
    /// Wayland globals advertised by the compositor (empty on X11).
    pub wayland_globals: BTreeSet<String>,
    /// External programs Forgekit can use, by name.
    pub binaries: BTreeSet<String>,
    pub helper_installed: bool,
    /// IBus is running (Umlaut types accents with its Ctrl+Shift+U entry).
    pub ibus: bool,
    /// Forgekit's GNOME Shell extension is running (window tools on GNOME).
    pub forgekit_shell: bool,
}

const BINARIES: &[&str] = &["teku", "tesseract", "pkexec", "wl-copy", "xclip", "xsel"];
const HELPER_PATHS: &[&str] = &["/usr/libexec/forgekit-helper", "/usr/lib/forgekit/forgekit-helper"];

impl Capabilities {
    /// Probe the running system. Never fails: anything that can't be probed is
    /// simply reported as missing.
    pub fn detect() -> Self {
        let desktop_raw = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let session = match std::env::var("XDG_SESSION_TYPE").as_deref() {
            Ok("wayland") => Session::Wayland,
            Ok("x11") => Session::X11,
            Ok("tty") => Session::Tty,
            _ if std::env::var_os("WAYLAND_DISPLAY").is_some() => Session::Wayland,
            _ if std::env::var_os("DISPLAY").is_some() => Session::X11,
            _ => Session::Unknown,
        };
        let wayland_globals =
            if session == Session::Wayland { wayland_globals().unwrap_or_default() } else { BTreeSet::new() };
        Capabilities {
            session,
            desktop: parse_desktop(&desktop_raw, &wayland_globals),
            desktop_raw,
            portals: portals().unwrap_or_default(),
            wayland_globals,
            binaries: BINARIES.iter().filter(|b| which(b).is_some()).map(|b| b.to_string()).collect(),
            helper_installed: HELPER_PATHS.iter().any(|p| Path::new(p).exists()),
            ibus: process_running("ibus-daemon"),
            forgekit_shell: crate::desktop::shell_running(),
        }
    }

    pub fn portal(&self, name: &str) -> Option<u32> {
        self.portals.get(name).copied()
    }

    pub fn has_bin(&self, name: &str) -> bool {
        self.binaries.contains(name)
    }

    pub fn has_global(&self, name: &str) -> bool {
        self.wayland_globals.contains(name)
    }

    /// Whether a background clipboard listener is possible without extras.
    pub fn has_data_control(&self) -> bool {
        self.session == Session::X11
            || self.has_global("ext_data_control_manager_v1")
            || self.has_global("zwlr_data_control_manager_v1")
    }

    pub fn has_layer_shell(&self) -> bool {
        self.has_global("zwlr_layer_shell_v1")
    }

    /// Text for the "copy to clipboard" helper command, if any.
    pub fn clipboard_cmd(&self) -> Option<Vec<&'static str>> {
        match self.session {
            Session::Wayland if self.has_bin("wl-copy") => Some(vec!["wl-copy"]),
            _ if self.has_bin("xclip") => Some(vec!["xclip", "-selection", "clipboard"]),
            _ if self.has_bin("xsel") => Some(vec!["xsel", "--clipboard", "--input"]),
            _ => None,
        }
    }
}

fn process_running(name: &str) -> bool {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| c.trim() == name))
}

pub fn parse_desktop(raw: &str, globals: &BTreeSet<String>) -> Desktop {
    let parts: Vec<String> = raw.split(':').map(|s| s.to_ascii_lowercase()).collect();
    let has = |n: &str| parts.iter().any(|p| p == n);
    if has("gnome") || has("unity") {
        Desktop::Gnome
    } else if has("kde") {
        Desktop::Kde
    } else if has("hyprland") {
        Desktop::Hyprland
    } else if has("sway") {
        Desktop::Sway
    } else if has("xfce") {
        Desktop::Xfce
    } else if has("x-cinnamon") || has("cinnamon") {
        Desktop::Cinnamon
    } else if globals.contains("zwlr_layer_shell_v1") {
        Desktop::Wlroots
    } else {
        Desktop::Other
    }
}

/// List portal interfaces and their versions by introspecting the portal
/// frontend on the session bus.
fn portals() -> anyhow::Result<BTreeMap<String, u32>> {
    let conn = zbus::blocking::Connection::session()?;
    let xml: String = conn
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.DBus.Introspectable"),
            "Introspect",
            &(),
        )?
        .body()
        .deserialize()?;

    let mut out = BTreeMap::new();
    for name in interface_names(&xml) {
        let Some(short) = name.strip_prefix("org.freedesktop.portal.") else {
            continue;
        };
        let version = conn
            .call_method(
                Some("org.freedesktop.portal.Desktop"),
                "/org/freedesktop/portal/desktop",
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(name.as_str(), "version"),
            )
            .ok()
            .and_then(|m| m.body().deserialize::<zbus::zvariant::OwnedValue>().ok())
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0);
        out.insert(short.to_string(), version);
    }
    Ok(out)
}

/// Pull `<interface name="…">` values out of introspection XML.
pub(crate) fn interface_names(xml: &str) -> Vec<String> {
    xml.split("<interface name=\"").skip(1).filter_map(|rest| rest.split('"').next()).map(str::to_string).collect()
}

/// Connect to the compositor once and record every global it advertises.
fn wayland_globals() -> anyhow::Result<BTreeSet<String>> {
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::wl_registry;
    use wayland_client::{Connection, Dispatch, QueueHandle};

    struct State;
    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    let conn = Connection::connect_to_env()?;
    let (globals, _queue) = registry_queue_init::<State>(&conn)?;
    Ok(globals.contents().clone_list().into_iter().map(|g| g.interface).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_parsing() {
        let none = BTreeSet::new();
        assert_eq!(parse_desktop("ubuntu:GNOME", &none), Desktop::Gnome);
        assert_eq!(parse_desktop("KDE", &none), Desktop::Kde);
        assert_eq!(parse_desktop("Hyprland", &none), Desktop::Hyprland);
        assert_eq!(parse_desktop("X-Cinnamon", &none), Desktop::Cinnamon);
        let wl: BTreeSet<_> = ["zwlr_layer_shell_v1".to_string()].into();
        assert_eq!(parse_desktop("labwc", &wl), Desktop::Wlroots);
    }

    #[test]
    fn introspection_names() {
        let xml = r#"<node><interface name="org.freedesktop.portal.Screenshot"><method/></interface>
            <interface name="org.freedesktop.DBus.Peer"/></node>"#;
        assert_eq!(interface_names(xml), ["org.freedesktop.portal.Screenshot", "org.freedesktop.DBus.Peer"]);
    }
}
