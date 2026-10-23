//! Desktop integration that plain Wayland apps can't do on their own:
//! the companion GNOME Shell extension (DropShelf pop-up, Mosaic zones, Pin)
//! and one-shot KWin scripts on KDE Plasma.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, bail};
use serde_json::json;

use crate::capability::{Capabilities, Desktop};
use crate::tools::mosaic::{self, Zone};
use crate::tools::shelf;

// ------------------------------------------------------------------ GNOME extension

pub const UUID: &str = "forgekit@anuzdhk.github.io";
const METADATA: &str = include_str!("../../../data/gnome-extension/metadata.json");
const EXTENSION: &str = include_str!("../../../data/gnome-extension/extension.js");
/// The D-Bus name the extension owns while it runs.
pub const SHELL_BUS: &str = "org.forgekit.Shell";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    NotInstalled,
    /// Files are in place but GNOME Shell hasn't loaded them (Wayland needs a new login).
    NeedsRelogin,
    Disabled,
    Active,
    /// Installed but GNOME reports an error (e.g. an unsupported Shell version).
    Error,
    /// Running, but an older copy than this Forgekit bundles: reinstall and log in again.
    Outdated,
}

fn dir() -> PathBuf {
    dirs::data_dir().unwrap_or_default().join("gnome-shell/extensions").join(UUID)
}

pub fn state() -> State {
    let installed = std::fs::read_to_string(dir().join("extension.js"));
    let Ok(installed) = installed else { return State::NotInstalled };
    let Ok(out) = Command::new("gnome-extensions").args(["info", UUID]).output() else { return State::NeedsRelogin };
    let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
    if !out.status.success() || text.is_empty() {
        return State::NeedsRelogin;
    }
    let field = |name: &str| {
        text.lines().find_map(|l| l.trim().strip_prefix(name).map(|v| v.trim_start_matches(':').trim().to_string()))
    };
    match field("state").as_deref() {
        Some("active" | "enabled") if installed != EXTENSION || !shell_running() => State::Outdated,
        Some("active" | "enabled") => State::Active,
        Some("error" | "out of date") => State::Error,
        Some("initialized") if field("enabled").as_deref() == Some("yes") => State::NeedsRelogin,
        _ => State::Disabled,
    }
}

/// Whether the running extension serves its D-Bus interface (needed by Mosaic and Pin).
pub fn shell_running() -> bool {
    let Ok(conn) = zbus::blocking::Connection::session() else { return false };
    let Ok(proxy) = zbus::blocking::fdo::DBusProxy::new(&conn) else { return false };
    proxy.name_has_owner(SHELL_BUS.try_into().unwrap()).unwrap_or(false)
}

/// Copy the bundled extension into place and switch it on. Returns whether
/// it's already running (otherwise a new login loads it).
pub fn install(app: Option<&str>) -> anyhow::Result<bool> {
    let d = dir();
    std::fs::create_dir_all(&d)?;
    std::fs::write(d.join("metadata.json"), METADATA)?;
    std::fs::write(d.join("extension.js"), EXTENSION)?;
    write_config(app);
    let _ = Command::new("gnome-extensions").args(["enable", UUID]).output();
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

pub fn remove() -> anyhow::Result<()> {
    let _ = Command::new("gnome-extensions").args(["disable", UUID]).output();
    std::fs::remove_dir_all(dir())?;
    Ok(())
}

fn config_file() -> PathBuf {
    crate::paths::config_dir().join("shell.json")
}

/// `~/.config/forgekit/shell.json`, read by the extension on every use.
/// `app` is the forgekit-gtk path; `None` keeps the one already recorded.
pub fn write_config(app: Option<&str>) {
    let old: serde_json::Value =
        std::fs::read_to_string(config_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let app = app.map(str::to_string).or_else(|| old["app"].as_str().map(str::to_string)).unwrap_or_default();
    let s = shelf::Settings::load().unwrap_or_default();
    let m = mosaic::Settings::load().unwrap_or_default();
    let p = mosaic::PinSettings::load().unwrap_or_default();
    let zones: Vec<[f64; 4]> = m.zones().iter().map(|z| [z.x, z.y, z.w, z.h]).collect();
    let v = json!({
        "app": app,
        "auto_show": s.auto_show,
        "files_only": s.files_only,
        "corner": s.corner,
        "delay_ms": 350,
        "mosaic": { "zones": zones, "gap": m.gap, "shift_drag": m.shift_drag },
        "pin": { "border": p.border, "color": p.color },
    });
    let _ = std::fs::create_dir_all(crate::paths::config_dir());
    let _ = std::fs::write(config_file(), format!("{v:#}\n"));
}

// ------------------------------------------------------------------ window actions

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Our GNOME Shell extension, over D-Bus.
    GnomeShell,
    /// One-shot KWin scripts, over D-Bus.
    KWin,
}

pub fn backend(caps: &Capabilities) -> Option<Backend> {
    match caps.desktop {
        Desktop::Gnome => Some(Backend::GnomeShell),
        Desktop::Kde => Some(Backend::KWin),
        _ => None,
    }
}

fn shell_call(
    method: &str,
    body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
) -> anyhow::Result<zbus::message::Message> {
    let conn = zbus::blocking::Connection::session()?;
    conn.call_method(Some(SHELL_BUS), "/org/forgekit/Shell", Some("org.forgekit.Shell1"), method, body)
        .context("the Forgekit GNOME extension isn't running (install it from Forgekit, then log out and in once)")
}

/// Toggle always-on-top for the focused window. Returns the new state when known.
pub fn pin_toggle(b: Backend) -> anyhow::Result<Option<bool>> {
    match b {
        Backend::GnomeShell => Ok(Some(shell_call("PinToggle", &())?.body().deserialize()?)),
        Backend::KWin => {
            kwin_run("const w = workspace.activeWindow; if (w && w.normalWindow) { w.keepAbove = !w.keepAbove; }")?;
            Ok(None)
        }
    }
}

/// Move the focused window into zone `index` (0-based) of the current layout.
pub fn snap(b: Backend, index: usize) -> anyhow::Result<()> {
    let zones = mosaic::Settings::load().unwrap_or_default().zones();
    if index >= zones.len() {
        bail!("this layout has {} zones", zones.len());
    }
    match b {
        Backend::GnomeShell => {
            let ok: bool = shell_call("Snap", &(index as i32))?.body().deserialize()?;
            if !ok {
                bail!("no window to move");
            }
            Ok(())
        }
        Backend::KWin => kwin_run(&kwin_snap_script(zones[index], mosaic::Settings::load().unwrap_or_default().gap)),
    }
}

/// KWin script that mirrors [`mosaic::zone_rect`].
fn kwin_snap_script(z: Zone, gap: u32) -> String {
    format!(
        "const w = workspace.activeWindow;
if (w && w.normalWindow && w.moveable && w.resizeable) {{
  const a = workspace.clientArea(KWin.MaximizeArea, w);
  const g = {gap};
  const edge = f => (f <= 0.0001 || f >= 0.9999) ? g : g / 2;
  const z = {{x: {x}, y: {y}, w: {zw}, h: {zh}}};
  const left = a.x + z.x * a.width + edge(z.x);
  const top = a.y + z.y * a.height + edge(z.y);
  const right = a.x + (z.x + z.w) * a.width - edge(z.x + z.w);
  const bottom = a.y + (z.y + z.h) * a.height - edge(z.y + z.h);
  if (w.setMaximize) w.setMaximize(false, false);
  w.frameGeometry = {{x: Math.round(left), y: Math.round(top), width: Math.round(right - left), height: Math.round(bottom - top)}};
}}",
        x = z.x,
        y = z.y,
        zw = z.w,
        zh = z.h
    )
}

/// Load, run and unload a one-shot KWin script (Plasma 6 scripting over D-Bus).
fn kwin_run(source: &str) -> anyhow::Result<()> {
    const NAME: &str = "forgekit-oneshot";
    let path = std::env::temp_dir().join(format!("forgekit-kwin-{}.js", std::process::id()));
    std::fs::write(&path, source)?;
    let conn = zbus::blocking::Connection::session()?;
    let scripting =
        |method: &str, body: &dyn erased::Body| -> anyhow::Result<zbus::message::Message> { body.call(&conn, method) };
    let _ = scripting("unloadScript", &(NAME,));
    let id: i32 = scripting("loadScript", &(path.to_string_lossy().to_string(), NAME))
        .context("talking to KWin")?
        .body()
        .deserialize()?;
    let run = conn.call_method(
        Some("org.kde.KWin"),
        format!("/Scripting/Script{id}").as_str(),
        Some("org.kde.kwin.Script"),
        "run",
        &(),
    );
    let _ = scripting("unloadScript", &(NAME,));
    let _ = std::fs::remove_file(&path);
    run.context("running the KWin script")?;
    Ok(())
}

/// Small helper so `kwin_run` can call the Scripting object with different argument tuples.
mod erased {
    pub trait Body {
        fn call(&self, conn: &zbus::blocking::Connection, method: &str) -> anyhow::Result<zbus::message::Message>;
    }

    impl<T: serde::Serialize + zbus::zvariant::DynamicType> Body for T {
        fn call(&self, conn: &zbus::blocking::Connection, method: &str) -> anyhow::Result<zbus::message::Message> {
            Ok(conn.call_method(Some("org.kde.KWin"), "/Scripting", Some("org.kde.kwin.Scripting"), method, self)?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_extension() {
        assert!(METADATA.contains(UUID));
        assert!(EXTENSION.contains("export default class"));
        assert!(EXTENSION.contains(SHELL_BUS));
    }

    #[test]
    fn kwin_script_mirrors_rust_math() {
        let s = kwin_snap_script(mosaic::PRESETS[0].2[1], 8);
        assert!(s.contains("const g = 8;") && s.contains("x: 0.5, y: 0, w: 0.5, h: 1"));
        assert!(s.contains("w.frameGeometry ="));
    }
}
