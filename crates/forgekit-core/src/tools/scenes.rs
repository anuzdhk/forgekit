//! Scenes: save how your apps are laid out (which app, which monitor, where)
//! and bring the whole arrangement back in one click. GNOME through the
//! Forgekit extension, which reports windows and places new ones.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Desktop file id, e.g. `org.gnome.Nautilus.desktop`.
    pub app_id: String,
    pub name: String,
    pub monitor: i32,
    /// Fractions of the monitor's work area.
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(default)]
    pub maximized: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub name: String,
    pub items: Vec<Item>,
}

fn file() -> PathBuf {
    crate::paths::config_dir().join("scenes.json")
}

pub fn load() -> Vec<Scene> {
    std::fs::read_to_string(file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(scenes: &[Scene]) -> anyhow::Result<()> {
    std::fs::create_dir_all(crate::paths::config_dir())?;
    std::fs::write(file(), serde_json::to_string_pretty(scenes)?)?;
    Ok(())
}

fn shell(method: &str, body: &(impl serde::Serialize + zbus::zvariant::DynamicType)) -> anyhow::Result<String> {
    let conn = zbus::blocking::Connection::session()?;
    let m = conn
        .call_method(Some(crate::desktop::SHELL_BUS), "/org/forgekit/Shell", Some("org.forgekit.Shell1"), method, body)
        .context(
            "the Forgekit GNOME extension isn't running, or is older than this Forgekit (update it on the Scenes page)",
        )?;
    Ok(m.body().deserialize()?)
}

/// The windows open right now, as scene items (one per window).
pub fn capture(name: &str) -> anyhow::Result<Scene> {
    let items: Vec<Item> = serde_json::from_str(&shell("ListWindows", &())?)?;
    let items: Vec<Item> = items
        .into_iter()
        .filter(|i| !i.app_id.is_empty() && i.app_id != "io.github.anuzdhk.Forgekit.desktop")
        .collect();
    if items.is_empty() {
        bail!("no app windows to save");
    }
    Ok(Scene { name: name.to_string(), items })
}

/// Where an app's launcher lives, so it can be started.
fn launcher(app_id: &str) -> Option<PathBuf> {
    let mut dirs = vec![dirs::data_dir().unwrap_or_default()];
    let sys = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(sys.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    dirs.into_iter().map(|d| d.join("applications").join(app_id)).find(|p| p.exists())
}

/// Arrange already-open windows and start the apps that aren't running;
/// the extension places their windows as they appear.
/// Returns the names of apps that couldn't be started.
pub fn restore(scene: &Scene) -> anyhow::Result<Vec<String>> {
    let mut placed: Vec<String> = serde_json::from_str(&shell("Place", &serde_json::to_string(&scene.items)?)?)?;
    let mut missing = Vec::new();
    for item in &scene.items {
        // Each placed window counts once (a scene can have two windows of one app).
        if let Some(i) = placed.iter().position(|p| *p == item.app_id) {
            placed.remove(i);
            continue;
        }
        match launcher(&item.app_id) {
            Some(path) => {
                let _ = Command::new("gio").arg("launch").arg(path).spawn();
            }
            None => missing.push(item.name.clone()),
        }
    }
    Ok(missing)
}
