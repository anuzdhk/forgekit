//! Linux input key codes (from `linux/input-event-codes.h`) and their names.

pub const ESC: u16 = 1;
pub const BACKSPACE: u16 = 14;
pub const TAB: u16 = 15;
pub const ENTER: u16 = 28;
pub const LEFTCTRL: u16 = 29;
pub const LEFTSHIFT: u16 = 42;
pub const RIGHTSHIFT: u16 = 54;
pub const LEFTALT: u16 = 56;
pub const SPACE: u16 = 57;
pub const CAPSLOCK: u16 = 58;
pub const RIGHTCTRL: u16 = 97;
pub const RIGHTALT: u16 = 100;
pub const LEFT: u16 = 105;
pub const RIGHT: u16 = 106;
pub const LEFTMETA: u16 = 125;
pub const RIGHTMETA: u16 = 126;
pub const U: u16 = 22;

/// Every key Forgekit can name, as (name, code). The first name for a code is
/// the display name.
pub const NAMES: &[(&str, u16)] = &[
    ("Esc", 1),
    ("1", 2),
    ("2", 3),
    ("3", 4),
    ("4", 5),
    ("5", 6),
    ("6", 7),
    ("7", 8),
    ("8", 9),
    ("9", 10),
    ("0", 11),
    ("Minus", 12),
    ("Equal", 13),
    ("Backspace", 14),
    ("Tab", 15),
    ("Q", 16),
    ("W", 17),
    ("E", 18),
    ("R", 19),
    ("T", 20),
    ("Y", 21),
    ("U", 22),
    ("I", 23),
    ("O", 24),
    ("P", 25),
    ("LeftBrace", 26),
    ("RightBrace", 27),
    ("Enter", 28),
    ("LeftCtrl", 29),
    ("A", 30),
    ("S", 31),
    ("D", 32),
    ("F", 33),
    ("G", 34),
    ("H", 35),
    ("J", 36),
    ("K", 37),
    ("L", 38),
    ("Semicolon", 39),
    ("Apostrophe", 40),
    ("Grave", 41),
    ("LeftShift", 42),
    ("Backslash", 43),
    ("Z", 44),
    ("X", 45),
    ("C", 46),
    ("V", 47),
    ("B", 48),
    ("N", 49),
    ("M", 50),
    ("Comma", 51),
    ("Dot", 52),
    ("Slash", 53),
    ("RightShift", 54),
    ("KpAsterisk", 55),
    ("LeftAlt", 56),
    ("Space", 57),
    ("CapsLock", 58),
    ("F1", 59),
    ("F2", 60),
    ("F3", 61),
    ("F4", 62),
    ("F5", 63),
    ("F6", 64),
    ("F7", 65),
    ("F8", 66),
    ("F9", 67),
    ("F10", 68),
    ("NumLock", 69),
    ("ScrollLock", 70),
    ("Kp7", 71),
    ("Kp8", 72),
    ("Kp9", 73),
    ("KpMinus", 74),
    ("Kp4", 75),
    ("Kp5", 76),
    ("Kp6", 77),
    ("KpPlus", 78),
    ("Kp1", 79),
    ("Kp2", 80),
    ("Kp3", 81),
    ("Kp0", 82),
    ("KpDot", 83),
    ("F11", 87),
    ("F12", 88),
    ("KpEnter", 96),
    ("RightCtrl", 97),
    ("KpSlash", 98),
    ("PrintScreen", 99),
    ("RightAlt", 100),
    ("Home", 102),
    ("Up", 103),
    ("PageUp", 104),
    ("Left", 105),
    ("Right", 106),
    ("End", 107),
    ("Down", 108),
    ("PageDown", 109),
    ("Insert", 110),
    ("Delete", 111),
    ("Mute", 113),
    ("VolumeDown", 114),
    ("VolumeUp", 115),
    ("Pause", 119),
    ("LeftMeta", 125),
    ("RightMeta", 126),
    ("Menu", 127),
    ("Calculator", 140),
    ("Mail", 155),
    ("NextSong", 163),
    ("PlayPause", 164),
    ("PreviousSong", 165),
    ("StopMedia", 166),
    ("Homepage", 172),
    ("F13", 183),
    ("F14", 184),
    ("F15", 185),
    ("F16", 186),
    ("F17", 187),
    ("F18", 188),
    ("F19", 189),
    ("F20", 190),
    ("F21", 191),
    ("F22", 192),
    ("F23", 193),
    ("F24", 194),
    ("BrightnessDown", 224),
    ("BrightnessUp", 225),
    // Aliases (parsing only).
    ("Escape", 1),
    ("Return", 28),
    ("Ctrl", 29),
    ("Control", 29),
    ("Shift", 42),
    ("Alt", 56),
    ("AltGr", 100),
    ("Super", 125),
    ("Meta", 125),
    ("Win", 125),
    ("Caps", 58),
    ("Del", 111),
    ("Ins", 110),
    ("PgUp", 104),
    ("PgDn", 109),
    ("Print", 99),
    ("-", 12),
    ("=", 13),
    ("[", 26),
    ("]", 27),
    (";", 39),
    ("'", 40),
    ("`", 41),
    ("\\", 43),
    (",", 51),
    (".", 52),
    ("/", 53),
];

pub fn code(name: &str) -> Option<u16> {
    NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())).map(|&(_, c)| c)
}

/// Display name, or `Key123` for codes without one.
pub fn name(code: u16) -> String {
    NAMES.iter().find(|&&(_, c)| c == code).map(|&(n, _)| n.to_string()).unwrap_or_else(|| format!("Key{code}"))
}

/// The four modifier families. Left and right keys count as the same modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mod {
    Ctrl,
    Alt,
    Shift,
    Meta,
}

impl Mod {
    pub fn of(code: u16) -> Option<Mod> {
        match code {
            LEFTCTRL | RIGHTCTRL => Some(Mod::Ctrl),
            LEFTALT | RIGHTALT => Some(Mod::Alt),
            LEFTSHIFT | RIGHTSHIFT => Some(Mod::Shift),
            LEFTMETA | RIGHTMETA => Some(Mod::Meta),
            _ => None,
        }
    }

    /// The left-hand key, used when we have to press this modifier ourselves.
    pub fn key(self) -> u16 {
        match self {
            Mod::Ctrl => LEFTCTRL,
            Mod::Alt => LEFTALT,
            Mod::Shift => LEFTSHIFT,
            Mod::Meta => LEFTMETA,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mod::Ctrl => "Ctrl",
            Mod::Alt => "Alt",
            Mod::Shift => "Shift",
            Mod::Meta => "Super",
        }
    }
}

/// Key code of a hex digit (0-9, a-f) on a US-style layout.
pub fn hex_digit(d: u32) -> u16 {
    const DIGITS: [u16; 16] = [11, 2, 3, 4, 5, 6, 7, 8, 9, 10, 30, 48, 46, 32, 18, 33];
    DIGITS[(d & 0xf) as usize]
}

/// The letter key for an ASCII letter.
pub fn letter(c: char) -> Option<u16> {
    let name = c.to_ascii_uppercase().to_string();
    c.is_ascii_alphabetic().then(|| code(&name)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_roundtrip() {
        assert_eq!(code("capslock"), Some(CAPSLOCK));
        assert_eq!(code("Super"), Some(LEFTMETA));
        assert_eq!(name(LEFTMETA), "LeftMeta");
        assert_eq!(name(999), "Key999");
        assert_eq!(letter('e'), Some(18));
        assert_eq!(hex_digit(0xe), 18);
        assert_eq!(hex_digit(9), 10);
        assert_eq!(Mod::of(RIGHTALT), Some(Mod::Alt));
        // Every display name parses back to its own code.
        for &(n, c) in NAMES {
            if name(c) == n {
                assert_eq!(code(n), Some(c), "{n}");
            }
        }
    }
}
