//! Beacon: find the mouse pointer and show clicks. GNOME uses Forgekit's
//! Shell extension; KDE Plasma has built-in effects that we switch on.

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::desktop::Backend;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Draw a ripple where you click.
    pub clicks: bool,
    /// Press Ctrl twice quickly to highlight the pointer (GNOME).
    pub double_ctrl: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { clicks: false, double_ctrl: true }
    }
}

impl Settings {
    pub fn load() -> anyhow::Result<Self> {
        crate::settings::load("beacon")
    }

    pub fn save(&self) -> anyhow::Result<()> {
        crate::settings::save("beacon", self)
    }
}

/// Highlight the pointer once (a shrinking ring around it).
pub fn find(b: Backend) -> anyhow::Result<()> {
    match b {
        Backend::GnomeShell => {
            let conn = zbus::blocking::Connection::session()?;
            conn.call_method(
                Some(crate::desktop::SHELL_BUS),
                "/org/forgekit/Shell",
                Some("org.forgekit.Shell1"),
                "FindCursor",
                &(),
            )
            .context("the Forgekit GNOME extension isn't running")?;
            Ok(())
        }
        Backend::KWin => bail!("on KDE Plasma, hold Ctrl+Meta to highlight the pointer (KWin's Track Mouse effect)"),
    }
}

/// Apply settings. On GNOME the extension reads them from its config file;
/// on KDE this switches KWin's Mouse Click and Track Mouse effects.
pub fn apply(b: Backend, s: &Settings) -> anyhow::Result<()> {
    match b {
        Backend::GnomeShell => {
            crate::desktop::write_config(None);
            Ok(())
        }
        Backend::KWin => {
            let conn = zbus::blocking::Connection::session()?;
            let effect = |method: &str, name: &str| -> anyhow::Result<()> {
                conn.call_method(Some("org.kde.KWin"), "/Effects", Some("org.kde.kwin.Effects"), method, &name)
                    .with_context(|| format!("asking KWin to {method} {name}"))?;
                Ok(())
            };
            effect(if s.clicks { "loadEffect" } else { "unloadEffect" }, "mouseclick")?;
            effect("loadEffect", "trackmouse")
        }
    }
}
