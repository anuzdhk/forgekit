//! Forgekit core: capability detection, settings, the tool catalog and the
//! tool implementations shared by the CLI, the daemon and the GTK app.

pub mod capability;
pub mod catalog;
pub mod desktop;
pub mod helper;
pub mod paths;
pub mod privileged;
pub mod settings;
pub mod shortcuts;
pub mod tools;
pub mod update;

pub use capability::{Capabilities, Status};
pub use catalog::{CATALOG, Tier, Tool};
