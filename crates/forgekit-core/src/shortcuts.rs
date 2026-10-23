//! Global keyboard shortcuts for Forgekit tools, written into the desktop's
//! own custom-shortcut settings (the same ones its Settings app edits), so
//! they work without Forgekit running and show up in the desktop's UI.
//!
//! Supported: GNOME (and Ubuntu, Budgie, Pop) and Cinnamon. Elsewhere the app
//! shows the command to bind by hand.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, bail};

use crate::capability::Desktop;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Gnome,
    Cinnamon,
}

pub fn backend(desktop: Desktop) -> Option<Backend> {
    match desktop {
        Desktop::Gnome => Some(Backend::Gnome),
        Desktop::Cinnamon => Some(Backend::Cinnamon),
        _ => None,
    }
}

/// What a tool's shortcut does: a label for people, and the command it runs.
pub struct Action {
    pub label: String,
    pub command: String,
}

/// The shortcut action for a tool. Quick tools run directly; the rest open
/// their page in the app.
pub fn action(tool: &crate::Tool) -> Action {
    let (label, bin, args): (String, &str, Vec<&str>) = match tool.id {
        "spark" => ("Open Spark".into(), "forgekit-gtk", vec!["spark"]),
        "shelf" => ("Open DropShelf".into(), "forgekit-gtk", vec!["--shelf"]),
        "clipstack" => ("Open clipboard history".into(), "forgekit-gtk", vec!["clipstack"]),
        "eyedrop" => ("Pick a color and copy it".into(), "forgekit", vec!["eyedrop", "--notify"]),
        "glyphgrab" => ("Grab text from the screen".into(), "forgekit", vec!["glyphgrab", "--notify"]),
        "nightowl" => ("Turn keep-awake on or off".into(), "forgekit", vec!["nightowl", "--toggle"]),
        "dusk" => ("Switch light/dark".into(), "forgekit", vec!["dusk", "toggle"]),
        id => (format!("Open {}", tool.name), "forgekit-gtk", vec![id]),
    };
    let mut command = shell_quote(&bin_path(bin).to_string_lossy());
    for a in args {
        command.push(' ');
        command.push_str(a);
    }
    Action { label, command }
}

/// Prefer the binary next to the running program (a build folder or an
/// install prefix), then `$PATH`.
fn bin_path(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .filter(|p| p.exists())
        .or_else(|| crate::paths::which(name))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn shell_quote(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

// ------------------------------------------------------------------ accelerators

/// Canonical form for comparing accelerators: `<Primary><Shift>C` and
/// `<Shift><Control>c` both become `ctrl+shift+c`.
pub fn normalize(accel: &str) -> String {
    let mut mods = Vec::new();
    let mut rest = accel.trim();
    while let Some(after) = rest.strip_prefix('<') {
        let Some(end) = after.find('>') else { break };
        let m = match after[..end].to_ascii_lowercase().as_str() {
            "control" | "primary" | "ctrl" => "ctrl",
            "alt" | "mod1" => "alt",
            "shift" => "shift",
            "super" | "mod4" | "meta" | "hyper" => "super",
            other => {
                mods.push(other.to_string());
                rest = &after[end + 1..];
                continue;
            }
        };
        mods.push(m.to_string());
        rest = &after[end + 1..];
    }
    mods.sort();
    mods.dedup();
    let mut s = mods.join("+");
    if !s.is_empty() {
        s.push('+');
    }
    s.push_str(&rest.to_ascii_lowercase());
    s
}

/// Accept `super+shift+c` as well as GTK's `<Super><Shift>c`.
pub fn parse_accel(s: &str) -> anyhow::Result<String> {
    let s = s.trim();
    if s.starts_with('<') {
        return Ok(s.to_string());
    }
    let parts: Vec<&str> = s.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
    let Some((key, mods)) = parts.split_last() else { bail!("empty shortcut") };
    let mut out = String::new();
    for m in mods {
        out.push_str(match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "primary" => "<Control>",
            "alt" => "<Alt>",
            "shift" => "<Shift>",
            "super" | "win" | "meta" => "<Super>",
            other => bail!("'{other}' isn't a modifier (Ctrl, Alt, Shift, Super)"),
        });
    }
    if mods.is_empty() && !(key.len() > 1 && key.starts_with(['F', 'f']) && key[1..].parse::<u8>().is_ok()) {
        bail!("add Ctrl, Alt or Super so the shortcut doesn't block normal typing");
    }
    out.push_str(&if key.chars().count() == 1 { key.to_ascii_lowercase() } else { key.to_string() });
    Ok(out)
}

/// Pull every single-quoted string out of a GVariant text value like
/// `['<Super>a', '<Alt>F4']` or `'<Super>a'`.
fn quoted_strings(v: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c != '\'' {
            continue;
        }
        let mut s = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(n) = chars.next() {
                        s.push(n);
                    }
                }
                '\'' => break,
                c => s.push(c),
            }
        }
        out.push(s);
    }
    out
}

fn gvariant_string(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

// ------------------------------------------------------------------ gsettings

fn gsettings(args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("gsettings").args(args).output().context("running gsettings")?;
    if !out.status.success() {
        bail!("gsettings {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

struct Layout {
    list_schema: &'static str,
    list_key: &'static str,
    item_schema: &'static str,
    /// Item paths are `{prefix}{name}/`.
    prefix: &'static str,
    /// GNOME stores full paths in the list; Cinnamon stores names.
    list_holds_paths: bool,
    /// GNOME's binding is a string, Cinnamon's an array of strings.
    binding_is_array: bool,
    /// Built-in shortcut schemas to check for conflicts.
    builtin: &'static [&'static str],
}

const GNOME: Layout = Layout {
    list_schema: "org.gnome.settings-daemon.plugins.media-keys",
    list_key: "custom-keybindings",
    item_schema: "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding",
    prefix: "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/",
    list_holds_paths: true,
    binding_is_array: false,
    builtin: &[
        "org.gnome.desktop.wm.keybindings",
        "org.gnome.shell.keybindings",
        "org.gnome.mutter.keybindings",
        "org.gnome.mutter.wayland.keybindings",
        "org.gnome.settings-daemon.plugins.media-keys",
    ],
};

const CINNAMON: Layout = Layout {
    list_schema: "org.cinnamon.desktop.keybindings",
    list_key: "custom-list",
    item_schema: "org.cinnamon.desktop.keybindings.custom-keybinding",
    prefix: "/org/cinnamon/desktop/keybindings/custom-keybindings/",
    list_holds_paths: false,
    binding_is_array: true,
    builtin: &["org.cinnamon.desktop.keybindings.wm", "org.cinnamon.desktop.keybindings.media-keys"],
};

fn layout(b: Backend) -> &'static Layout {
    match b {
        Backend::Gnome => &GNOME,
        Backend::Cinnamon => &CINNAMON,
    }
}

fn entry_name(tool: &str) -> String {
    format!("forgekit-{tool}")
}

impl Layout {
    fn item(&self, name: &str) -> String {
        format!("{}:{}{name}/", self.item_schema, self.prefix)
    }

    fn list(&self) -> anyhow::Result<Vec<String>> {
        Ok(quoted_strings(&gsettings(&["get", self.list_schema, self.list_key])?))
    }

    fn list_entry(&self, name: &str) -> String {
        if self.list_holds_paths { format!("{}{name}/", self.prefix) } else { name.to_string() }
    }

    fn entry_names(&self) -> anyhow::Result<Vec<String>> {
        Ok(self
            .list()?
            .into_iter()
            .map(|e| {
                if self.list_holds_paths {
                    e.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string()
                } else {
                    e
                }
            })
            .collect())
    }

    fn binding(&self, name: &str) -> Option<String> {
        let v = gsettings(&["get", &self.item(name), "binding"]).ok()?;
        quoted_strings(&v).into_iter().find(|s| !s.is_empty())
    }

    fn label(&self, name: &str) -> String {
        gsettings(&["get", &self.item(name), "name"])
            .ok()
            .and_then(|v| quoted_strings(&v).pop())
            .unwrap_or_else(|| name.to_string())
    }
}

/// The accelerator currently bound to a tool, if any.
pub fn get(b: Backend, tool: &str) -> Option<String> {
    let l = layout(b);
    let name = entry_name(tool);
    l.entry_names().ok()?.contains(&name).then(|| l.binding(&name)).flatten()
}

/// Bind `accel` (GTK syntax, e.g. `<Super><Shift>c`) to run `command`.
pub fn set(b: Backend, tool: &str, label: &str, command: &str, accel: &str) -> anyhow::Result<()> {
    let l = layout(b);
    let name = entry_name(tool);
    let item = l.item(&name);
    gsettings(&["set", &item, "name", &gvariant_string(&format!("Forgekit: {label}"))])?;
    gsettings(&["set", &item, "command", &gvariant_string(command)])?;
    let binding = if l.binding_is_array { format!("[{}]", gvariant_string(accel)) } else { gvariant_string(accel) };
    gsettings(&["set", &item, "binding", &binding])?;
    let mut list = l.list()?;
    let entry = l.list_entry(&name);
    if !list.contains(&entry) {
        list.push(entry);
        let v = format!("[{}]", list.iter().map(|s| gvariant_string(s)).collect::<Vec<_>>().join(", "));
        gsettings(&["set", l.list_schema, l.list_key, &v])?;
    }
    Ok(())
}

pub fn remove(b: Backend, tool: &str) -> anyhow::Result<()> {
    let l = layout(b);
    let name = entry_name(tool);
    let entry = l.list_entry(&name);
    let list: Vec<String> = l.list()?.into_iter().filter(|e| *e != entry).collect();
    let v = if list.is_empty() {
        "@as []".to_string()
    } else {
        format!("[{}]", list.iter().map(|s| gvariant_string(s)).collect::<Vec<_>>().join(", "))
    };
    gsettings(&["set", l.list_schema, l.list_key, &v])?;
    for key in ["name", "command", "binding"] {
        let _ = gsettings(&["reset", &l.item(&name), key]);
    }
    Ok(())
}

/// What else already uses `accel` (desktop shortcuts and other custom ones),
/// ignoring this tool's own binding.
pub fn conflicts(b: Backend, tool: &str, accel: &str) -> Vec<String> {
    let l = layout(b);
    let want = normalize(accel);
    let mut out = Vec::new();
    for schema in l.builtin {
        let Ok(text) = gsettings(&["list-recursively", schema]) else { continue };
        for line in text.lines() {
            let mut parts = line.splitn(3, ' ');
            let (_, Some(key), Some(value)) = (parts.next(), parts.next(), parts.next()) else { continue };
            if quoted_strings(value).iter().any(|a| !a.is_empty() && normalize(a) == want) {
                out.push(humanize(key));
            }
        }
    }
    let ours = entry_name(tool);
    for name in l.entry_names().unwrap_or_default() {
        if name != ours && l.binding(&name).is_some_and(|a| normalize(&a) == want) {
            out.push(l.label(&name));
        }
    }
    out
}

/// `switch-to-workspace-1` → `Switch to workspace 1`.
fn humanize(key: &str) -> String {
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
    fn accelerators() {
        assert_eq!(normalize("<Primary><Shift>C"), "ctrl+shift+c");
        assert_eq!(normalize("<Shift><Control>c"), "ctrl+shift+c");
        assert_eq!(normalize("<Super>space"), "super+space");
        assert_eq!(normalize("<Mod4><Alt>F4"), "alt+super+f4");
        assert_eq!(normalize("XF86Calculator"), "xf86calculator");
        assert_eq!(parse_accel("super+shift+C").unwrap(), "<Super><Shift>c");
        assert_eq!(parse_accel("F9").unwrap(), "F9");
        assert!(parse_accel("c").is_err());
        assert!(parse_accel("hyper+c").is_err());
    }

    #[test]
    fn gvariant_parsing() {
        assert_eq!(quoted_strings("['<Super>a', '<Alt>F4']"), ["<Super>a", "<Alt>F4"]);
        assert_eq!(quoted_strings("@as []"), Vec::<String>::new());
        assert_eq!(quoted_strings(r"'it\'s'"), ["it's"]);
        assert_eq!(gvariant_string("it's"), r"'it\'s'");
        assert_eq!(humanize("switch-to-workspace-1"), "Switch to workspace 1");
        assert_eq!(shell_quote("/home/a b/forgekit"), "'/home/a b/forgekit'");
        assert_eq!(shell_quote("/usr/bin/forgekit"), "/usr/bin/forgekit");
    }
}
