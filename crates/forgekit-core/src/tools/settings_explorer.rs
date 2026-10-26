//! Settings Explorer: every GSettings key (GNOME, Cinnamon, and apps that
//! use it), which ones you've changed, and editing/resetting them.

use std::collections::{HashMap, HashSet};
use std::process::Command;

use anyhow::{Context, bail};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Key {
    pub schema: String,
    pub key: String,
    /// GVariant text, e.g. `'prefer-dark'`, `true`, `['a', 'b']`.
    pub value: String,
    /// Differs from the default (stored in dconf).
    pub modified: bool,
}

fn gsettings(args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("gsettings").args(args).output().context("running gsettings")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// dconf sections → keys present (i.e. changed from their defaults).
fn changed() -> HashMap<String, HashSet<String>> {
    let Ok(out) = Command::new("dconf").args(["dump", "/"]).output() else { return HashMap::new() };
    parse_dump(&String::from_utf8_lossy(&out.stdout))
}

fn parse_dump(text: &str) -> HashMap<String, HashSet<String>> {
    let mut out: HashMap<String, HashSet<String>> = HashMap::new();
    let mut section = String::new();
    for line in text.lines() {
        if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = format!("/{}/", s.trim_matches('/'));
        } else if let Some((k, _)) = line.split_once('=') {
            out.entry(section.clone()).or_default().insert(k.trim().to_string());
        }
    }
    out
}

/// Every key of every installed schema.
pub fn all() -> anyhow::Result<Vec<Key>> {
    let paths: HashMap<String, String> = gsettings(&["list-schemas", "--print-paths"])?
        .lines()
        .filter_map(|l| l.split_once(' ').map(|(s, p)| (s.to_string(), p.trim().to_string())))
        .collect();
    let changed = changed();
    let mut out = Vec::new();
    for line in gsettings(&["list-recursively"])?.lines() {
        let mut f = line.splitn(3, ' ');
        let (Some(schema), Some(key), Some(value)) = (f.next(), f.next(), f.next()) else { continue };
        let modified = paths.get(schema).and_then(|p| changed.get(p)).is_some_and(|keys| keys.contains(key));
        out.push(Key { schema: schema.into(), key: key.into(), value: value.into(), modified });
    }
    out.sort_by(|a, b| a.schema.cmp(&b.schema).then(a.key.cmp(&b.key)));
    Ok(out)
}

/// Plain-language description from the schema, if it has one.
pub fn describe(schema: &str, key: &str) -> String {
    gsettings(&["describe", schema, key]).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// Allowed values for enum keys (empty for free-form ones).
pub fn choices(schema: &str, key: &str) -> Vec<String> {
    let Ok(r) = gsettings(&["range", schema, key]) else { return Vec::new() };
    let mut lines = r.lines();
    if lines.next().map(str::trim) != Some("enum") {
        return Vec::new();
    }
    lines.map(|l| l.trim().trim_matches('\'').to_string()).filter(|l| !l.is_empty()).collect()
}

/// Set a key from GVariant text. `gsettings` validates the type.
pub fn set(schema: &str, key: &str, value: &str) -> anyhow::Result<()> {
    gsettings(&["set", schema, key, value]).map(|_| ())
}

pub fn reset(schema: &str, key: &str) -> anyhow::Result<()> {
    gsettings(&["reset", schema, key]).map(|_| ())
}

pub fn get(schema: &str, key: &str) -> anyhow::Result<String> {
    Ok(gsettings(&["get", schema, key])?.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dconf_dump() {
        let m = parse_dump(
            "[org/gnome/desktop/interface]\ncolor-scheme='prefer-dark'\ngtk-theme='Yaru'\n\n[apps/x]\nn=1\n",
        );
        assert!(m["/org/gnome/desktop/interface/"].contains("color-scheme"));
        assert_eq!(m["/apps/x/"].len(), 1);
    }
}
