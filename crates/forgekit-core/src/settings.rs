//! Settings in `~/.config/forgekit/`, one TOML file per tool, plus
//! `general.toml` holding the per-tool on/off toggles.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::paths::config_dir;

fn file(name: &str) -> PathBuf {
    config_dir().join(format!("{name}.toml"))
}

/// Load a tool's settings, falling back to defaults when the file is missing.
pub fn load<T: DeserializeOwned + Default>(tool: &str) -> anyhow::Result<T> {
    let path = file(tool);
    match std::fs::read_to_string(&path) {
        Ok(s) => toml::from_str(&s).with_context(|| format!("reading {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

pub fn save<T: Serialize>(tool: &str, value: &T) -> anyhow::Result<()> {
    let path = file(tool);
    std::fs::create_dir_all(config_dir())?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, toml::to_string_pretty(value)?)?;
    std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct General {
    /// Tools the user switched off. Everything else is on.
    #[serde(default)]
    pub disabled: BTreeMap<String, bool>,
}

impl General {
    pub fn load() -> anyhow::Result<Self> {
        load("general")
    }

    pub fn save(&self) -> anyhow::Result<()> {
        save("general", self)
    }

    pub fn is_enabled(&self, tool: &str) -> bool {
        !self.disabled.get(tool).copied().unwrap_or(false)
    }

    pub fn set_enabled(&mut self, tool: &str, on: bool) {
        if on {
            self.disabled.remove(tool);
        } else {
            self.disabled.insert(tool.to_string(), true);
        }
    }
}
