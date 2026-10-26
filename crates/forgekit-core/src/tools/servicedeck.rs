//! Servicedeck: systemd services. Listing and logs come from `systemctl` and
//! `journalctl`; changes go through systemd's D-Bus API, which asks the
//! desktop's polkit agent for the password when a system service needs it.

use std::collections::HashMap;
use std::process::Command;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    System,
    User,
}

impl Scope {
    fn flag(self) -> Option<&'static str> {
        match self {
            Scope::System => None,
            Scope::User => Some("--user"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Service {
    pub unit: String,
    pub description: String,
    /// active, inactive, failed, activating…
    pub active: String,
    /// running, exited, dead…
    pub sub: String,
    /// enabled, disabled, static, masked… (from the unit file)
    pub file_state: String,
}

#[derive(Deserialize)]
struct Unit {
    unit: String,
    active: String,
    sub: String,
    description: String,
}

#[derive(Deserialize)]
struct UnitFile {
    unit_file: String,
    state: String,
}

fn systemctl(scope: Scope, args: &[&str]) -> anyhow::Result<String> {
    let mut c = Command::new("systemctl");
    if let Some(f) = scope.flag() {
        c.arg(f);
    }
    let out = c.args(args).arg("--no-pager").output().context("running systemctl")?;
    if !out.status.success() {
        bail!("systemctl {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Every service systemd knows about in `scope`, loaded or not.
pub fn list(scope: Scope) -> anyhow::Result<Vec<Service>> {
    let units: Vec<Unit> =
        serde_json::from_str(&systemctl(scope, &["list-units", "--type=service", "--all", "--output=json"])?)?;
    let files: Vec<UnitFile> =
        serde_json::from_str(&systemctl(scope, &["list-unit-files", "--type=service", "--output=json"])?)?;
    let states: HashMap<String, String> = files
        .into_iter()
        .map(|f| (f.unit_file.rsplit('/').next().unwrap_or(&f.unit_file).to_string(), f.state))
        .collect();
    let mut out: Vec<Service> = units
        .into_iter()
        .filter(|u| !u.unit.contains('@') || u.active == "active")
        .map(|u| Service {
            file_state: states.get(&u.unit).cloned().unwrap_or_else(|| "transient".into()),
            unit: u.unit,
            description: u.description,
            active: u.active,
            sub: u.sub,
        })
        .collect();
    out.sort_by(|a, b| a.unit.cmp(&b.unit));
    Ok(out)
}

/// The last `lines` log lines for a unit.
pub fn logs(scope: Scope, unit: &str, lines: u32) -> anyhow::Result<String> {
    check_name(unit)?;
    let mut c = Command::new("journalctl");
    c.args(["--no-pager", "-o", "short-iso", "-n", &lines.to_string()]);
    match scope {
        Scope::System => c.args(["-u", unit]),
        Scope::User => c.args(["--user-unit", unit]),
    };
    let out = c.output().context("running journalctl")?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if text.trim().is_empty() || text.trim() == "-- No entries --" {
        return Ok("No log entries you can read. System logs need the adm or systemd-journal group.".into());
    }
    Ok(text)
}

pub fn check_name(unit: &str) -> anyhow::Result<()> {
    let ok = unit.ends_with(".service")
        && unit.len() < 256
        && unit.chars().all(|c| c.is_ascii_alphanumeric() || "@._:-\\".contains(c));
    if !ok {
        bail!("'{unit}' isn't a service name");
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Stop,
    Restart,
    Enable,
    Disable,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Start => "start",
            Action::Stop => "stop",
            Action::Restart => "restart",
            Action::Enable => "enable",
            Action::Disable => "disable",
        }
    }
}

/// The systemd D-Bus call for an action: (method, arguments as GVariant text
/// is left to the caller); exposed so the app can make the call with
/// interactive authorization. Returns (method, is_unit_file_call).
pub fn dbus_method(a: Action) -> (&'static str, bool) {
    match a {
        Action::Start => ("StartUnit", false),
        Action::Stop => ("StopUnit", false),
        Action::Restart => ("RestartUnit", false),
        Action::Enable => ("EnableUnitFiles", true),
        Action::Disable => ("DisableUnitFiles", true),
    }
}

/// From a terminal: let `systemctl` handle authentication itself.
pub fn run_cli(scope: Scope, a: Action, unit: &str) -> anyhow::Result<()> {
    check_name(unit)?;
    let mut c = Command::new("systemctl");
    if let Some(f) = scope.flag() {
        c.arg(f);
    }
    if !c.args([a.label(), unit]).status().context("running systemctl")?.success() {
        bail!("couldn't {} {unit}", a.label());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(check_name("ssh.service").is_ok());
        assert!(check_name("getty@tty1.service").is_ok());
        assert!(check_name("x; rm -rf /.service").is_err());
        assert!(check_name("ssh").is_err());
    }

    #[test]
    fn lists_real_services() {
        // Every Linux test machine with systemd has at least one service.
        if std::path::Path::new("/run/systemd/system").exists() {
            assert!(!list(Scope::System).unwrap().is_empty());
        }
    }
}
