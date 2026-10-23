//! Envy: environment variables, edited where Linux actually reads them.
//!
//! - **User**: `~/.config/environment.d/60-forgekit.conf`, read by systemd for
//!   the graphical session (applies at next login).
//! - **System**: `/etc/environment`, read by pam_env for every login (needs root).
//!
//! Other files are shown read-only so people can see where a value comes from.
//! Like Hostbook, untouched lines are written back byte-for-byte.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::Serialize;

pub const SYSTEM_FILE: &str = "/etc/environment";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    User,
    System,
}

impl Scope {
    pub fn path(self) -> PathBuf {
        match self {
            Scope::User => user_dir().join("60-forgekit.conf"),
            Scope::System => PathBuf::from(SYSTEM_FILE),
        }
    }
}

pub fn user_dir() -> PathBuf {
    crate::paths::config_dir().parent().map(Path::to_path_buf).unwrap_or_default().join("environment.d")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Var {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    Var(Var, Option<String>),
    Other(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvFile {
    lines: Vec<Line>,
}

impl EnvFile {
    pub fn parse(text: &str) -> Self {
        EnvFile { lines: text.lines().map(parse_line).collect() }
    }

    /// A missing file is an empty one.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(t) => Ok(Self::parse(&t)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn vars(&self) -> impl Iterator<Item = &Var> {
        self.lines.iter().filter_map(|l| match l {
            Line::Var(v, _) => Some(v),
            Line::Other(_) => None,
        })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars().filter(|v| v.key == key).last().map(|v| v.value.as_str())
    }

    /// Set or add a variable. The last definition wins in both file formats,
    /// so earlier duplicates are removed.
    pub fn set(&mut self, key: &str, value: &str) -> anyhow::Result<()> {
        validate_key(key)?;
        if value.contains('\n') {
            bail!("Values can't contain line breaks");
        }
        let mut found = false;
        self.lines.retain_mut(|l| match l {
            Line::Var(v, raw) if v.key == key => {
                if found {
                    return false;
                }
                found = true;
                if v.value != value {
                    v.value = value.to_string();
                    *raw = None;
                }
                true
            }
            _ => true,
        });
        if !found {
            self.lines.push(Line::Var(Var { key: key.into(), value: value.into() }, None));
        }
        Ok(())
    }

    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.lines.len();
        self.lines.retain(|l| !matches!(l, Line::Var(v, _) if v.key == key));
        self.lines.len() != before
    }

    pub fn render(&self) -> String {
        let mut s = String::new();
        for l in &self.lines {
            match l {
                Line::Other(t) | Line::Var(_, Some(t)) => s.push_str(t),
                Line::Var(v, None) => s.push_str(&format!("{}={}", v.key, quote(&v.value))),
            }
            s.push('\n');
        }
        s
    }

    pub fn save(&self, scope: Scope) -> anyhow::Result<Option<PathBuf>> {
        crate::privileged::replace_file("envy", &scope.path(), &self.render())
    }
}

/// Every variable defined in a file Envy knows about, with where it came from.
#[derive(Debug, Clone, Serialize)]
pub struct Source {
    pub path: PathBuf,
    pub editable: Option<Scope>,
    pub vars: Vec<Var>,
}

pub fn sources() -> Vec<Source> {
    let mut out = Vec::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(user_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "conf"))
        .collect();
    files.sort();
    let ours = Scope::User.path();
    if !files.contains(&ours) {
        files.push(ours.clone());
    }
    for path in files.into_iter().chain([PathBuf::from(SYSTEM_FILE)]) {
        let editable = if path == ours {
            Some(Scope::User)
        } else if path == Path::new(SYSTEM_FILE) {
            Some(Scope::System)
        } else {
            None
        };
        let vars = EnvFile::load(&path).map(|f| f.vars().cloned().collect()).unwrap_or_default();
        out.push(Source { path, editable, vars });
    }
    out
}

fn parse_line(raw: &str) -> Line {
    let t = raw.trim_start();
    if t.is_empty() || t.starts_with('#') {
        return Line::Other(raw.to_string());
    }
    let t = t.strip_prefix("export ").unwrap_or(t);
    match t.split_once('=') {
        Some((k, v)) if validate_key(k.trim()).is_ok() => {
            Line::Var(Var { key: k.trim().to_string(), value: unquote(v.trim()) }, Some(raw.to_string()))
        }
        _ => Line::Other(raw.to_string()),
    }
}

fn validate_key(k: &str) -> anyhow::Result<()> {
    let mut chars = k.chars();
    let ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !ok {
        bail!("'{k}' isn't a valid variable name (letters, digits and _, not starting with a digit)");
    }
    Ok(())
}

fn unquote(v: &str) -> String {
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return v[1..v.len() - 1].to_string();
        }
    }
    v.to_string()
}

fn quote(v: &str) -> String {
    if v.chars().any(|c| c.is_whitespace() || c == '#') { format!("\"{}\"", v.replace('"', "\\\"")) } else { v.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# comment\nPATH=\"/usr/local/bin:/usr/bin\"\nexport EDITOR=nvim\n\nbad line\n";

    #[test]
    fn roundtrip_and_edit() {
        let mut f = EnvFile::parse(SAMPLE);
        assert_eq!(f.render(), SAMPLE);
        assert_eq!(f.get("PATH"), Some("/usr/local/bin:/usr/bin"));
        assert_eq!(f.get("EDITOR"), Some("nvim"));
        f.set("EDITOR", "helix").unwrap();
        f.set("GREETING", "hello world").unwrap();
        assert!(f.remove("PATH"));
        assert_eq!(f.render(), "# comment\nEDITOR=helix\n\nbad line\nGREETING=\"hello world\"\n");
        assert!(f.set("1BAD", "x").is_err());
        assert!(f.set("OK", "a\nb").is_err());
    }

    #[test]
    fn duplicate_definitions_collapse() {
        let mut f = EnvFile::parse("A=1\nA=2\n");
        assert_eq!(f.get("A"), Some("2"));
        f.set("A", "3").unwrap();
        assert_eq!(f.render(), "A=3\n");
    }
}
