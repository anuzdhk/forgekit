//! Dusk: switch the desktop between light and dark, by hand, at fixed times,
//! or at sunrise/sunset for a location.

use std::process::Command;

use anyhow::{Context, bail};
use chrono::{DateTime, Datelike, Local, NaiveTime, TimeZone, Timelike};
use serde::{Deserialize, Serialize};

use crate::capability::Desktop;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Light,
    Dark,
}

impl Mode {
    pub fn flip(self) -> Self {
        match self {
            Mode::Light => Mode::Dark,
            Mode::Dark => Mode::Light,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Schedule {
    /// Dusk does nothing on its own.
    #[default]
    Manual,
    /// Light from `light` until `dark` (local time, "HH:MM").
    Times { light: String, dark: String },
    /// Light from sunrise to sunset at this location.
    Sun { latitude: f64, longitude: f64 },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub schedule: Schedule,
    /// KDE color schemes to use (default BreezeLight / BreezeDark).
    #[serde(default)]
    pub kde_light: Option<String>,
    #[serde(default)]
    pub kde_dark: Option<String>,
}

impl Settings {
    pub fn load() -> anyhow::Result<Self> {
        crate::settings::load("dusk")
    }

    pub fn save(&self) -> anyhow::Result<()> {
        crate::settings::save("dusk", self)
    }
}

/// What the schedule wants right now; `None` for manual.
pub fn desired(schedule: &Schedule, now: DateTime<Local>) -> anyhow::Result<Option<Mode>> {
    let (light, dark) = match schedule {
        Schedule::Manual => return Ok(None),
        Schedule::Times { light, dark } => (parse_time(light)?, parse_time(dark)?),
        Schedule::Sun { latitude, longitude } => match sun_times(now, *latitude, *longitude) {
            Some(t) => t,
            // Polar day or night: pick by latitude and season.
            None => return Ok(Some(if is_polar_day(now, *latitude) { Mode::Light } else { Mode::Dark })),
        },
    };
    let t = now.time();
    let is_light = if light <= dark { t >= light && t < dark } else { t >= light || t < dark };
    Ok(Some(if is_light { Mode::Light } else { Mode::Dark }))
}

pub fn parse_time(s: &str) -> anyhow::Result<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").with_context(|| format!("'{s}' isn't a time like 07:30"))
}

/// Local sunrise and sunset (NOAA's simplified solar equations, about a minute accurate).
pub fn sun_times(now: DateTime<Local>, lat: f64, lon: f64) -> Option<(NaiveTime, NaiveTime)> {
    let day = now.ordinal() as f64;
    let g = 2.0 * std::f64::consts::PI / 365.0 * (day - 1.0);
    let eqtime = 229.18
        * (0.000075 + 0.001868 * g.cos()
            - 0.032077 * g.sin()
            - 0.014615 * (2.0 * g).cos()
            - 0.040849 * (2.0 * g).sin());
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos()
        + 0.000907 * (2.0 * g).sin()
        - 0.002697 * (3.0 * g).cos()
        + 0.00148 * (3.0 * g).sin();
    let lat_r = lat.to_radians();
    let cos_ha = (90.833f64.to_radians().cos() / (lat_r.cos() * decl.cos())) - lat_r.tan() * decl.tan();
    if !(-1.0..=1.0).contains(&cos_ha) {
        return None;
    }
    let ha = cos_ha.acos().to_degrees();
    let offset_min = now.offset().local_minus_utc() as f64 / 60.0;
    let to_time = |utc_min: f64| {
        let m = (utc_min + offset_min).rem_euclid(1440.0);
        NaiveTime::from_num_seconds_from_midnight_opt((m * 60.0) as u32, 0)
    };
    Some((to_time(720.0 - 4.0 * (lon + ha) - eqtime)?, to_time(720.0 - 4.0 * (lon - ha) - eqtime)?))
}

fn is_polar_day(now: DateTime<Local>, lat: f64) -> bool {
    let summer_north = (80..266).contains(&now.ordinal());
    (lat > 0.0) == summer_north
}

/// When the schedule next flips, for display.
pub fn next_change(schedule: &Schedule, now: DateTime<Local>) -> Option<DateTime<Local>> {
    let current = desired(schedule, now).ok()??;
    let start = now.with_second(0)?.with_nanosecond(0)?;
    (1..=48 * 60)
        .map(|m| start + chrono::Duration::minutes(m))
        .find(|t| desired(schedule, *t).ok().flatten() != Some(current))
}

// ------------------------------------------------------------------ applying

pub fn current(desktop: Desktop) -> anyhow::Result<Mode> {
    match desktop {
        Desktop::Kde => {
            let out =
                run_output("kreadconfig6", &["--file", "kdeglobals", "--group", "General", "--key", "ColorScheme"])?;
            Ok(if out.to_lowercase().contains("dark") { Mode::Dark } else { Mode::Light })
        }
        Desktop::Xfce => {
            let out = run_output("xfconf-query", &["-c", "xsettings", "-p", "/Net/ThemeName"])?;
            Ok(if out.to_lowercase().contains("dark") { Mode::Dark } else { Mode::Light })
        }
        _ => {
            let out = run_output("gsettings", &["get", "org.gnome.desktop.interface", "color-scheme"])?;
            Ok(if out.contains("dark") { Mode::Dark } else { Mode::Light })
        }
    }
}

pub fn apply(mode: Mode, desktop: Desktop, settings: &Settings) -> anyhow::Result<()> {
    match desktop {
        Desktop::Kde => {
            let scheme = match mode {
                Mode::Light => settings.kde_light.as_deref().unwrap_or("BreezeLight"),
                Mode::Dark => settings.kde_dark.as_deref().unwrap_or("BreezeDark"),
            };
            run("plasma-apply-colorscheme", &[scheme])
        }
        Desktop::Xfce => {
            let theme = run_output("xfconf-query", &["-c", "xsettings", "-p", "/Net/ThemeName"])?;
            match theme_variant(&theme, mode) {
                Some(t) => run("xfconf-query", &["-c", "xsettings", "-p", "/Net/ThemeName", "-s", &t]),
                None => Ok(()),
            }
        }
        _ => {
            let scheme = match mode {
                Mode::Light => "default",
                Mode::Dark => "prefer-dark",
            };
            run("gsettings", &["set", "org.gnome.desktop.interface", "color-scheme", scheme])?;
            // Older GTK3 apps follow the theme name, not color-scheme.
            if let Ok(theme) = run_output("gsettings", &["get", "org.gnome.desktop.interface", "gtk-theme"])
                && let Some(t) = theme_variant(theme.trim_matches('\''), mode)
            {
                run("gsettings", &["set", "org.gnome.desktop.interface", "gtk-theme", &t])?;
            }
            Ok(())
        }
    }
}

/// `Yaru-purple` ↔ `Yaru-purple-dark`, only if the target theme is installed.
fn theme_variant(theme: &str, mode: Mode) -> Option<String> {
    let base = theme.trim().strip_suffix("-dark").unwrap_or(theme.trim());
    let want = match mode {
        Mode::Light => base.to_string(),
        Mode::Dark => format!("{base}-dark"),
    };
    if want == theme.trim() {
        return None;
    }
    let dirs = [dirs::data_dir().unwrap_or_default().join("themes"), "/usr/share/themes".into()];
    let installed = dirs.iter().any(|d| d.join(&want).exists())
        || dirs::home_dir().is_some_and(|h| h.join(".themes").join(&want).exists());
    // Adwaita is built into GTK; it has no theme directory.
    (installed || base == "Adwaita").then_some(want)
}

fn run(cmd: &str, args: &[&str]) -> anyhow::Result<()> {
    let st = Command::new(cmd).args(args).status().with_context(|| format!("running {cmd}"))?;
    if !st.success() {
        bail!("{cmd} failed");
    }
    Ok(())
}

fn run_output(cmd: &str, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new(cmd).args(args).output().with_context(|| format!("running {cmd}"))?;
    if !out.status.success() {
        bail!("{cmd} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn local(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Local> {
    Local.with_ymd_and_hms(y, m, d, h, min, 0).single().expect("valid local time")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_times() {
        let s = Schedule::Times { light: "07:00".into(), dark: "19:30".into() };
        assert_eq!(desired(&s, local(2026, 3, 1, 12, 0)).unwrap(), Some(Mode::Light));
        assert_eq!(desired(&s, local(2026, 3, 1, 20, 0)).unwrap(), Some(Mode::Dark));
        assert_eq!(desired(&s, local(2026, 3, 1, 6, 59)).unwrap(), Some(Mode::Dark));
        // Night-shift style: light in the evening.
        let n = Schedule::Times { light: "18:00".into(), dark: "06:00".into() };
        assert_eq!(desired(&n, local(2026, 3, 1, 23, 0)).unwrap(), Some(Mode::Light));
        assert_eq!(desired(&Schedule::Manual, local(2026, 3, 1, 23, 0)).unwrap(), None);
        assert!(parse_time("25:00").is_err());
    }

    #[test]
    fn sun_is_up_at_noon_and_down_at_midnight() {
        // Equator, solar noon near 12:00 UTC at lon 0; compare in local terms.
        let s = Schedule::Sun { latitude: 0.0, longitude: 0.0 };
        let offset_h = local(2026, 6, 1, 12, 0).offset().local_minus_utc() / 3600;
        let noon = (12 + offset_h).rem_euclid(24) as u32;
        let midnight = offset_h.rem_euclid(24) as u32;
        assert_eq!(desired(&s, local(2026, 6, 1, noon, 0)).unwrap(), Some(Mode::Light));
        assert_eq!(desired(&s, local(2026, 6, 1, midnight, 0)).unwrap(), Some(Mode::Dark));
        let (rise, set) = sun_times(local(2026, 6, 1, 12, 0), 0.0, 0.0).unwrap();
        let day_len = (set - rise).num_minutes().rem_euclid(1440);
        assert!((715..=735).contains(&day_len), "{day_len}");
    }

    #[test]
    fn polar_and_next_change() {
        let svalbard = Schedule::Sun { latitude: 78.2, longitude: 15.6 };
        assert_eq!(desired(&svalbard, local(2026, 6, 21, 0, 30)).unwrap(), Some(Mode::Light));
        assert_eq!(desired(&svalbard, local(2026, 12, 21, 12, 0)).unwrap(), Some(Mode::Dark));
        let s = Schedule::Times { light: "07:00".into(), dark: "19:30".into() };
        let next = next_change(&s, local(2026, 3, 1, 12, 0)).unwrap();
        assert_eq!((next.hour(), next.minute()), (19, 30));
    }

    #[test]
    fn theme_variants() {
        assert_eq!(theme_variant("Adwaita", Mode::Dark).as_deref(), Some("Adwaita-dark"));
        assert_eq!(theme_variant("Adwaita-dark", Mode::Light).as_deref(), Some("Adwaita"));
        assert_eq!(theme_variant("Adwaita-dark", Mode::Dark), None);
    }
}
