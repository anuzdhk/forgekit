//! The remapping engine: physical key events in, virtual key events out.
//!
//! It has no I/O and no clock of its own (callers pass the time), so every
//! behavior is unit-tested. The helper feeds it events from grabbed keyboards
//! and writes its output to a uinput device.
//!
//! Order of handling for each event:
//! 1. The emergency chord Ctrl+Alt+Shift+Esc releases everything ([`Out::Panic`]).
//! 2. Keys whose press we consumed stay consumed until released.
//! 3. Umlaut: a held letter plus Space/→/← cycles accented variants in place.
//! 4. Keysmith: disabled keys, single-key remaps, then shortcuts.
//! 5. Everything else passes through unchanged.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::keys::{self, Mod};
use crate::rules::{Combo, Kind, Target};
use crate::{HelperConfig, umlaut};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Out {
    /// Write this key event (value 0 = up, 1 = down, 2 = repeat).
    Key(u16, i32),
    /// The emergency chord: stop remapping and release the keyboards.
    Panic,
}

struct Held {
    letter: u16,
    since_ms: u64,
    upper: bool,
    variants: Vec<char>,
    /// Which variant is on screen; `None` = still the plain letter.
    index: Option<usize>,
}

pub struct Engine {
    remap: HashMap<u16, u16>,
    disabled: BTreeSet<u16>,
    shortcuts: Vec<(Combo, Option<Combo>)>,
    accents: BTreeMap<u16, Vec<char>>,
    delay_ms: u64,
    /// Physical keys currently down.
    phys: BTreeSet<u16>,
    /// Keys we report as down on the virtual keyboard.
    virt: BTreeSet<u16>,
    /// Physical keys whose press was consumed; their repeats/release are dropped.
    consumed: BTreeSet<u16>,
    held: Option<Held>,
}

impl Engine {
    pub fn new(cfg: &HelperConfig) -> Self {
        let mut e = Engine {
            remap: HashMap::new(),
            disabled: BTreeSet::new(),
            shortcuts: Vec::new(),
            accents: BTreeMap::new(),
            delay_ms: cfg.umlaut.delay_ms as u64,
            phys: BTreeSet::new(),
            virt: BTreeSet::new(),
            consumed: BTreeSet::new(),
            held: None,
        };
        if cfg.keysmith_enabled {
            for r in &cfg.rules {
                if r.validate().is_err() {
                    continue;
                }
                match (r.kind(), &r.to) {
                    (Kind::Remap, Target::Keys(to)) => {
                        e.remap.insert(r.from.key, to.key);
                    }
                    (Kind::Disable, _) if r.from.mods.is_empty() => {
                        e.disabled.insert(r.from.key);
                    }
                    (_, Target::Keys(to)) => e.shortcuts.push((r.from.clone(), Some(to.clone()))),
                    (_, Target::Disabled) => e.shortcuts.push((r.from.clone(), None)),
                }
            }
        }
        if cfg.umlaut.enabled {
            for (letter, variants) in cfg.umlaut.variants() {
                if let Some(code) = keys::letter(letter) {
                    e.accents.insert(code, variants);
                }
            }
        }
        e
    }

    /// Whether this config needs the keyboards grabbed at all.
    pub fn is_active(cfg: &HelperConfig) -> bool {
        (cfg.keysmith_enabled && !cfg.rules.is_empty()) || cfg.umlaut.enabled
    }

    pub fn handle(&mut self, code: u16, value: i32, now_ms: u64) -> Vec<Out> {
        let mut out = Vec::new();
        match value {
            1 => {
                self.phys.insert(code);
            }
            0 => {
                self.phys.remove(&code);
            }
            _ => {}
        }

        // 1. Emergency release.
        if value == 1 && code == keys::ESC && [Mod::Ctrl, Mod::Alt, Mod::Shift].iter().all(|m| self.mod_down(*m)) {
            out.extend(self.release_all());
            out.push(Out::Panic);
            return out;
        }

        // 2. Consumed presses.
        if self.consumed.contains(&code) {
            if value == 0 {
                self.consumed.remove(&code);
            }
            return out;
        }

        // 3. Umlaut.
        if let Some(true) = self.umlaut(code, value, now_ms, &mut out) {
            return out;
        }

        // 4a. Disabled keys and single-key remaps.
        if self.disabled.contains(&code) {
            if value == 1 {
                self.consumed.insert(code);
            }
            return out;
        }
        let mapped = self.remap.get(&code).copied().unwrap_or(code);

        // 4b. Shortcuts, matched against the modifiers the system currently sees.
        if value == 1 {
            let mods = self.virt_mods();
            if let Some((_, target)) =
                self.shortcuts.iter().find(|(from, _)| from.key == mapped && from.mods == mods).cloned()
            {
                self.consumed.insert(code);
                if let Some(to) = target {
                    self.send_combo(&to, &mut out);
                }
                return out;
            }
        }

        // 5. Pass through.
        self.emit(mapped, value, &mut out);
        out
    }

    /// Release every key we hold down on the virtual keyboard.
    pub fn release_all(&mut self) -> Vec<Out> {
        let mut out: Vec<Out> = self.virt.iter().map(|&k| Out::Key(k, 0)).collect();
        out.dedup();
        self.virt.clear();
        self.consumed.clear();
        self.held = None;
        out
    }

    /// Returns `Some(true)` when the event was fully handled.
    fn umlaut(&mut self, code: u16, value: i32, now: u64, out: &mut Vec<Out>) -> Option<bool> {
        if self.accents.is_empty() {
            return None;
        }
        if let Some(h) = &mut self.held {
            if code == h.letter {
                // We already typed the letter as a tap; drop its repeats and release.
                if value == 0 {
                    self.held = None;
                }
                return Some(true);
            }
            let step = match code {
                keys::SPACE | keys::RIGHT => Some(1i64),
                keys::LEFT => Some(-1),
                _ => None,
            };
            if let (Some(step), 1) = (step, value)
                && now.saturating_sub(h.since_ms) >= self.delay_ms
            {
                let n = h.variants.len() as i64;
                let next = match h.index {
                    None if step > 0 => 0,
                    None => n - 1,
                    Some(i) => (i as i64 + step).rem_euclid(n),
                };
                h.index = Some(next as usize);
                let mut c = h.variants[next as usize];
                if h.upper {
                    c = umlaut::upper(c);
                }
                self.consumed.insert(code);
                self.replace_previous_char(c, out);
                return Some(true);
            }
            // Any other key ends the accent session; the letter stays typed.
            if value == 1 {
                self.held = None;
            }
            return None;
        }
        let plain = !self.mod_down(Mod::Ctrl) && !self.mod_down(Mod::Alt) && !self.mod_down(Mod::Meta);
        if value == 1 && plain && !self.remap.contains_key(&code) && !self.disabled.contains(&code) {
            let variants = self.accents.get(&code)?.clone();
            self.held =
                Some(Held { letter: code, since_ms: now, upper: self.mod_down(Mod::Shift), variants, index: None });
            // Tap instead of holding, so the desktop doesn't auto-repeat the letter.
            self.tap(code, out);
            return Some(true);
        }
        None
    }

    /// Backspace, then type `c` with the Ctrl+Shift+U unicode entry (IBus/GTK).
    fn replace_previous_char(&mut self, c: char, out: &mut Vec<Out>) {
        let shifts: Vec<u16> =
            [keys::LEFTSHIFT, keys::RIGHTSHIFT].into_iter().filter(|k| self.virt.contains(k)).collect();
        for &s in &shifts {
            self.emit(s, 0, out);
        }
        self.tap(keys::BACKSPACE, out);
        self.emit(keys::LEFTCTRL, 1, out);
        self.emit(keys::LEFTSHIFT, 1, out);
        self.tap(keys::U, out);
        self.emit(keys::LEFTSHIFT, 0, out);
        self.emit(keys::LEFTCTRL, 0, out);
        for d in format!("{:x}", c as u32).chars() {
            self.tap(keys::hex_digit(d.to_digit(16).unwrap()), out);
        }
        self.tap(keys::SPACE, out);
        for &s in &shifts {
            self.emit(s, 1, out);
        }
    }

    /// Press a combo as a quick chord, temporarily lifting modifiers the user
    /// is holding that the target doesn't want.
    fn send_combo(&mut self, to: &Combo, out: &mut Vec<Out>) {
        let lifted: Vec<u16> =
            self.virt.iter().copied().filter(|&k| Mod::of(k).is_some_and(|m| !to.mods.contains(&m))).collect();
        for &k in &lifted {
            self.emit(k, 0, out);
        }
        let mut pressed = Vec::new();
        for m in &to.mods {
            if !self.virt_mods().contains(m) {
                self.emit(m.key(), 1, out);
                pressed.push(m.key());
            }
        }
        self.tap(to.key, out);
        for &k in pressed.iter().rev() {
            self.emit(k, 0, out);
        }
        for &k in &lifted {
            self.emit(k, 1, out);
        }
    }

    fn tap(&mut self, code: u16, out: &mut Vec<Out>) {
        self.emit(code, 1, out);
        self.emit(code, 0, out);
    }

    fn emit(&mut self, code: u16, value: i32, out: &mut Vec<Out>) {
        match value {
            1 => {
                self.virt.insert(code);
            }
            0 => {
                self.virt.remove(&code);
            }
            _ => {}
        }
        out.push(Out::Key(code, value));
    }

    /// A modifier is down if a physical key for it is, or a key remapped to it is.
    fn mod_down(&self, m: Mod) -> bool {
        self.phys.iter().any(|&k| Mod::of(k) == Some(m)) || self.virt.iter().any(|&k| Mod::of(k) == Some(m))
    }

    fn virt_mods(&self) -> BTreeSet<Mod> {
        self.virt.iter().filter_map(|&k| Mod::of(k)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::*;
    use crate::rules::Rule;
    use crate::umlaut::Settings as Umlaut;

    const E: u16 = 18;
    const H: u16 = 35;
    const T: u16 = 20;
    const A: u16 = 30;

    fn keys_only(v: Vec<Out>) -> Vec<(u16, i32)> {
        v.into_iter()
            .filter_map(|o| match o {
                Out::Key(k, v) => Some((k, v)),
                Out::Panic => None,
            })
            .collect()
    }

    fn engine(rules: &[(&str, &str)], umlaut: bool) -> Engine {
        let cfg = HelperConfig {
            keysmith_enabled: true,
            rules: rules.iter().map(|(f, t)| Rule::parse(f, t).unwrap()).collect(),
            umlaut: Umlaut { enabled: umlaut, delay_ms: 300, languages: vec!["french".into()] },
        };
        Engine::new(&cfg)
    }

    #[test]
    fn passthrough_and_remap_while_held() {
        let mut e = engine(&[("capslock", "leftctrl")], false);
        assert_eq!(keys_only(e.handle(A, 1, 0)), [(A, 1)]);
        assert_eq!(keys_only(e.handle(A, 0, 0)), [(A, 0)]);
        assert_eq!(keys_only(e.handle(CAPSLOCK, 1, 0)), [(LEFTCTRL, 1)]);
        assert_eq!(keys_only(e.handle(CAPSLOCK, 2, 0)), [(LEFTCTRL, 2)]);
        assert_eq!(keys_only(e.handle(CAPSLOCK, 0, 0)), [(LEFTCTRL, 0)]);
    }

    #[test]
    fn shortcut_lifts_and_restores_modifiers() {
        let mut e = engine(&[("ctrl+h", "backspace")], false);
        e.handle(LEFTCTRL, 1, 0);
        let out = keys_only(e.handle(H, 1, 0));
        assert_eq!(out, [(LEFTCTRL, 0), (BACKSPACE, 1), (BACKSPACE, 0), (LEFTCTRL, 1)]);
        // The trigger's repeat and release are swallowed.
        assert!(e.handle(H, 2, 0).is_empty());
        assert!(e.handle(H, 0, 0).is_empty());
        // Without Ctrl, H is just H.
        e.handle(LEFTCTRL, 0, 0);
        assert_eq!(keys_only(e.handle(H, 1, 0)), [(H, 1)]);
    }

    #[test]
    fn shortcut_adds_modifiers_and_works_with_remapped_ctrl() {
        let mut e = engine(&[("capslock", "ctrl"), ("ctrl+t", "super+enter")], false);
        e.handle(CAPSLOCK, 1, 0);
        let out = keys_only(e.handle(T, 1, 0));
        assert_eq!(out, [(LEFTCTRL, 0), (LEFTMETA, 1), (ENTER, 1), (ENTER, 0), (LEFTMETA, 0), (LEFTCTRL, 1)]);
    }

    #[test]
    fn disabled_keys_and_shortcuts() {
        let mut e = engine(&[("insert", "none"), ("ctrl+q", "none")], false);
        assert!(e.handle(110, 1, 0).is_empty());
        assert!(e.handle(110, 0, 0).is_empty());
        e.handle(LEFTCTRL, 1, 0);
        assert!(e.handle(16, 1, 0).is_empty());
    }

    #[test]
    fn umlaut_cycles_in_place() {
        let mut e = engine(&[], true);
        // The letter is tapped, not held, so the desktop can't auto-repeat it.
        assert_eq!(keys_only(e.handle(E, 1, 0)), [(E, 1), (E, 0)]);
        assert!(e.handle(E, 2, 100).is_empty());
        // Space after the delay: Backspace, then Ctrl+Shift+U e9 Space → é.
        let out = keys_only(e.handle(SPACE, 1, 400));
        let mut want = vec![
            (BACKSPACE, 1),
            (BACKSPACE, 0),
            (LEFTCTRL, 1),
            (LEFTSHIFT, 1),
            (U, 1),
            (U, 0),
            (LEFTSHIFT, 0),
            (LEFTCTRL, 0),
        ];
        want.extend([(18, 1), (18, 0), (10, 1), (10, 0), (SPACE, 1), (SPACE, 0)]);
        assert_eq!(out, want);
        assert!(e.handle(SPACE, 0, 450).is_empty());
        // Next: è (e8); Left goes back to é.
        let out = keys_only(e.handle(RIGHT, 1, 500));
        assert!(out.contains(&(9, 1)), "{out:?}");
        e.handle(RIGHT, 0, 510);
        let out = keys_only(e.handle(LEFT, 1, 600));
        assert!(out.contains(&(10, 1)), "{out:?}");
        e.handle(LEFT, 0, 610);
        assert!(e.handle(E, 0, 700).is_empty());
        // Session over: Space is a normal space again.
        assert_eq!(keys_only(e.handle(SPACE, 1, 800)), [(SPACE, 1)]);
    }

    #[test]
    fn umlaut_space_too_early_and_uppercase() {
        let mut e = engine(&[], true);
        e.handle(E, 1, 0);
        // Typing "e " quickly must give a normal space.
        assert_eq!(keys_only(e.handle(SPACE, 1, 50)), [(SPACE, 1)]);
        e.handle(SPACE, 0, 60);
        e.handle(E, 0, 70);

        // Shift+E held → É (c9); Shift is lifted while typing and pressed again after.
        e.handle(LEFTSHIFT, 1, 1000);
        e.handle(E, 1, 1000);
        let out = keys_only(e.handle(SPACE, 1, 1400));
        assert_eq!(out.first(), Some(&(LEFTSHIFT, 0)));
        assert_eq!(out.last(), Some(&(LEFTSHIFT, 1)));
        assert!(out.windows(2).any(|w| w == [(46, 1), (46, 0)]), "c of c9: {out:?}");
        // Ctrl+E is never an accent key, including Ctrl from a remapped CapsLock.
        let mut e = engine(&[], true);
        e.handle(LEFTCTRL, 1, 0);
        assert_eq!(keys_only(e.handle(E, 1, 0)), [(E, 1)]);
        let mut e = engine(&[("capslock", "ctrl")], true);
        e.handle(CAPSLOCK, 1, 0);
        assert_eq!(keys_only(e.handle(E, 1, 0)), [(E, 1)]);
    }

    #[test]
    fn panic_chord_releases_everything() {
        let mut e = engine(&[("capslock", "ctrl")], false);
        e.handle(CAPSLOCK, 1, 0);
        e.handle(LEFTALT, 1, 0);
        e.handle(LEFTSHIFT, 1, 0);
        let out = e.handle(ESC, 1, 0);
        assert_eq!(out.last(), Some(&Out::Panic));
        let released: BTreeSet<u16> = keys_only(out).into_iter().filter(|(_, v)| *v == 0).map(|(k, _)| k).collect();
        assert_eq!(released, [LEFTCTRL, LEFTALT, LEFTSHIFT].into());
    }

    #[test]
    fn inactive_configs() {
        assert!(!Engine::is_active(&HelperConfig::default()));
        let mut cfg = HelperConfig::default();
        cfg.umlaut.enabled = true;
        assert!(Engine::is_active(&cfg));
    }
}
