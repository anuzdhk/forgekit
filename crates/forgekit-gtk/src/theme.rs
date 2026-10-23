//! Themes. Each one is a palette that is turned into libadwaita's CSS color
//! variables, so every widget (ours and libadwaita's) follows it. "System"
//! leaves libadwaita alone and follows the desktop's light/dark and accent.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// Window background.
    pub bg: &'static str,
    /// Cards, lists, entries.
    pub surface: &'static str,
    pub sidebar: &'static str,
    pub fg: &'static str,
    pub muted: &'static str,
    pub accent: &'static str,
    pub accent_fg: &'static str,
    pub success: &'static str,
    pub warning: &'static str,
    pub error: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    /// `None` = System.
    pub palette: Option<Palette>,
    /// Glass themes make backgrounds translucent by the opacity setting.
    pub glass: bool,
}

macro_rules! p {
    ($dark:expr, $bg:literal, $surface:literal, $sidebar:literal, $fg:literal, $muted:literal, $accent:literal, $accent_fg:literal, $ok:literal, $warn:literal, $err:literal) => {
        Some(Palette {
            dark: $dark,
            bg: $bg,
            surface: $surface,
            sidebar: $sidebar,
            fg: $fg,
            muted: $muted,
            accent: $accent,
            accent_fg: $accent_fg,
            success: $ok,
            warning: $warn,
            error: $err,
        })
    };
}

const fn t(id: &'static str, name: &'static str, palette: Option<Palette>) -> Theme {
    Theme { id, name, palette, glass: false }
}

pub static THEMES: &[Theme] = &[
    t("system", "System", None),
    t(
        "light",
        "Forge Light",
        p!(
            false, "#f3f4f7", "#ffffff", "#ffffff", "#1d2130", "#6b7185", "#2563eb", "#ffffff", "#16a34a", "#d97706",
            "#dc2626"
        ),
    ),
    t(
        "dark",
        "Forge Dark",
        p!(
            true, "#101217", "#191c23", "#14161c", "#e6e8ee", "#8b91a3", "#5b8cff", "#ffffff", "#4ade80", "#fbbf24",
            "#f87171"
        ),
    ),
    t(
        "latte",
        "Catppuccin Latte",
        p!(
            false, "#eff1f5", "#ffffff", "#e6e9ef", "#4c4f69", "#6c6f85", "#8839ef", "#ffffff", "#40a02b", "#df8e1d",
            "#d20f39"
        ),
    ),
    t(
        "frappe",
        "Catppuccin Frappé",
        p!(
            true, "#303446", "#414559", "#292c3c", "#c6d0f5", "#a5adce", "#ca9ee6", "#232634", "#a6d189", "#e5c890",
            "#e78284"
        ),
    ),
    t(
        "macchiato",
        "Catppuccin Macchiato",
        p!(
            true, "#24273a", "#363a4f", "#1e2030", "#cad3f5", "#a5adcb", "#c6a0f6", "#181926", "#a6da95", "#eed49f",
            "#ed8796"
        ),
    ),
    t(
        "mocha",
        "Catppuccin Mocha",
        p!(
            true, "#1e1e2e", "#313244", "#181825", "#cdd6f4", "#a6adc8", "#cba6f7", "#11111b", "#a6e3a1", "#f9e2af",
            "#f38ba8"
        ),
    ),
    t(
        "dracula",
        "Dracula",
        p!(
            true, "#282a36", "#343746", "#21222c", "#f8f8f2", "#a0a4c0", "#bd93f9", "#21222c", "#50fa7b", "#ffb86c",
            "#ff5555"
        ),
    ),
    t(
        "nord",
        "Nord",
        p!(
            true, "#2e3440", "#3b4252", "#292e39", "#eceff4", "#a3abbd", "#88c0d0", "#2e3440", "#a3be8c", "#ebcb8b",
            "#bf616a"
        ),
    ),
    t(
        "gruvbox-dark",
        "Gruvbox Dark",
        p!(
            true, "#282828", "#32302f", "#1d2021", "#ebdbb2", "#a89984", "#fe8019", "#1d2021", "#b8bb26", "#fabd2f",
            "#fb4934"
        ),
    ),
    t(
        "gruvbox-light",
        "Gruvbox Light",
        p!(
            false, "#f2e5bc", "#fbf1c7", "#ebdbb2", "#3c3836", "#7c6f64", "#af3a03", "#fbf1c7", "#79740e", "#b57614",
            "#9d0006"
        ),
    ),
    t(
        "tokyo-night",
        "Tokyo Night",
        p!(
            true, "#1a1b26", "#24283b", "#16161e", "#c0caf5", "#9aa5ce", "#7aa2f7", "#16161e", "#9ece6a", "#e0af68",
            "#f7768e"
        ),
    ),
    t(
        "material",
        "Material Palenight",
        p!(
            true, "#292d3e", "#32374d", "#242837", "#c8cde8", "#8f97c0", "#c792ea", "#1e2130", "#c3e88d", "#ffcb6b",
            "#f07178"
        ),
    ),
    t(
        "material-ocean",
        "Material Ocean",
        p!(
            true, "#0f111a", "#181a25", "#0b0d14", "#c5cbe3", "#7f88ab", "#84ffff", "#0f111a", "#c3e88d", "#ffcb6b",
            "#f07178"
        ),
    ),
    t(
        "one-dark",
        "One Dark",
        p!(
            true, "#282c34", "#2f343e", "#21252b", "#d7dae0", "#9da5b4", "#61afef", "#1e2127", "#98c379", "#e5c07b",
            "#e06c75"
        ),
    ),
    t(
        "rose-pine",
        "Rosé Pine",
        p!(
            true, "#191724", "#1f1d2e", "#16141f", "#e0def4", "#908caa", "#c4a7e7", "#191724", "#9ccfd8", "#f6c177",
            "#eb6f92"
        ),
    ),
    t(
        "rose-pine-dawn",
        "Rosé Pine Dawn",
        p!(
            false, "#faf4ed", "#fffaf3", "#f2e9e1", "#575279", "#797593", "#907aa9", "#fffaf3", "#56949f", "#ea9d34",
            "#b4637a"
        ),
    ),
    t(
        "everforest",
        "Everforest",
        p!(
            true, "#2d353b", "#343f44", "#272e33", "#d3c6aa", "#9da9a0", "#a7c080", "#272e33", "#a7c080", "#dbbc7f",
            "#e67e80"
        ),
    ),
    t(
        "kanagawa",
        "Kanagawa",
        p!(
            true, "#1f1f28", "#2a2a37", "#16161d", "#dcd7ba", "#a6a69c", "#7e9cd8", "#16161d", "#98bb6c", "#e6c384",
            "#e46876"
        ),
    ),
    t(
        "solarized-dark",
        "Solarized Dark",
        p!(
            true, "#002b36", "#073642", "#00252f", "#eee8d5", "#93a1a1", "#268bd2", "#fdf6e3", "#859900", "#b58900",
            "#dc322f"
        ),
    ),
    t(
        "solarized-light",
        "Solarized Light",
        p!(
            false, "#eee8d5", "#fdf6e3", "#e4ddc8", "#073642", "#657b83", "#268bd2", "#fdf6e3", "#859900", "#b58900",
            "#dc322f"
        ),
    ),
    Theme {
        id: "glass-dark",
        name: "Glass Dark",
        glass: true,
        palette: p!(
            true, "#12141b", "#ffffff", "#0d0f15", "#f1f2f6", "#a3a8b8", "#7c9cff", "#0d0f15", "#4ade80", "#fbbf24",
            "#f87171"
        ),
    },
    Theme {
        id: "glass-light",
        name: "Glass Light",
        glass: true,
        palette: p!(
            false, "#f5f6fa", "#ffffff", "#eef0f5", "#1b1e28", "#5d6375", "#3b6cf6", "#ffffff", "#16a34a", "#d97706",
            "#dc2626"
        ),
    },
];

pub fn find(id: &str) -> &'static Theme {
    THEMES.iter().find(|t| t.id == id).unwrap_or(&THEMES[0])
}

/// Accent overrides offered in Settings (`None` keeps the theme's own).
pub static ACCENTS: &[(&str, &str)] = &[
    ("Blue", "#3b82f6"),
    ("Teal", "#14b8a6"),
    ("Green", "#22c55e"),
    ("Yellow", "#eab308"),
    ("Orange", "#f97316"),
    ("Red", "#ef4444"),
    ("Pink", "#ec4899"),
    ("Purple", "#a855f7"),
    ("Slate", "#64748b"),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: String,
    /// Hex color, or `None` for the theme's accent.
    pub accent: Option<String>,
    /// 0.5–1.0, used by glass themes.
    pub opacity: f64,
    /// Hide tools that this desktop can't run.
    pub hide_unavailable: bool,
    /// Hide tools from later releases.
    pub hide_planned: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance { theme: "system".into(), accent: None, opacity: 0.82, hide_unavailable: false, hide_planned: false }
    }
}

impl Appearance {
    pub fn load() -> Self {
        forgekit_core::settings::load("appearance").unwrap_or_default()
    }

    pub fn save(&self) {
        if let Err(e) = forgekit_core::settings::save("appearance", self) {
            eprintln!("saving appearance: {e:#}");
        }
    }
}

/// `#rrggbb` → (r, g, b).
fn rgb(hex: &str) -> (u8, u8, u8) {
    let h = hex.trim_start_matches('#');
    let c = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    (c(0), c(2), c(4))
}

fn rgba(hex: &str, a: f64) -> String {
    let (r, g, b) = rgb(hex);
    format!("rgba({r},{g},{b},{a:.3})")
}

/// Readable text color on top of `hex`.
fn contrast(hex: &str) -> &'static str {
    let (r, g, b) = rgb(hex);
    let lum = 0.2126 * r as f64 + 0.7152 * g as f64 + 0.0722 * b as f64;
    if lum > 160.0 { "#11131a" } else { "#ffffff" }
}

/// CSS for a theme, or an empty string for "System".
pub fn css(theme: &Theme, appearance: &Appearance) -> String {
    let Some(p) = theme.palette else {
        return match &appearance.accent {
            Some(a) => accent_css(a, contrast(a)),
            None => String::new(),
        };
    };
    let (accent, accent_fg) = match &appearance.accent {
        Some(a) => (a.as_str(), contrast(a)),
        None => (p.accent, p.accent_fg),
    };
    let op = appearance.opacity.clamp(0.4, 1.0);
    let (bg, sidebar, surface, headerbar) = if theme.glass {
        let tint = if p.dark { 0.07 } else { 0.55 };
        (rgba(p.bg, op), rgba(p.sidebar, op * 0.92), rgba(p.surface, tint), rgba(p.bg, op * 0.6))
    } else {
        (p.bg.to_string(), p.sidebar.to_string(), p.surface.to_string(), p.bg.to_string())
    };
    let shade = if p.dark { "rgba(0,0,0,0.36)" } else { "rgba(0,0,0,0.07)" };
    format!(
        ":root {{
  --window-bg-color: {bg};
  --window-fg-color: {fg};
  --view-bg-color: {surface};
  --view-fg-color: {fg};
  --headerbar-bg-color: {headerbar};
  --headerbar-fg-color: {fg};
  --headerbar-backdrop-color: {headerbar};
  --headerbar-shade-color: {shade};
  --sidebar-bg-color: {sidebar};
  --sidebar-fg-color: {fg};
  --sidebar-backdrop-color: {sidebar};
  --sidebar-shade-color: {shade};
  --secondary-sidebar-bg-color: {sidebar};
  --card-bg-color: {surface};
  --card-fg-color: {fg};
  --card-shade-color: {shade};
  --dialog-bg-color: {dialog};
  --dialog-fg-color: {fg};
  --popover-bg-color: {dialog};
  --popover-fg-color: {fg};
  --thumbnail-bg-color: {surface};
  --shade-color: {shade};
  --success-bg-color: {ok}; --success-fg-color: {ok_fg}; --success-color: {ok};
  --warning-bg-color: {warn}; --warning-fg-color: {warn_fg}; --warning-color: {warn};
  --error-bg-color: {err}; --error-fg-color: {err_fg}; --error-color: {err};
  --destructive-bg-color: {err}; --destructive-fg-color: {err_fg}; --destructive-color: {err};
  --fk-muted: {muted};
}}
{accent_css}",
        fg = p.fg,
        muted = p.muted,
        dialog = if theme.glass { rgba(p.bg, 0.96) } else { p.surface.to_string() },
        ok = p.success,
        ok_fg = contrast(p.success),
        warn = p.warning,
        warn_fg = contrast(p.warning),
        err = p.error,
        err_fg = contrast(p.error),
        accent_css = accent_css(accent, accent_fg),
    )
}

fn accent_css(accent: &str, accent_fg: &str) -> String {
    format!(":root {{ --accent-bg-color: {accent}; --accent-fg-color: {accent_fg}; --accent-color: {accent}; }}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_renders() {
        let a = Appearance::default();
        for t in THEMES {
            let c = css(t, &a);
            assert_eq!(c.is_empty(), t.palette.is_none(), "{}", t.id);
        }
        assert!(css(find("glass-dark"), &a).contains("rgba(18,20,27,0.820)"));
        assert_eq!(contrast("#ffffff"), "#11131a");
        assert_eq!(contrast("#1e1e2e"), "#ffffff");
        assert_eq!(find("nope").id, "system");
    }
}
