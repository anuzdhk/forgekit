//! Mosaic: window zones. Layouts are fractions of a monitor's work area; the
//! desktop backends (GNOME extension, KWin scripts) turn them into pixels with
//! [`zone_rect`]'s formula.

use serde::{Deserialize, Serialize};

/// A zone as fractions (0–1) of the work area.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

const fn z(x: f64, y: f64, w: f64, h: f64) -> Zone {
    Zone { x, y, w, h }
}

/// (id, name, zones). "grid" is built from the custom columns × rows setting.
pub const PRESETS: &[(&str, &str, &[Zone])] = &[
    ("halves", "Halves", &[z(0.0, 0.0, 0.5, 1.0), z(0.5, 0.0, 0.5, 1.0)]),
    (
        "thirds",
        "Thirds",
        &[z(0.0, 0.0, 1.0 / 3.0, 1.0), z(1.0 / 3.0, 0.0, 1.0 / 3.0, 1.0), z(2.0 / 3.0, 0.0, 1.0 / 3.0, 1.0)],
    ),
    (
        "quarters",
        "Quarters",
        &[z(0.0, 0.0, 0.5, 0.5), z(0.5, 0.0, 0.5, 0.5), z(0.0, 0.5, 0.5, 0.5), z(0.5, 0.5, 0.5, 0.5)],
    ),
    ("focus", "Focus", &[z(0.0, 0.0, 0.25, 1.0), z(0.25, 0.0, 0.5, 1.0), z(0.75, 0.0, 0.25, 1.0)]),
    ("priority", "Priority", &[z(0.0, 0.0, 0.65, 1.0), z(0.65, 0.0, 0.35, 0.5), z(0.65, 0.5, 0.35, 0.5)]),
    ("wide", "One-third / two-thirds", &[z(0.0, 0.0, 1.0 / 3.0, 1.0), z(1.0 / 3.0, 0.0, 2.0 / 3.0, 1.0)]),
    ("rows", "Rows", &[z(0.0, 0.0, 1.0, 0.5), z(0.0, 0.5, 1.0, 0.5)]),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// A preset id, or "grid" for `columns` × `rows`.
    pub layout: String,
    pub columns: u8,
    pub rows: u8,
    /// Pixels between zones and around the edges.
    pub gap: u32,
    /// Hold Shift while dragging a window to show zones (GNOME).
    pub shift_drag: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { layout: "halves".into(), columns: 3, rows: 2, gap: 8, shift_drag: true }
    }
}

impl Settings {
    pub fn load() -> anyhow::Result<Self> {
        crate::settings::load("mosaic")
    }

    pub fn save(&self) -> anyhow::Result<()> {
        crate::settings::save("mosaic", self)
    }

    pub fn zones(&self) -> Vec<Zone> {
        if self.layout == "grid" {
            return grid(self.columns, self.rows);
        }
        PRESETS
            .iter()
            .find(|(id, _, _)| *id == self.layout)
            .map(|(_, _, z)| z.to_vec())
            .unwrap_or_else(|| PRESETS[0].2.to_vec())
    }
}

pub fn grid(columns: u8, rows: u8) -> Vec<Zone> {
    let (c, r) = (columns.clamp(1, 6) as f64, rows.clamp(1, 4) as f64);
    let mut out = Vec::new();
    for row in 0..rows.clamp(1, 4) {
        for col in 0..columns.clamp(1, 6) {
            out.push(z(col as f64 / c, row as f64 / r, 1.0 / c, 1.0 / r));
        }
    }
    out
}

/// A zone in pixels inside `area` (x, y, w, h), with `gap` around every zone:
/// a full gap at the screen edges and half a gap on each side between zones,
/// so neighbours are exactly `gap` apart. Mirrored in the GNOME extension and
/// the KWin script.
pub fn zone_rect(zone: Zone, area: (i32, i32, i32, i32), gap: u32) -> (i32, i32, i32, i32) {
    let (ax, ay, aw, ah) = (area.0 as f64, area.1 as f64, area.2 as f64, area.3 as f64);
    let g = gap as f64;
    let edge = |frac: f64| if frac <= 0.0001 || frac >= 0.9999 { g } else { g / 2.0 };
    let left = ax + zone.x * aw + edge(zone.x);
    let top = ay + zone.y * ah + edge(zone.y);
    let right = ax + (zone.x + zone.w) * aw - edge(zone.x + zone.w);
    let bottom = ay + (zone.y + zone.h) * ah - edge(zone.y + zone.h);
    (left.round() as i32, top.round() as i32, (right - left).round() as i32, (bottom - top).round() as i32)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PinSettings {
    /// Draw a colored border around pinned windows (GNOME).
    pub border: bool,
    /// Border color, `#rrggbb`.
    pub color: String,
}

impl Default for PinSettings {
    fn default() -> Self {
        PinSettings { border: true, color: "#3b82f6".into() }
    }
}

impl PinSettings {
    pub fn load() -> anyhow::Result<Self> {
        crate::settings::load("pin")
    }

    pub fn save(&self) -> anyhow::Result<()> {
        crate::settings::save("pin", self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_cover_the_screen() {
        for (id, _, zones) in PRESETS {
            let area: f64 = zones.iter().map(|z| z.w * z.h).sum();
            assert!((area - 1.0).abs() < 1e-9, "{id} covers {area}");
        }
        assert_eq!(grid(3, 2).len(), 6);
        assert_eq!(Settings { layout: "grid".into(), columns: 9, rows: 9, ..Default::default() }.zones().len(), 24);
        assert_eq!(Settings { layout: "nope".into(), ..Default::default() }.zones().len(), 2);
    }

    #[test]
    fn gaps_are_even() {
        let area = (0, 32, 1920, 1048);
        let halves = PRESETS[0].2;
        let l = zone_rect(halves[0], area, 8);
        let r = zone_rect(halves[1], area, 8);
        assert_eq!(l, (8, 40, 948, 1032));
        assert_eq!(r, (964, 40, 948, 1032));
        // Exactly one gap between neighbours.
        assert_eq!(r.0 - (l.0 + l.2), 8);
        assert_eq!(zone_rect(halves[0], area, 0), (0, 32, 960, 1048));
    }
}
