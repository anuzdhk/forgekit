//! Home: what works here, what needs attention, and every tool as a tile.

use std::rc::Rc;

use adw::prelude::*;
use chrono::Timelike;
use forgekit_core::capability::Session;
use forgekit_core::catalog::Category;
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
    let setup =
        statuses.iter().filter(|(t, s, on)| !t.is_planned() && *on && matches!(s, Status::NeedsSetup(_))).count();
    let off = statuses.iter().filter(|(t, _, on)| !t.is_planned() && !on).count();
    let soon = statuses.iter().filter(|(t, _, _)| t.is_planned()).count();

    // ---- greeting
    let head = hbox(16);
    let text = vbox(4);
    text.set_hexpand(true);
    let hour = chrono::Local::now().hour();
    let part = match hour {
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
    text.append(&w::label(&format!("{desktop} on {session} · {ready} tools ready to use"), &["fk-page-subtitle"]));
    head.append(&text);
    let shelf_btn = w::button("Open DropShelf", Some("stack-2"), "suggested-action");
    shelf_btn.add_css_class("pill");
    {
        let win = app.window.clone();
        shelf_btn.connect_clicked(move |_| {
            if let Some(a) = win.application().and_downcast::<adw::Application>() {
                crate::shelf::open(&a, Vec::new());
            }
        });
    }
    head.append(&shelf_btn);
    root.append(&head);

    // ---- stats
    let stats = gtk::FlowBox::builder()
        .homogeneous(true)
        .min_children_per_line(2)
        .max_children_per_line(4)
        .column_spacing(14)
        .row_spacing(14)
        .selection_mode(gtk::SelectionMode::None)
        .build();
    for (value, label, icon, tone) in [
        (ready, "Ready to use", "circle-check", "ok"),
        (setup, "Need setup", "alert-triangle", "warn"),
        (off, "Turned off", "eye-off", "off"),
        (soon, "Coming soon", "clock", ""),
    ] {
        let card = hbox(14);
        card.add_css_class("fk-card");
        card.add_css_class("fk-stat");
        card.append(&w::icon_tile(icon, false, tone));
        let t = vbox(0);
        t.append(&w::label(&value.to_string(), &["fk-stat-value"]));
        t.append(&w::label(label, &["fk-stat-label"]));
        card.append(&t);
        stats.append(&card);
    }
    root.append(&stats);

    // ---- quick actions
    root.append(&w::label("Quick actions", &["fk-section-title"]));
    let quick = gtk::FlowBox::builder()
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(4)
        .column_spacing(12)
        .row_spacing(12)
        .selection_mode(gtk::SelectionMode::None)
        .build();
    let actions: [(&str, &str, &str, &str); 6] = [
        ("Pick a color", "Copy any pixel's color", "color-picker", "eyedrop"),
        ("Grab text", "OCR an area of the screen", "text-scan-2", "glyphgrab"),
        ("Clipboard history", "Find something you copied", "clipboard-list", "clipstack"),
        ("Keep awake", "Stop sleep for a while", "moon-stars", "nightowl"),
        ("Install a package", ".deb, .rpm, AppImage…", "package", "teku"),
        ("Rename files", "Bulk rename with preview", "forms", "relabel"),
    ];
    for (title, sub, icon, id) in actions {
        let tool = forgekit_core::catalog::find(id).unwrap();
        if tool.is_planned() {
            continue;
        }
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
        btn.connect_clicked(move |btn| match id {
            "eyedrop" | "glyphgrab" => run_quick(&a, btn, id),
            _ => a.navigate(id),
        });
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
            let open = w::button("Open", None, "flat");
            let (a, id) = (app.clone(), t.id);
            open.connect_clicked(move |_| a.navigate(id));
            row.append(&open);
            body.append(&row);
        }
        root.append(&card);
    }

    // ---- every tool
    let appearance = app.appearance.borrow();
    for cat in Category::ALL {
        let tools: Vec<_> = statuses
            .iter()
            .filter(|(t, s, _)| {
                t.category == cat
                    && !(appearance.hide_planned && t.is_planned())
                    && !(appearance.hide_unavailable && !t.is_planned() && matches!(s, Status::Unsupported(_)))
            })
            .collect();
        if tools.is_empty() {
            continue;
        }
        root.append(&w::label(cat.title(), &["fk-section-title"]));
        let grid = gtk::FlowBox::builder()
            .homogeneous(true)
            .min_children_per_line(1)
            .max_children_per_line(3)
            .column_spacing(14)
            .row_spacing(14)
            .selection_mode(gtk::SelectionMode::None)
            .activate_on_single_click(true)
            .css_classes(["fk-tiles"])
            .build();
        for (t, s, on) in tools {
            let tile = vbox(10);
            tile.add_css_class("fk-card");
            tile.add_css_class("fk-tile");
            if t.is_planned() || matches!(s, Status::Unsupported(_)) || !on {
                tile.add_css_class("unavailable");
            }
            let top = hbox(12);
            top.append(&w::icon_tile(t.icon, false, ""));
            let name = w::label(t.name, &["fk-tile-title"]);
            name.set_hexpand(true);
            top.append(&name);
            let (text, tone) = w::status_style(t, s, *on);
            top.append(&w::pill(&text, tone));
            tile.append(&top);
            tile.append(&w::label(t.summary, &["fk-tile-summary"]));
            let child = gtk::FlowBoxChild::builder().child(&tile).name(t.id).build();
            grid.append(&child);
        }
        let a = app.clone();
        grid.connect_child_activated(move |_, child| a.navigate(child.widget_name().as_str()));
        root.append(&grid);
    }

    w::page(&root).upcast()
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
