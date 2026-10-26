//! Home: one-click Clean PC, the shortcuts worth remembering, a few quick
//! actions, and anything that still needs setting up. Every tool is in the
//! sidebar, so Home doesn't repeat the full list.

use std::rc::Rc;

use adw::prelude::*;
use chrono::Timelike;
use forgekit_core::capability::Session;
use forgekit_core::shortcuts;
use forgekit_core::tools::glance::human_size;
use forgekit_core::tools::sweep;
use forgekit_core::{CATALOG, Status};

use crate::util;
use crate::widgets::{self as w, hbox, vbox};
use crate::window::App;

pub fn build(app: &Rc<App>) -> gtk::Widget {
    let root = vbox(22);
    root.add_css_class("fk-page");
    let general = forgekit_core::settings::General::load().unwrap_or_default();
    let statuses: Vec<_> = CATALOG.iter().map(|t| (t, t.status(&app.caps), general.is_enabled(t.id))).collect();
    let ready = statuses.iter().filter(|(t, s, on)| !t.is_planned() && s.is_supported() && *on).count();

    // ---- greeting
    let text = vbox(4);
    let part = match chrono::Local::now().hour() {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    };
    let name = gtk::glib::real_name().to_string_lossy().split_whitespace().next().unwrap_or("").to_string();
    let greet = if name.is_empty() || name == "Unknown" { part.to_string() } else { format!("{part}, {name}") };
    text.append(&w::label(&greet, &["fk-page-title"]));
    let session = match app.caps.session {
        Session::Wayland => "Wayland",
        Session::X11 => "X11",
        _ => "a text session",
    };
    let desktop = app.caps.desktop_raw.rsplit(':').next().filter(|s| !s.is_empty()).unwrap_or("your desktop");
    text.append(&w::label(&format!("{desktop} on {session} · {ready} tools ready"), &["fk-page-subtitle"]));
    root.append(&text);

    // ---- Clean PC and shortcuts, side by side when there's room
    let top = gtk::FlowBox::builder()
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(2)
        .column_spacing(16)
        .row_spacing(16)
        .selection_mode(gtk::SelectionMode::None)
        .build();
    top.append(&clean_pc(app));
    top.append(&shortcuts(app));
    root.append(&top);

    // ---- quick actions
    root.append(&w::label("Quick actions", &["fk-section-title"]));
    let quick = gtk::FlowBox::builder()
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(3)
        .column_spacing(12)
        .row_spacing(12)
        .selection_mode(gtk::SelectionMode::None)
        .build();
    let actions: [(&str, &str, &str, &str); 6] = [
        ("Search", "Apps, maths and the web", "bolt", "spark"),
        ("Clipboard", "Paste something you copied", "clipboard-list", "clipstack"),
        ("Pick a color", "Copy any pixel's color", "color-picker", "eyedrop"),
        ("Grab text", "Copy text from the screen", "text-scan-2", "glyphgrab"),
        ("Bundle", "Hold files while you work", "stack-2", "shelf"),
        ("Keep awake", "Stop sleep for a while", "moon-stars", "nightowl"),
    ];
    for (title, sub, icon, id) in actions {
        let b = hbox(12);
        b.append(&w::icon_tile(icon, false, ""));
        let t = vbox(0);
        t.set_hexpand(true);
        t.set_valign(gtk::Align::Center);
        t.append(&w::label(title, &["fk-tile-title"]));
        t.append(&w::label(sub, &["fk-tile-summary"]));
        b.append(&t);
        let btn = gtk::Button::builder().child(&b).css_classes(["fk-quick"]).build();
        let a = app.clone();
        btn.connect_clicked(move |btn| quick_action(&a, btn, id));
        quick.append(&btn);
    }
    root.append(&quick);

    // ---- needs attention
    let attention: Vec<_> = statuses
        .iter()
        .filter_map(|(t, s, on)| match s {
            Status::NeedsSetup(r) if *on && !t.is_planned() => Some((t, r.clone())),
            _ => None,
        })
        .collect();
    if !attention.is_empty() {
        let (card, body) = w::card("Needs attention", "These tools work after a quick setup step");
        for (t, reason) in attention {
            let row = hbox(12);
            row.append(&w::icon_tile(t.icon, false, "warn"));
            let txt = vbox(2);
            txt.set_hexpand(true);
            txt.append(&w::label(t.name, &["fk-tile-title"]));
            txt.append(&w::label(&reason, &["fk-tile-summary"]));
            row.append(&txt);
            let open = w::button("Set up", None, "flat");
            let (a, id) = (app.clone(), t.id);
            open.connect_clicked(move |_| a.navigate(id));
            row.append(&open);
            body.append(&row);
        }
        root.append(&card);
    }

    w::page(&root).upcast()
}

// ------------------------------------------------------------------ Clean PC

/// Scan in the background and clean what's ticked on the Sweep page
/// (the safe, recommended items until the user changes that).
fn clean_pc(app: &Rc<App>) -> gtk::Widget {
    let card = vbox(14);
    card.add_css_class("fk-card");
    card.add_css_class("fk-clean");
    let head = hbox(14);
    head.append(&w::icon_tile("vacuum-cleaner", false, "accent"));
    let t = vbox(2);
    t.set_hexpand(true);
    t.set_valign(gtk::Align::Center);
    t.append(&w::label("Clean PC", &["fk-card-title"]));
    let amount = w::label("Checking what can be cleaned…", &["fk-clean-amount"]);
    t.append(&amount);
    head.append(&t);
    card.append(&head);
    let detail = w::label("", &["dim"]);
    card.append(&detail);

    let actions = hbox(8);
    let clean = w::button("Clean PC", Some("vacuum-cleaner"), "suggested-action");
    clean.add_css_class("pill");
    clean.set_sensitive(false);
    actions.append(&clean);
    let choose = w::button("Choose what to clean", None, "flat");
    let a = app.clone();
    choose.connect_clicked(move |_| a.navigate("sweep"));
    actions.append(&choose);
    card.append(&actions);

    let picked: Rc<std::cell::RefCell<Vec<sweep::Item>>> = Rc::default();
    let scan: Rc<dyn Fn()> = {
        let (amount, detail, clean, picked) = (amount.clone(), detail.clone(), clean.clone(), picked.clone());
        Rc::new(move || {
            let (amount, detail, clean, picked) = (amount.clone(), detail.clone(), clean.clone(), picked.clone());
            util::background(sweep::scan, move |items| {
                let saved = sweep::Settings::load().unwrap_or_default();
                let chosen: Vec<_> = items.into_iter().filter(|i| saved.picks(i)).collect();
                let total: u64 = chosen.iter().filter_map(|i| i.size).sum();
                amount.set_label(&if chosen.is_empty() {
                    "Nothing selected to clean".to_string()
                } else if total < 1_000_000 {
                    "Already clean".to_string()
                } else {
                    format!("{} can be freed", human_size(total))
                });
                detail.set_label(&chosen.iter().map(|i| i.title.as_str()).collect::<Vec<_>>().join(" · "));
                clean.set_sensitive(!chosen.is_empty());
                *picked.borrow_mut() = chosen;
            });
        })
    };
    scan();

    let a = app.clone();
    clean.connect_clicked(move |b| {
        let items = picked.borrow().clone();
        let kinds: Vec<sweep::Kind> = items.iter().map(|i| i.kind).collect();
        let trash = kinds.contains(&sweep::Kind::Trash);
        let list = items.iter().map(|i| format!("• {}", i.title)).collect::<Vec<_>>().join("\n");
        let body = if trash {
            format!("{list}\n\nEmptying the trash deletes those files for good.")
        } else {
            format!("{list}\n\nAll of it is rebuilt or downloaded again when needed.")
        };
        let dialog = adw::AlertDialog::new(Some("Clean your PC?"), Some(&body));
        dialog.add_responses(&[("cancel", "Cancel"), ("clean", "Clean")]);
        dialog.set_response_appearance(
            "clean",
            if trash { adw::ResponseAppearance::Destructive } else { adw::ResponseAppearance::Suggested },
        );
        let (a, scan, b2) = (a.clone(), scan.clone(), b.clone());
        dialog.connect_response(None, move |_, r| {
            if r != "clean" {
                return;
            }
            b2.set_sensitive(false);
            b2.set_label("Cleaning…");
            let kinds = kinds.clone();
            let (a, scan, b3) = (a.clone(), scan.clone(), b2.clone());
            util::background(
                move || sweep::clean(&kinds).map_err(|e| format!("{e:#}")),
                move |r| {
                    b3.set_label("Clean PC");
                    match r {
                        Ok(done) => a.toast(&done.join(" · ")),
                        Err(e) => a.toast(&e),
                    }
                    scan();
                },
            );
        });
        dialog.present(Some(b));
    });
    card.upcast()
}

// ------------------------------------------------------------------ shortcuts

/// The few gestures worth remembering, with the keys actually set.
fn shortcuts(app: &Rc<App>) -> gtk::Widget {
    let (card, body) = w::card("Shortcuts to remember", "");
    let grid = gtk::Grid::builder().column_spacing(16).row_spacing(12).build();
    let mut row = 0;
    let mut add = |label: &str, keys: gtk::Widget| {
        let l = w::label(label, &[]);
        l.set_hexpand(true);
        grid.attach(&l, 0, row, 1, 1);
        keys.set_halign(gtk::Align::End);
        grid.attach(&keys, 1, row, 1, 1);
        row += 1;
    };
    let spark_keys = hbox(0);
    let clip_keys = hbox(0);
    add("Search apps, maths, the web", spark_keys.clone().upcast());
    add("Clipboard history", clip_keys.clone().upcast());
    add("Preview a file in Files", crate::cheatsheet::keys_widget("Space").upcast());
    if app.caps.desktop == forgekit_core::capability::Desktop::Gnome {
        let drag = hbox(6);
        drag.append(&crate::cheatsheet::keys_widget("Shift"));
        drag.append(&w::label("+ drag a window to the top", &["dim"]));
        add("Snap layouts", drag.upcast());
    }
    body.append(&grid);

    // Reading the desktop's bindings runs gsettings, so do it off the UI thread.
    let desktop = app.caps.desktop;
    let a = app.clone();
    util::background(
        move || {
            let b = shortcuts::backend(desktop);
            ["spark", "clipstack"].map(|t| (t, b.and_then(|b| shortcuts::get(b, t)).map(|k| shortcuts::pretty(&k))))
        },
        move |keys| {
            for ((tool, accel), slot) in keys.into_iter().zip([&spark_keys, &clip_keys]) {
                match accel {
                    Some(k) => slot.append(&crate::cheatsheet::keys_widget(&k)),
                    None => {
                        let set = w::button("Set a key", None, "flat");
                        set.add_css_class("fk-link");
                        let a = a.clone();
                        set.connect_clicked(move |_| a.navigate(tool));
                        slot.append(&set);
                    }
                }
            }
        },
    );
    card.upcast()
}

// ------------------------------------------------------------------ quick actions

fn quick_action(app: &Rc<App>, btn: &gtk::Button, id: &'static str) {
    let gapp = app.window.application().and_downcast::<adw::Application>();
    match id {
        "eyedrop" | "glyphgrab" => run_quick(app, btn, id),
        "spark" => {
            if let Some(g) = gapp {
                crate::spark::toggle(&g);
            }
        }
        "shelf" => {
            if let Some(g) = gapp {
                crate::shelf::open(&g, Vec::new(), false);
            }
        }
        "clipstack" => {
            let _ = std::process::Command::new(shortcuts::bin_path("forgekit")).args(["clipboard", "open"]).spawn();
        }
        _ => app.navigate(id),
    }
}

/// Eyedrop and Glyphgrab straight from Home: run, copy, toast.
fn run_quick(app: &Rc<App>, btn: &gtk::Button, id: &'static str) {
    btn.set_sensitive(false);
    let (a, b) = (app.clone(), btn.clone());
    util::background(
        move || util::cli_output(&[id, "--no-copy"]),
        move |r| {
            b.set_sensitive(true);
            match r {
                Ok(out) => {
                    util::copy(&out);
                    let shown: String = out.chars().take(40).collect();
                    a.toast(&format!("Copied {shown}"));
                }
                Err(e) => a.toast(&e),
            }
        },
    );
}
