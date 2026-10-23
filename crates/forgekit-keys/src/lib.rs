//! Keyboard pieces shared by Forgekit's app and its privileged helper: key
//! names, Keysmith rules, Umlaut's accent tables, and the remapping engine.
//! Kept free of I/O and heavy dependencies so the helper stays small.

pub mod engine;
pub mod keys;
pub mod rules;
pub mod umlaut;

use serde::{Deserialize, Serialize};

/// Everything the helper needs, sent by the app over D-Bus as JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HelperConfig {
    pub keysmith_enabled: bool,
    pub rules: Vec<rules::Rule>,
    pub umlaut: umlaut::Settings,
}

impl HelperConfig {
    pub fn validate(&self) -> Result<(), String> {
        for r in &self.rules {
            r.validate()?;
        }
        rules::check_unique(&self.rules)?;
        if !(100..=2000).contains(&self.umlaut.delay_ms) {
            return Err("Umlaut's hold delay must be between 100 and 2000 ms".into());
        }
        Ok(())
    }
}
