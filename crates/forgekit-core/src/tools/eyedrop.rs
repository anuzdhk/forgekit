//! Eyedrop: pick a color anywhere on screen through the Screenshot portal's
//! PickColor, and keep a short history.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Hex,
    Rgb,
    Hsl,
}

impl std::str::FromStr for Format {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "hex" => Ok(Format::Hex),
            "rgb" => Ok(Format::Rgb),
            "hsl" => Ok(Format::Hsl),
            _ => anyhow::bail!("unknown format '{s}' (hex, rgb, hsl)"),
        }
    }
}

impl Rgb {
    pub fn from_unit(r: f64, g: f64, b: f64) -> Self {
        let c = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Rgb(c(r), c(g), c(b))
    }

    pub fn format(self, f: Format) -> String {
        let Rgb(r, g, b) = self;
        match f {
            Format::Hex => format!("#{r:02X}{g:02X}{b:02X}"),
            Format::Rgb => format!("rgb({r}, {g}, {b})"),
            Format::Hsl => {
                let (h, s, l) = self.hsl();
                format!("hsl({h}, {s}%, {l}%)")
            }
        }
    }

    fn hsl(self) -> (u32, u32, u32) {
        let (r, g, b) = (self.0 as f64 / 255.0, self.1 as f64 / 255.0, self.2 as f64 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        let d = max - min;
        if d == 0.0 {
            return (0, 0, (l * 100.0).round() as u32);
        }
        let s = d / (1.0 - (2.0 * l - 1.0).abs());
        let h = if max == r {
            60.0 * (((g - b) / d).rem_euclid(6.0))
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        (h.round() as u32 % 360, (s * 100.0).round() as u32, (l * 100.0).round() as u32)
    }
}

/// Ask the desktop to let the user pick a pixel.
pub async fn pick() -> anyhow::Result<Rgb> {
    let color = ashpd::desktop::Color::pick().send().await?.response()?;
    Ok(Rgb::from_unit(color.red(), color.green(), color.blue()))
}

const HISTORY_LEN: usize = 32;

fn history_file() -> PathBuf {
    crate::paths::data_dir("eyedrop").join("history.json")
}

/// Newest first.
pub fn history() -> Vec<Rgb> {
    std::fs::read_to_string(history_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn remember(c: Rgb) -> anyhow::Result<()> {
    let mut h = history();
    h.retain(|x| *x != c);
    h.insert(0, c);
    h.truncate(HISTORY_LEN);
    let f = history_file();
    std::fs::create_dir_all(f.parent().unwrap())?;
    std::fs::write(f, serde_json::to_string(&h)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        let c = Rgb::from_unit(1.0, 0.5, 0.0);
        assert_eq!(c.format(Format::Hex), "#FF8000");
        assert_eq!(c.format(Format::Rgb), "rgb(255, 128, 0)");
        assert_eq!(c.format(Format::Hsl), "hsl(30, 100%, 50%)");
        assert_eq!(Rgb(128, 128, 128).format(Format::Hsl), "hsl(0, 0%, 50%)");
        assert_eq!(Rgb(0, 0, 255).format(Format::Hsl), "hsl(240, 100%, 50%)");
    }
}
