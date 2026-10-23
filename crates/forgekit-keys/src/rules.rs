//! Keysmith rules: remap a key, turn a shortcut into another, or disable a key.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::keys::{self, Mod};

/// Modifiers plus one key, e.g. Ctrl+Alt+T. A modifier key on its own (e.g.
/// CapsLock → LeftCtrl) is a combo with no modifiers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Combo {
    pub mods: BTreeSet<Mod>,
    pub key: u16,
}

impl Combo {
    pub fn key(key: u16) -> Self {
        Combo { mods: BTreeSet::new(), key }
    }

    /// `ctrl+alt+t`, `CapsLock`, `super+Return` (case-insensitive).
    pub fn parse(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
        let Some((last, mods)) = parts.split_last() else {
            return Err("empty shortcut".into());
        };
        let key = keys::code(last).ok_or_else(|| format!("unknown key '{last}'"))?;
        let mut set = BTreeSet::new();
        for m in mods {
            let c = keys::code(m).ok_or_else(|| format!("unknown key '{m}'"))?;
            let m = Mod::of(c).ok_or_else(|| format!("'{m}' isn't a modifier (Ctrl, Alt, Shift, Super)"))?;
            set.insert(m);
        }
        Ok(Combo { mods: set, key })
    }
}

impl fmt::Display for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for m in &self.mods {
            write!(f, "{}+", m.label())?;
        }
        write!(f, "{}", keys::name(self.key))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "combo", rename_all = "lowercase")]
pub enum Target {
    Keys(Combo),
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub from: Combo,
    pub to: Target,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// One key becomes another everywhere, including while held (CapsLock → Ctrl).
    Remap,
    /// A shortcut fires a different shortcut.
    Shortcut,
    Disable,
}

impl Rule {
    pub fn kind(&self) -> Kind {
        match &self.to {
            Target::Disabled => Kind::Disable,
            Target::Keys(to) if self.from.mods.is_empty() && to.mods.is_empty() => Kind::Remap,
            Target::Keys(_) => Kind::Shortcut,
        }
    }

    /// `capslock -> ctrl`, `ctrl+h -> backspace`, `insert -> none`.
    pub fn parse(from: &str, to: &str) -> Result<Self, String> {
        let from = Combo::parse(from)?;
        let to = match to.trim().to_ascii_lowercase().as_str() {
            "none" | "disable" | "disabled" | "off" => Target::Disabled,
            _ => Target::Keys(Combo::parse(to)?),
        };
        let r = Rule { from, to };
        r.validate()?;
        Ok(r)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.from.key == keys::ESC && [Mod::Ctrl, Mod::Alt, Mod::Shift].iter().all(|m| self.from.mods.contains(m)) {
            return Err("Ctrl+Alt+Shift+Esc is reserved: it always releases the keyboard".into());
        }
        if Target::Keys(self.from.clone()) == self.to {
            return Err("a key can't be remapped to itself".into());
        }
        Ok(())
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.to {
            Target::Keys(c) => write!(f, "{} → {c}", self.from),
            Target::Disabled => write!(f, "{} → (disabled)", self.from),
        }
    }
}

/// Reject a rule list where two rules have the same trigger.
pub fn check_unique(rules: &[Rule]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for r in rules {
        if !seen.insert(&r.from) {
            return Err(format!("{} is used by more than one rule", r.from));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing_and_kinds() {
        let r = Rule::parse("CapsLock", "ctrl").unwrap();
        assert_eq!(r.kind(), Kind::Remap);
        assert_eq!(r.to_string(), "CapsLock → LeftCtrl");
        let s = Rule::parse("ctrl+alt+t", "super+enter").unwrap();
        assert_eq!(s.kind(), Kind::Shortcut);
        assert_eq!(s.to_string(), "Ctrl+Alt+T → Super+Enter");
        assert_eq!(Rule::parse("Insert", "none").unwrap().kind(), Kind::Disable);
        assert!(Rule::parse("ctrl+alt+shift+esc", "a").is_err());
        assert!(Rule::parse("a+b", "c").is_err());
        assert!(Rule::parse("a", "a").is_err());
        assert!(Rule::parse("bogus", "a").is_err());
        assert!(check_unique(&[r.clone(), r]).is_err());
    }
}
