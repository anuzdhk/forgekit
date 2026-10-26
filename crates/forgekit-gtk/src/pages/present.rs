//! Stage, Caliper, Beacon and Cheatsheet pages.

use std::rc::Rc;

use adw::prelude::*;
use forgekit_core::capability::Desktop;
use forgekit_core::desktop;
use forgekit_core::tools::{beacon, cheatsheet};

use super::boxed_list;
use crate::widgets::{self as w, hbox, vbox};
use crate::window::App;

type Cards = Vec<gtk::Widget>;

/// A card with a big start button and a one-line explanation.
fn launcher(
    app: &Rc<App>,
    title: &str,
    text: &str,
    button: &str,
    icon: &str,
    run: fn(&adw::Application),
) -> gtk::Widget {
    let card = hbox(16);
    card.add_css_class("fk-card");
    let t = vbox(4);
    t.set_hexpand(true);
    t.set_valign(gtk::Align::Center);
    t.append(&w::label(title, &["fk-card-title"]));
    t.append(&w::label(text, &["dim"]));
    card.append(&t);
    let b = w::button(button, Some(icon), "suggested-action");
    b.add_css_class("pill");
    let win = app.window.clone();
    b.connect_clicked(move |_| {
        if let Some(a) = win.application().and_downcast::<adw::Application>() {
            run(&a);
        }
    });
    card.append(&b);
    card.upcast()
}

/// Key reference rows: (keys, what they do).
fn keys_card(title: &str, rows: &[(&str, &str)]) -> gtk::Widget {
    let (card, body) = w::card(title, "");
    let grid = gtk::Grid::builder().column_spacing(24).row_spacing(10).build();
    for (i, (keys, what)) in rows.iter().enumerate() {
        let (col, row) = ((i % 2) as i32 * 2, (i / 2) as i32);
        grid.attach(&crate::cheatsheet::keys_widget(keys), col, row, 1, 1);
        let l = w::label(what, &["dim"]);
        l.set_hexpand(true);
        grid.attach(&l, col + 1, row, 1, 1);
    }
    body.append(&grid);
    card.upcast()
}

pub fn stage(app: &Rc<App>) -> Cards {
    vec![
        launcher(
            app,
            "Zoom in and draw",
            "Freezes your screen so you can zoom into details and mark things up while presenting.",
            "Start",
            "presentation",
            crate::stage::open,
        ),
        keys_card(
            "While Stage is open",
            &[
                ("Scroll", "Zoom where you point"),
                ("Right-drag", "Move around when zoomed"),
                ("P", "Pen"),
                ("H", "Highlighter"),
                ("L", "Line"),
                ("A", "Arrow"),
                ("R", "Rectangle"),
                ("O", "Ellipse"),
                ("1+…+6", "Colors"),
                ("[+]", "Thinner / thicker"),
                ("W", "Whiteboard"),
                ("K", "Blackboard"),
                ("E", "Erase everything"),
                ("Ctrl+Z", "Undo"),
                ("Ctrl+C", "Copy the picture"),
                ("Ctrl+S", "Save to Pictures/Forgekit"),
                ("0", "Reset zoom"),
                ("Esc", "Exit"),
            ],
        ),
    ]
}

pub fn caliper(app: &Rc<App>) -> Cards {
    vec![
        launcher(
            app,
            "Measure anything on screen",
            "Hover to see the distance between edges, or drag a box. Click to copy the size.",
            "Measure",
            "ruler-measure",
            crate::caliper::open,
        ),
        keys_card(
            "How it measures",
            &[
                ("Hover", "Distances to the nearest edges"),
                ("Drag", "Measure a box"),
                ("Click", "Copy the size (e.g. 120 × 48)"),
                ("Esc", "Exit"),
            ],
        ),
    ]
}

pub fn beacon(app: &Rc<App>) -> Cards {
    let mut cards: Cards = Vec::new();
    let kde = app.caps.desktop == Desktop::Kde;
    if !kde {
        let rows = boxed_list();
        rows.append(&super::extension_row(app, "beacon"));
        cards.push(rows.upcast());
    }
    let s = beacon::Settings::load().unwrap_or_default();
    let find = w::button("Find my pointer", Some("pointer-search"), "suggested-action");
    let (card, body) =
        w::card_with_action("Find and show the pointer", "Helps your audience follow you while presenting.", &find);
    {
        let a = app.clone();
        find.connect_clicked(move |_| {
            let Some(b) = desktop::backend(&a.caps) else { return };
            if let Err(e) = beacon::find(b) {
                a.toast(&format!("{e:#}"));
            }
        });
    }
    let rows = boxed_list();
    let clicks = adw::SwitchRow::builder()
        .title("Show clicks")
        .subtitle("A ripple where you click; right clicks in orange")
        .active(s.clicks)
        .build();
    rows.append(&clicks);
    let ctrl = adw::SwitchRow::builder()
        .title("Press Ctrl twice to find the pointer")
        .subtitle(if kde {
            "On KDE Plasma, hold Ctrl+Meta instead (KWin's Track Mouse)"
        } else {
            "Rings shrink onto the pointer"
        })
        .active(s.double_ctrl)
        .sensitive(!kde)
        .build();
    rows.append(&ctrl);
    let save = {
        let (a, clicks, ctrl) = (app.clone(), clicks.clone(), ctrl.clone());
        Rc::new(move || {
            let s = beacon::Settings { clicks: clicks.is_active(), double_ctrl: ctrl.is_active() };
            let r = s.save().and_then(|_| match desktop::backend(&a.caps) {
                Some(b) => beacon::apply(b, &s),
                None => Ok(()),
            });
            if let Err(e) = r {
                a.toast(&format!("{e:#}"));
            }
        })
    };
    for r in [&clicks, &ctrl] {
        let save = save.clone();
        r.connect_active_notify(move |_| save());
    }
    body.append(&rows);
    cards.push(card.upcast());
    cards
}

pub fn cheatsheet(app: &Rc<App>) -> Cards {
    let card = hbox(16);
    card.add_css_class("fk-card");
    let t = vbox(4);
    t.set_hexpand(true);
    t.append(&w::label("All your shortcuts in one window", &["fk-card-title"]));
    t.append(&w::label(
        "Give it a shortcut above (Super+/ works well) to pull it up whenever you forget one.",
        &["dim"],
    ));
    card.append(&t);
    let open = w::button("Open", Some("keyboard-show"), "suggested-action");
    open.add_css_class("pill");
    let win = app.window.clone();
    open.connect_clicked(move |_| {
        if let Some(a) = win.application().and_downcast::<adw::Application>() {
            crate::cheatsheet::open(&a);
        }
    });
    card.append(&open);
    let all = cheatsheet::collect(app.caps.desktop);
    let list: gtk::Widget = if all.is_empty() {
        w::empty_state("keyboard", "No shortcuts found", "This desktop's shortcut settings couldn't be read.").upcast()
    } else {
        crate::cheatsheet::grid(&all, "").upcast()
    };
    vec![card.upcast(), list]
}
