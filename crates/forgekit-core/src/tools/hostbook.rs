//! Hostbook: edit `/etc/hosts` safely.
//!
//! Every line is kept, comments and spacing included. Entries Hostbook turns
//! off are commented out with a `#off ` prefix so they can be turned back on.
//! Saving keeps a timestamped backup and writes through `pkexec`.

use std::net::IpAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::Serialize;

pub const HOSTS: &str = "/etc/hosts";
const OFF: &str = "#off ";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Line {
    Entry(Entry),
    /// Comments, blank lines and anything we don't understand, kept verbatim.
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub enabled: bool,
    pub ip: IpAddr,
    pub names: Vec<String>,
    pub comment: Option<String>,
    /// The original line, written back unchanged unless the entry is edited.
    #[serde(skip)]
    raw: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Hosts {
    pub lines: Vec<Line>,
}

impl Hosts {
    pub fn parse(text: &str) -> Self {
        Hosts { lines: text.lines().map(parse_line).collect() }
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        Ok(Self::parse(&std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?))
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.lines.iter().filter_map(|l| match l {
            Line::Entry(e) => Some(e),
            Line::Other(_) => None,
        })
    }

    fn entries_mut(&mut self) -> impl Iterator<Item = &mut Entry> {
        self.lines.iter_mut().filter_map(|l| match l {
            Line::Entry(e) => Some(e),
            Line::Other(_) => None,
        })
    }

    pub fn add(&mut self, ip: IpAddr, names: Vec<String>) -> anyhow::Result<()> {
        if names.is_empty() {
            bail!("Give at least one host name");
        }
        if let Some(bad) = names.iter().find(|n| !valid_name(n)) {
            bail!("'{bad}' isn't a valid host name");
        }
        self.lines.push(Line::Entry(Entry { enabled: true, ip, names, comment: None, raw: None }));
        Ok(())
    }

    /// Turn every entry mentioning `name` on or off. Returns how many changed.
    pub fn set_enabled(&mut self, name: &str, on: bool) -> usize {
        let mut n = 0;
        for e in self.entries_mut().filter(|e| e.names.iter().any(|x| x == name)) {
            if e.enabled != on {
                e.enabled = on;
                e.raw = None;
                n += 1;
            }
        }
        n
    }

    /// Remove `name` from entries; entries left with no names are dropped.
    pub fn remove(&mut self, name: &str) -> usize {
        let mut touched = 0;
        for e in self.entries_mut() {
            let len = e.names.len();
            e.names.retain(|x| x != name);
            if e.names.len() != len {
                e.raw = None;
                touched += 1;
            }
        }
        self.lines.retain(|l| !matches!(l, Line::Entry(e) if e.names.is_empty()));
        touched
    }

    pub fn render(&self) -> String {
        let mut s = String::new();
        for l in &self.lines {
            match l {
                Line::Other(t) | Line::Entry(Entry { raw: Some(t), .. }) => s.push_str(t),
                Line::Entry(e) => {
                    if !e.enabled {
                        s.push_str(OFF);
                    }
                    s.push_str(&format!("{}\t{}", e.ip, e.names.join(" ")));
                    if let Some(c) = &e.comment {
                        s.push_str(&format!(" # {c}"));
                    }
                }
            }
            s.push('\n');
        }
        s
    }

    /// Back up `target`, then replace it (through `pkexec` when needed).
    pub fn save(&self, target: &Path) -> anyhow::Result<Option<PathBuf>> {
        crate::privileged::replace_file("hostbook", target, &self.render())
    }
}

pub fn backups() -> Vec<PathBuf> {
    crate::privileged::backups("hostbook")
}

fn parse_line(raw: &str) -> Line {
    let (enabled, body) = match raw.strip_prefix(OFF) {
        Some(rest) => (false, rest),
        None => (true, raw),
    };
    let (data, comment) = match body.split_once('#') {
        Some((d, c)) => (d, Some(c.trim_start().to_string()).filter(|c| !c.is_empty())),
        None => (body, None),
    };
    let mut words = data.split_whitespace();
    let ip = words.next().and_then(|w| w.parse::<IpAddr>().ok());
    let names: Vec<String> = words.map(str::to_string).collect();
    match ip {
        Some(ip) if !names.is_empty() => Line::Entry(Entry { enabled, ip, names, comment, raw: Some(raw.to_string()) }),
        _ => Line::Other(raw.to_string()),
    }
}

fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 253
        && n.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str =
        "127.0.0.1 localhost\n# comment\n\n::1     ip6-localhost ip6-loopback\n#off 10.0.0.5\tdev.local # staging\n";

    #[test]
    fn roundtrip_keeps_unknown_lines() {
        let h = Hosts::parse(SAMPLE);
        assert_eq!(h.entries().count(), 3);
        assert!(!h.entries().nth(2).unwrap().enabled);
        let again = Hosts::parse(&h.render());
        assert_eq!(h, again);
        assert_eq!(h.render(), SAMPLE);
    }

    #[test]
    fn toggle_add_remove() {
        let mut h = Hosts::parse(SAMPLE);
        assert_eq!(h.set_enabled("dev.local", true), 1);
        assert!(h.render().contains("\n10.0.0.5\tdev.local # staging\n"));
        h.add("192.168.1.2".parse().unwrap(), vec!["nas".into()]).unwrap();
        assert!(h.add("1.2.3.4".parse().unwrap(), vec!["bad name".into()]).is_err());
        assert_eq!(h.remove("ip6-loopback"), 1);
        assert_eq!(h.remove("nas"), 1);
        assert_eq!(h.entries().count(), 3);
    }

    #[test]
    fn save_to_writable_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("hosts");
        std::fs::write(&p, SAMPLE).unwrap();
        let mut h = Hosts::load(&p).unwrap();
        h.set_enabled("localhost", false);
        let b = h.save(&p).unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(&b).unwrap(), SAMPLE);
        assert!(std::fs::read_to_string(&p).unwrap().starts_with("#off 127.0.0.1"));
        std::fs::remove_file(b).unwrap();
    }
}
