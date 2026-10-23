//! Keysmith and Umlaut pages, and the shared helper status card.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use adw::prelude::*;
use forgekit_core::helper::{self, HelperConfig};
use forgekit_keys::keys::{self, Mod};
use forgekit_keys::rules::{Combo, Kind, Rule, Target};
use forgekit_keys::umlaut::LANGUAGES;
use gtk::{gdk, glib};

use super::boxed_list;
use crate::util;
use crate::widgets::{self as w, hbox, vbox};
use crate::window::App;

type Cards = Vec<gtk::Widget>;

/// Save, and push to the helper when it runs (it may ask for a password).
fn apply(app: &Rc<App>, cfg: HelperConfig, page: &'static str) {
    let a = app.clone();
    util::background(
        move || {
            if helper::status().is_some() {
                helper::push(&cfg).map(|_| true).map_err(|e| format!("{e:#}"))
            } else {
                helper::save(&cfg).map(|_| false).map_err(|e| format!("{e:#}"))
            }
        },
        move |r| {
            match r {
                Ok(true) => a.toast("Applied"),
                Ok(false) => a.toast("Saved. It applies once the helper is installed."),
                Err(e) => a.toast(&e),
            }
            a.rebuild_page(page);
        },
    );
}

// ------------------------------------------------------------------ helper card

fn helper_card(app: &Rc<App>) -> gtk::Widget {
    let status = helper::status();
    let card = hbox(14);
    card.add_css_class("fk-card");
    let text = vbox(6);
    text.set_hexpand(true);
    let actions = hbox(8);
    actions.set_valign(gtk::Align::Center);

    let run = |label: &str, icon: &str, style: &str, work: fn() -> anyhow::Result<()>, done_msg: &'static str| {
        let b = w::button(label, Some(icon), style);
        let a = app.clone();
        b.connect_clicked(move |b| {
            b.set_sensitive(false);
            let (a, b) = (a.clone(), b.clone());
            util::background(
                move || work().map_err(|e| format!("{e:#}")),
                move |r| {
                    b.set_sensitive(true);
                    match r {
                        Ok(()) => {
                            a.toast(done_msg);
                            a.rebuild_page("keysmith");
                            a.rebuild_page("umlaut");
                            a.tool_toggled();
                        }
                        Err(e) => a.toast(&e),
                    }
                },
            );
        });
        b
    };
    fn install_and_apply() -> anyhow::Result<()> {
        helper::install(false)?;
        helper::push(&helper::load())
    }

    match status {
        None if !app.caps.helper_installed => {
            card.append(&w::icon_tile("shield-check", false, ""));
            text.append(&w::label("Needs a small system helper", &["fk-card-title"]));
            text.append(&w::label(
                "On Wayland only a system service can read and remap the keyboard. Forgekit's helper is tiny and sandboxed:",
                &["dim"],
            ));
            for line in [
                "It never records or sends what you type.",
                "It only touches the keyboard while a rule or Umlaut is on.",
                "Ctrl+Alt+Shift+Esc always gives the keyboard back.",
            ] {
                let r = hbox(8);
                let i = w::icon("check", 14);
                i.add_css_class("success");
                r.append(&i);
                r.append(&w::label(line, &[]));
                text.append(&r);
            }
            actions.append(&run(
                "Install helper",
                "download",
                "suggested-action",
                install_and_apply,
                "Helper installed",
            ));
        }
        None => {
            card.append(&w::icon_tile("alert-triangle", false, "warn"));
            text.append(&w::label("The helper is installed but not running", &["fk-card-title"]));
            text.append(&w::label("Reinstalling restarts it.", &["dim"]));
            actions.append(&run("Reinstall", "refresh", "suggested-action", install_and_apply, "Helper restarted"));
        }
        Some(s) if s.paused => {
            card.append(&w::icon_tile("player-pause", false, "warn"));
            text.append(&w::label("Paused by Ctrl+Alt+Shift+Esc", &["fk-card-title"]));
            text.append(&w::label("Your keyboard works normally until you resume.", &["dim"]));
            actions.append(&run("Resume", "player-play", "suggested-action", helper::resume, "Remapping resumed"));
        }
        Some(s) => {
            let (icon, tone, title, sub) = if s.active {
                ("circle-check", "ok", "Helper running", format!("Remapping: {}", s.keyboards.join(", ")))
            } else {
                (
                    "circle-check",
                    "ok",
                    "Helper running, idle",
                    "No keyboard is touched until you switch something on.".to_string(),
                )
            };
            card.append(&w::icon_tile(icon, false, tone));
            text.append(&w::label(title, &["fk-card-title"]));
            text.append(&w::label(&sub, &["dim"]));
            if let Some(e) = s.error {
                text.append(&w::label(&e, &["warning"]));
            }
            fn remove() -> anyhow::Result<()> {
                helper::install(true)
            }
            let b = run("Remove", "trash", "flat", remove, "Helper removed");
            actions.append(&b);
        }
    }
    card.append(&text);
    card.append(&actions);
    card.upcast()
}

// ------------------------------------------------------------------ key capture

fn combo_label(c: &Combo) -> gtk::Box {
    let b = hbox(4);
    for m in &c.mods {
        b.append(&gtk::Label::builder().label(m.label()).css_classes(["fk-kbd"]).build());
    }
    b.append(&gtk::Label::builder().label(keys::name(c.key)).css_classes(["fk-kbd"]).build());
    b
}

/// An entry row that also records a shortcut when its button is pressed.
fn combo_row(title: &str) -> (adw::EntryRow, gtk::Button) {
    let row = adw::EntryRow::builder().title(title).build();
    let rec = w::button("Record", Some("keyboard"), "flat");
    row.add_suffix(&rec);
    let keyc = gtk::EventControllerKey::new();
    let recording = Rc::new(RefCell::new(false));
    {
        let (recording, rec2, row2) = (recording.clone(), rec.clone(), row.clone());
        rec.connect_clicked(move |b| {
            *recording.borrow_mut() = true;
            b.set_label("Press keys…");
            b.grab_focus();
            let (recording, rec, row) = (recording.clone(), rec2.clone(), row2.clone());
            // Give up after 6 s.
            glib::timeout_add_local_once(std::time::Duration::from_secs(6), move || {
                if std::mem::take(&mut *recording.borrow_mut()) {
                    reset_button(&rec);
                    row.grab_focus();
                }
            });
        });
    }
    let held: Rc<RefCell<BTreeSet<u16>>> = Rc::default();
    {
        let (recording, held, row, rec) = (recording.clone(), held.clone(), row.clone(), rec.clone());
        keyc.connect_key_pressed(move |_, _, keycode, state| {
            if !*recording.borrow() {
                return glib::Propagation::Proceed;
            }
            let code = (keycode as u16).saturating_sub(8);
            if Mod::of(code).is_some() {
                held.borrow_mut().insert(code);
                return glib::Propagation::Stop;
            }
            let mut mods = BTreeSet::new();
            for (flag, m) in [
                (gdk::ModifierType::CONTROL_MASK, Mod::Ctrl),
                (gdk::ModifierType::ALT_MASK, Mod::Alt),
                (gdk::ModifierType::SHIFT_MASK, Mod::Shift),
                (gdk::ModifierType::SUPER_MASK, Mod::Meta),
            ] {
                if state.contains(flag) {
                    mods.insert(m);
                }
            }
            finish(&row, &rec, &recording, &held, Combo { mods, key: code });
            glib::Propagation::Stop
        });
    }
    {
        // A modifier pressed and released on its own records that key (e.g. RightCtrl).
        let (recording, held, row, rec) = (recording.clone(), held.clone(), row.clone(), rec.clone());
        keyc.connect_key_released(move |_, _, keycode, _| {
            let code = (keycode as u16).saturating_sub(8);
            if *recording.borrow() && held.borrow().len() == 1 && held.borrow().contains(&code) {
                finish(&row, &rec, &recording, &held, Combo::key(code));
            }
        });
    }
    rec.add_controller(keyc);
    (row, rec)
}

fn finish(row: &adw::EntryRow, rec: &gtk::Button, recording: &RefCell<bool>, held: &RefCell<BTreeSet<u16>>, c: Combo) {
    *recording.borrow_mut() = false;
    held.borrow_mut().clear();
    row.set_text(&c.to_string());
    reset_button(rec);
}

fn reset_button(b: &gtk::Button) {
    let c = adw::ButtonContent::builder().label("Record").icon_name(w::icon_name("keyboard")).build();
    b.set_child(Some(&c));
}

// ------------------------------------------------------------------ Keysmith

pub fn keysmith(app: &Rc<App>) -> Cards {
    let cfg = Rc::new(RefCell::new(helper::load()));
    let mut cards: Cards = vec![helper_card(app)];

    let on = gtk::Switch::builder().active(cfg.borrow().keysmith_enabled).valign(gtk::Align::Center).build();
    let (rules_card, body) =
        w::card_with_action("Rules", "Changes apply right away (the helper may ask for your password once).", &on);
    {
        let (a, cfg) = (app.clone(), cfg.clone());
        on.connect_active_notify(move |s| {
            cfg.borrow_mut().keysmith_enabled = s.is_active();
            apply(&a, cfg.borrow().clone(), "keysmith");
        });
    }
    let rules = cfg.borrow().rules.clone();
    if rules.is_empty() {
        body.append(&w::empty_state("keyboard", "No rules yet", "Add one below, or start from a common remap."));
    } else {
        let table = w::Table::new(&["When you press", "You get", "Type", ""], &[-1, -1, 110, 40]);
        for (i, r) in rules.iter().enumerate() {
            let to: gtk::Widget = match &r.to {
                Target::Keys(c) => combo_label(c).upcast(),
                Target::Disabled => w::cell("nothing (disabled)", &["dim"]),
            };
            let (kind, tone) = match r.kind() {
                Kind::Remap => ("Key", "soon"),
                Kind::Shortcut => ("Shortcut", "ok"),
                Kind::Disable => ("Off", "na"),
            };
            let del = w::icon_button("trash", "Remove rule");
            let (a, cfg) = (app.clone(), cfg.clone());
            del.connect_clicked(move |_| {
                cfg.borrow_mut().rules.remove(i);
                apply(&a, cfg.borrow().clone(), "keysmith");
            });
            table.add(vec![combo_label(&r.from).upcast(), to, w::pill(kind, tone).upcast(), del.upcast()]);
        }
        body.append(&table.root);
    }
    cards.push(rules_card.upcast());

    // ---- add
    let add_btn = w::button("Add rule", Some("plus"), "suggested-action");
    let (add, add_body) = w::card_with_action(
        "Add a rule",
        "Type keys like CapsLock or Ctrl+Alt+T, or press Record and hit them. Leave “You get” empty to switch a key off.",
        &add_btn,
    );
    let rows = boxed_list();
    let (from_row, _) = combo_row("When you press");
    let (to_row, _) = combo_row("You get");
    rows.append(&from_row);
    rows.append(&to_row);
    add_body.append(&rows);
    {
        let (a, cfg) = (app.clone(), cfg.clone());
        add_btn.connect_clicked(move |_| {
            let to_text = to_row.text().to_string();
            let to = if to_text.trim().is_empty() { "none".to_string() } else { to_text };
            match Rule::parse(&from_row.text(), &to) {
                Ok(r) => {
                    let mut c = cfg.borrow_mut();
                    c.rules.retain(|x| x.from != r.from);
                    c.rules.push(r);
                    c.keysmith_enabled = true;
                    let snapshot = c.clone();
                    drop(c);
                    apply(&a, snapshot, "keysmith");
                }
                Err(e) => a.toast(&e),
            }
        });
    }
    cards.push(add.upcast());

    // ---- presets
    let (presets, p_body) = w::card("Common remaps", "One click to add");
    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(3)
        .min_children_per_line(1)
        .row_spacing(8)
        .column_spacing(8)
        .homogeneous(true)
        .build();
    for (title, sub, from, to) in [
        ("Caps Lock → Ctrl", "Popular with Emacs and terminal users", "capslock", "leftctrl"),
        ("Caps Lock → Esc", "Popular with Vim users", "capslock", "esc"),
        ("Disable Insert", "No more accidental overwrite mode", "insert", "none"),
        ("Super+Enter → terminal", "Sends Ctrl+Alt+T (GNOME's terminal shortcut)", "super+enter", "ctrl+alt+t"),
        ("Right Alt → Super", "For keyboards without a Super key", "rightalt", "leftmeta"),
        ("Disable Caps Lock", "Never type in capitals by accident", "capslock", "none"),
    ] {
        let b = vbox(2);
        b.append(&w::label(title, &["fk-tile-title"]));
        b.append(&w::label(sub, &["fk-tile-summary"]));
        let btn = gtk::Button::builder().child(&b).css_classes(["fk-quick"]).build();
        let (a, cfg) = (app.clone(), cfg.clone());
        btn.connect_clicked(move |_| {
            let r = Rule::parse(from, to).expect("preset rules are valid");
            let mut c = cfg.borrow_mut();
            c.rules.retain(|x| x.from != r.from);
            c.rules.push(r);
            c.keysmith_enabled = true;
            let snapshot = c.clone();
            drop(c);
            apply(&a, snapshot, "keysmith");
        });
        flow.append(&btn);
    }
    p_body.append(&flow);
    cards.push(presets.upcast());
    cards.push(safety_card());
    cards
}

fn safety_card() -> gtk::Widget {
    let c = hbox(14);
    c.add_css_class("fk-card");
    c.append(&w::icon_tile("lock", false, "off"));
    let t = vbox(4);
    t.append(&w::label("If your keyboard ever acts up", &["fk-card-title"]));
    let keysrow = hbox(4);
    keysrow.append(&w::label("Press", &["dim"]));
    for k in ["Ctrl", "Alt", "Shift", "Esc"] {
        keysrow.append(&gtk::Label::builder().label(k).css_classes(["fk-kbd"]).build());
    }
    keysrow.append(&w::label("together: every remap stops at once.", &["dim"]));
    t.append(&keysrow);
    c.append(&t);
    c.upcast()
}

// ------------------------------------------------------------------ Umlaut

pub fn umlaut(app: &Rc<App>) -> Cards {
    let cfg = Rc::new(RefCell::new(helper::load()));
    let mut cards: Cards = vec![helper_card(app)];

    // ---- how it works + switch
    let hero = hbox(18);
    hero.add_css_class("fk-card");
    let demo = vbox(6);
    demo.set_hexpand(true);
    demo.append(&w::label("Hold a letter, then tap Space", &["fk-card-title"]));
    let steps = hbox(6);
    for (k, after) in [("e", "hold"), ("Space", "é"), ("→", "è"), ("→", "ê"), ("←", "è")] {
        steps.append(&gtk::Label::builder().label(k).css_classes(["fk-kbd"]).build());
        steps.append(&w::label(after, &["dim"]));
    }
    demo.append(&steps);
    demo.append(&w::label("Release the letter to keep the accent. Hold Shift too for capitals (É).", &["dim"]));
    hero.append(&demo);
    let on = gtk::Switch::builder().active(cfg.borrow().umlaut.enabled).valign(gtk::Align::Center).build();
    hero.append(&on);
    {
        let (a, cfg) = (app.clone(), cfg.clone());
        on.connect_active_notify(move |s| {
            cfg.borrow_mut().umlaut.enabled = s.is_active();
            apply(&a, cfg.borrow().clone(), "umlaut");
        });
    }
    cards.push(hero.upcast());

    // ---- languages
    let save_langs = w::button("Apply", Some("check"), "suggested-action");
    let (langs, l_body) = w::card_with_action(
        "Languages",
        "Only the accents you need show up when cycling. None selected = all.",
        &save_langs,
    );
    let chips = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(6)
        .min_children_per_line(2)
        .row_spacing(6)
        .column_spacing(6)
        .build();
    let checks: Rc<RefCell<Vec<(&'static str, gtk::ToggleButton)>>> = Rc::default();
    let selected = cfg.borrow().umlaut.languages.clone();
    for (id, name, _) in LANGUAGES {
        let t = gtk::ToggleButton::builder()
            .label(*name)
            .active(selected.iter().any(|s| s == id))
            .css_classes(["pill", "fk-chip-toggle"])
            .build();
        chips.append(&t);
        checks.borrow_mut().push((id, t));
    }
    l_body.append(&chips);
    let rows = boxed_list();
    let delay = adw::SpinRow::with_range(100.0, 2000.0, 50.0);
    delay.set_title("Hold for");
    delay.set_subtitle("milliseconds before Space/arrows pick accents");
    delay.set_value(cfg.borrow().umlaut.delay_ms as f64);
    rows.append(&delay);
    l_body.append(&rows);

    // Live preview of what each letter cycles through.
    let preview = w::Table::new(&["Letter", "Accents"], &[90, -1]);
    let preview = Rc::new(preview);
    let refresh: Rc<dyn Fn()> = {
        let (checks, preview, cfg) = (checks.clone(), preview.clone(), cfg.clone());
        Rc::new(move || {
            let mut s = cfg.borrow().umlaut.clone();
            s.languages = checks.borrow().iter().filter(|(_, t)| t.is_active()).map(|(id, _)| id.to_string()).collect();
            preview.clear();
            for (letter, v) in s.variants() {
                let accents: String = v.iter().map(|c| format!("{c}  ")).collect();
                preview.add(vec![w::cell(&letter.to_string(), &["fk-kbd"]), w::cell(&accents, &["title-4"])]);
            }
        })
    };
    refresh();
    for (_, t) in checks.borrow().iter() {
        let r = refresh.clone();
        t.connect_toggled(move |_| r());
    }
    l_body.append(&preview.root);
    {
        let (a, cfg) = (app.clone(), cfg.clone());
        save_langs.connect_clicked(move |_| {
            {
                let mut c = cfg.borrow_mut();
                c.umlaut.languages =
                    checks.borrow().iter().filter(|(_, t)| t.is_active()).map(|(id, _)| id.to_string()).collect();
                c.umlaut.delay_ms = delay.value() as u32;
            }
            apply(&a, cfg.borrow().clone(), "umlaut");
        });
    }
    cards.push(langs.upcast());

    // ---- try it
    let (try_card, t_body) = w::card(
        "Try it here",
        "Accents are typed with Ctrl+Shift+U, the Unicode entry from IBus (on by default in GNOME).",
    );
    t_body.append(&gtk::Entry::builder().placeholder_text("Hold e, then tap Space…").build());
    if !app.caps.ibus {
        t_body.append(&w::label(
            "IBus doesn't seem to be running, so accents may not appear in some apps.",
            &["warning"],
        ));
    }
    cards.push(try_card.upcast());
    cards.push(safety_card());
    cards
}
