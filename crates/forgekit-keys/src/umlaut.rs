//! Umlaut's accent tables: which accented letters each language needs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// (id, display name, letter → lowercase variants)
pub type Language = (&'static str, &'static str, &'static [(char, &'static str)]);

pub const LANGUAGES: &[Language] = &[
    (
        "french",
        "French",
        &[('a', "àâæ"), ('c', "ç"), ('e', "éèêë"), ('i', "îï"), ('o', "ôœ"), ('u', "ùûü"), ('y', "ÿ")],
    ),
    ("german", "German", &[('a', "ä"), ('o', "ö"), ('s', "ß"), ('u', "ü")]),
    ("spanish", "Spanish", &[('a', "á"), ('e', "é"), ('i', "í"), ('n', "ñ"), ('o', "ó"), ('u', "úü")]),
    ("portuguese", "Portuguese", &[('a', "áàâã"), ('c', "ç"), ('e', "éê"), ('i', "í"), ('o', "óôõ"), ('u', "úü")]),
    ("italian", "Italian", &[('a', "à"), ('e', "èé"), ('i', "ìí"), ('o', "òó"), ('u', "ùú")]),
    ("nordic", "Nordic", &[('a', "åäæ"), ('d', "ð"), ('e', "é"), ('o', "øö"), ('t', "þ")]),
    (
        "polish",
        "Polish",
        &[('a', "ą"), ('c', "ć"), ('e', "ę"), ('l', "ł"), ('n', "ń"), ('o', "ó"), ('s', "ś"), ('z', "źż")],
    ),
    (
        "czech",
        "Czech & Slovak",
        &[
            ('a', "áä"),
            ('c', "č"),
            ('d', "ď"),
            ('e', "éě"),
            ('i', "í"),
            ('l', "ľĺ"),
            ('n', "ň"),
            ('o', "óô"),
            ('r', "řŕ"),
            ('s', "š"),
            ('t', "ť"),
            ('u', "úů"),
            ('y', "ý"),
            ('z', "ž"),
        ],
    ),
    ("turkish", "Turkish", &[('c', "ç"), ('g', "ğ"), ('i', "ı"), ('o', "ö"), ('s', "ş"), ('u', "ü")]),
    ("romanian", "Romanian", &[('a', "ăâ"), ('i', "î"), ('s', "ș"), ('t', "ț")]),
    ("hungarian", "Hungarian", &[('a', "á"), ('e', "é"), ('i', "í"), ('o', "óöő"), ('u', "úüű")]),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    /// How long a letter must be held before Space/arrows pick accents.
    pub delay_ms: u32,
    /// Language ids from [`LANGUAGES`]; empty means all of them.
    pub languages: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { enabled: false, delay_ms: 300, languages: Vec::new() }
    }
}

impl Settings {
    /// letter → accented variants (lowercase), merged across the chosen
    /// languages in table order without duplicates.
    pub fn variants(&self) -> BTreeMap<char, Vec<char>> {
        let mut out: BTreeMap<char, Vec<char>> = BTreeMap::new();
        for (id, _, table) in LANGUAGES {
            if !self.languages.is_empty() && !self.languages.iter().any(|l| l == id) {
                continue;
            }
            for (letter, chars) in *table {
                let v = out.entry(*letter).or_default();
                for c in chars.chars() {
                    if !v.contains(&c) {
                        v.push(c);
                    }
                }
            }
        }
        out
    }
}

/// Uppercase of a single character, keeping it a single character (ß → ẞ).
/// Dotless ı pairs with dotted İ: plain I is already on the key, İ is what Turkish needs.
pub fn upper(c: char) -> char {
    match c {
        'ß' => 'ẞ',
        'ı' => 'İ',
        _ => {
            let mut u = c.to_uppercase();
            match (u.next(), u.next()) {
                (Some(x), None) => x,
                _ => c,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_variants() {
        let all = Settings::default().variants();
        assert_eq!(all[&'e'][..4], ['é', 'è', 'ê', 'ë']);
        assert!(all[&'a'].contains(&'å') && all[&'a'].contains(&'ã'));
        let de = Settings { languages: vec!["german".into()], ..Default::default() }.variants();
        assert_eq!(de.keys().collect::<String>(), "aosu");
        assert_eq!(upper('é'), 'É');
        assert_eq!(upper('ß'), 'ẞ');
        // No language lists a letter twice.
        for (_, _, table) in LANGUAGES {
            let mut letters: Vec<char> = table.iter().map(|(l, _)| *l).collect();
            letters.dedup();
            assert_eq!(letters.len(), table.len());
        }
    }
}
