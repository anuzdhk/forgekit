//! Cheatsheet: the keyboard shortcuts this desktop actually has, read from
//! its settings (GNOME and Cinnamon gsettings, KDE's kglobalshortcutsrc).

use std::process::Command;

use serde::Serialize;

use crate::capability::Desktop;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Shortcut {
    pub group: String,
    pub action: String,
    /// Display form, keys joined by '+': "Super+Shift+Page Down".
    pub keys: String,
}

pub fn collect(desktop: Desktop) -> Vec<Shortcut> {
    let mut out = match desktop {
        Desktop::Gnome => gsettings(&[
            ("org.gnome.shell.keybindings", "GNOME Shell"),
            ("org.gnome.desktop.wm.keybindings", "Windows & workspaces"),
            ("org.gnome.mutter.keybindings", "Windows & workspaces"),
            ("org.gnome.mutter.wayland.keybindings", "Windows & workspaces"),
            ("org.gnome.settings-daemon.plugins.media-keys", "System & apps"),
        ]),
        Desktop::Cinnamon => gsettings(&[
            ("org.cinnamon.desktop.keybindings.wm", "Windows & workspaces"),
            ("org.cinnamon.desktop.keybindings.media-keys", "System & apps"),
        ]),
        Desktop::Kde => kde(),
        _ => Vec::new(),
    };
    if let Some(b) = crate::shortcuts::backend(desktop) {
        out.extend(custom(b));
    }
    out.sort_by(|a, b| a.group.cmp(&b.group).then(a.action.cmp(&b.action)));
    out.dedup();
    out
}

fn gsettings(schemas: &[(&str, &str)]) -> Vec<Shortcut> {
    let mut out = Vec::new();
    for (schema, group) in schemas {
        let Ok(o) = Command::new("gsettings").args(["list-recursively", schema]).output() else { continue };
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            let mut parts = line.splitn(3, ' ');
            let (_, Some(key), Some(value)) = (parts.next(), parts.next(), parts.next()) else { continue };
            // Skip the "-static" duplicates of media keys and hardware-only XF86 keys.
            if key.ends_with("-static") {
                continue;
            }
            for accel in quoted(value).into_iter().filter(|a| !a.is_empty() && !a.contains("XF86")) {
                out.push(Shortcut { group: group.to_string(), action: describe_key(key), keys: display_accel(&accel) });
            }
        }
    }
    out
}

fn custom(b: crate::shortcuts::Backend) -> Vec<Shortcut> {
    crate::shortcuts::list_custom(b)
        .into_iter()
        .map(|(name, accel)| Shortcut { group: "Your shortcuts".into(), action: name, keys: display_accel(&accel) })
        .collect()
}

/// KDE: `~/.config/kglobalshortcutsrc`, lines `Action id=active,default,Friendly name`.
fn kde() -> Vec<Shortcut> {
    let path = dirs::config_dir().unwrap_or_default().join("kglobalshortcutsrc");
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    parse_kde(&text)
}

pub fn parse_kde(text: &str) -> Vec<Shortcut> {
    let mut out = Vec::new();
    let mut group = String::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(g) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            group = match g {
                "kwin" => "Windows (KWin)".into(),
                "plasmashell" => "Plasma".into(),
                "ksmserver" => "Session".into(),
                "kmix" => "Audio".into(),
                "$Version" => String::new(),
                other => other.trim_end_matches(".desktop").rsplit('.').next().unwrap_or(other).to_string(),
            };
            continue;
        }
        if group.is_empty() || line.starts_with('_') {
            continue;
        }
        let Some((id, value)) = line.split_once('=') else { continue };
        let fields: Vec<&str> = value.splitn(3, ',').collect();
        let active = fields.first().copied().unwrap_or("");
        let name = fields.get(2).copied().filter(|n| !n.is_empty()).unwrap_or(id);
        for keys in active.split('\t').filter(|k| !k.is_empty() && *k != "none") {
            out.push(Shortcut { group: group.clone(), action: name.to_string(), keys: keys.to_string() });
        }
    }
    out
}

fn quoted(v: &str) -> Vec<String> {
    v.split('\'').skip(1).step_by(2).map(str::to_string).collect()
}

/// `<Super><Shift>Page_Down` → `Super+Shift+Page Down`.
pub fn display_accel(accel: &str) -> String {
    let mut parts = Vec::new();
    let mut rest = accel.trim();
    while let Some(after) = rest.strip_prefix('<') {
        let Some(end) = after.find('>') else { break };
        parts.push(match after[..end].to_ascii_lowercase().as_str() {
            "control" | "primary" | "ctrl" => "Ctrl".to_string(),
            "super" | "mod4" => "Super".to_string(),
            "alt" | "mod1" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            other => other.to_string(),
        });
        rest = &after[end + 1..];
    }
    let key = match rest {
        "" => return parts.join("+"),
        "Above_Tab" | "grave" => "`".to_string(),
        "equal" => "=".to_string(),
        "minus" => "-".to_string(),
        "period" => ".".to_string(),
        "comma" => ",".to_string(),
        "slash" => "/".to_string(),
        "backslash" => "\\".to_string(),
        "semicolon" => ";".to_string(),
        "apostrophe" => "'".to_string(),
        "bracketleft" => "[".to_string(),
        "bracketright" => "]".to_string(),
        "space" => "Space".to_string(),
        "Return" => "Enter".to_string(),
        "Escape" => "Esc".to_string(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => k.replace('_', " "),
    };
    parts.push(key);
    parts.join("+")
}

/// `switch-to-workspace-1` → `Switch to workspace 1`, with friendlier names for common ones.
fn describe_key(key: &str) -> String {
    let known = [
        ("toggle-overview", "Show the overview"),
        ("toggle-application-view", "Show all apps"),
        ("toggle-message-tray", "Show notifications"),
        ("focus-active-notification", "Focus the latest notification"),
        ("switch-applications", "Switch apps"),
        ("switch-windows", "Switch windows"),
        ("switch-group", "Switch windows of the same app"),
        ("close", "Close window"),
        ("minimize", "Minimize window"),
        ("toggle-maximized", "Maximize or restore window"),
        ("toggle-fullscreen", "Toggle fullscreen"),
        ("show-desktop", "Show the desktop"),
        ("panel-run-dialog", "Run a command"),
        ("screenshot-ui", "Take a screenshot"),
        ("show-screenshot-ui", "Take a screenshot"),
        ("show-screen-recording-ui", "Record the screen"),
        ("screensaver", "Lock the screen"),
        ("logout", "Log out"),
        ("terminal", "Open a terminal"),
        ("home", "Open your Home folder"),
        ("www", "Open the web browser"),
        ("control-center", "Open Settings"),
        ("switch-input-source", "Next keyboard layout"),
        ("switch-input-source-backward", "Previous keyboard layout"),
        ("toggle-tiled-left", "Tile window left"),
        ("toggle-tiled-right", "Tile window right"),
    ];
    if let Some((_, nice)) = known.iter().find(|(k, _)| *k == key) {
        return nice.to_string();
    }
    let s = key.replace('-', " ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerators_read_nicely() {
        assert_eq!(display_accel("<Super><Shift>Page_Down"), "Super+Shift+Page Down");
        assert_eq!(display_accel("<Primary><Alt>t"), "Ctrl+Alt+T");
        assert_eq!(display_accel("<Super>Above_Tab"), "Super+`");
        assert_eq!(display_accel("<Alt><Super>equal"), "Alt+Super+=");
        assert_eq!(display_accel("<Super>slash"), "Super+/");
        assert_eq!(describe_key("switch-to-workspace-1"), "Switch to workspace 1");
        assert_eq!(describe_key("close"), "Close window");
    }

    #[test]
    fn kde_config() {
        let text = "[$Version]\nupdate_info=x\n\n[kwin]\nWindow Close=Alt+F4,Alt+F4,Close Window\nShow Desktop=none,Meta+D,Peek at Desktop\nExpose=Ctrl+F9\tMeta+W,Ctrl+F9,Toggle Present Windows\n_k_friendly_name=KWin\n\n[org.kde.spectacle.desktop]\nRectangularRegionScreenShot=Meta+Shift+Print,Meta+Shift+Print,Capture Rectangular Region\n";
        let s = parse_kde(text);
        let pairs: Vec<(&str, &str, &str)> =
            s.iter().map(|x| (x.group.as_str(), x.action.as_str(), x.keys.as_str())).collect();
        assert_eq!(
            pairs,
            [
                ("Windows (KWin)", "Close Window", "Alt+F4"),
                ("Windows (KWin)", "Toggle Present Windows", "Ctrl+F9"),
                ("Windows (KWin)", "Toggle Present Windows", "Meta+W"),
                ("spectacle", "Capture Rectangular Region", "Meta+Shift+Print"),
            ]
        );
    }
}
