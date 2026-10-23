//! Mosaic and Pin pages.

use std::rc::Rc;

use adw::prelude::*;
use forgekit_core::capability::Desktop;
use forgekit_core::desktop;
use forgekit_core::tools::mosaic::{self, PRESETS, PinSettings, Zone};
use gtk::cairo;

use super::boxed_list;
use crate::shortcut::{self, Spec};
use crate::widgets::{self as w, hbox, vbox};
use crate::window::App;

type Cards = Vec<gtk::Widget>;

fn hex(s: &str) -> (f64, f64, f64) {
    let h = s.trim_start_matches('#');
    let c = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("80"), 16).unwrap_or(128) as f64 / 255.0;
    (c(0), c(2), c(4))
}

/// Backend card: the GNOME extension's status, or a note that KDE needs nothing.
fn backend_card(app: &Rc<App>, page: &'static str) -> gtk::Widget {
    let rows = boxed_list();
    match app.caps.desktop {
        Desktop::Kde => {
            let r = adw::ActionRow::builder()
                .title("Works through KWin")
                .subtitle("Nothing to install. For dragging windows into zones, use Plasma's own Shift-drag tiling.")
                .build();
            r.add_prefix(&w::icon_tile("circle-check", false, "ok"));
            rows.append(&r);
        }
        _ => rows.append(&super::extension_row(app, page)),
    }
    rows.upcast()
}

/// A small drawing of a layout's zones, numbered.
fn layout_preview(zones: Vec<Zone>, accent: &'static str) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder().content_width(150).content_height(88).build();
    area.set_draw_func(move |a, cr, wd, ht| {
        let (w, h) = (wd as f64, ht as f64);
        let fg = a.color();
        cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.06);
        rounded(cr, 0.0, 0.0, w, h, 10.0);
        let _ = cr.fill();
        let (r, g, b) = hex(accent);
        for (i, z) in zones.iter().enumerate() {
            let (x, y, zw, zh) = (z.x * w + 3.0, z.y * h + 3.0, z.w * w - 6.0, z.h * h - 6.0);
            cr.set_source_rgba(r, g, b, 0.22);
            rounded(cr, x, y, zw, zh, 6.0);
            let _ = cr.fill_preserve();
            cr.set_source_rgba(r, g, b, 0.9);
            cr.set_line_width(1.5);
            let _ = cr.stroke();
            cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.85);
            cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
            cr.set_font_size(13.0);
            let label = format!("{}", i + 1);
            if let Ok(ext) = cr.text_extents(&label) {
                cr.move_to(x + zw / 2.0 - ext.width() / 2.0, y + zh / 2.0 + ext.height() / 2.0);
                let _ = cr.show_text(&label);
            }
        }
    });
    area
}

fn rounded(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

fn save(app: &Rc<App>, s: &mosaic::Settings) {
    match s.save() {
        Ok(()) => desktop::write_config(None),
        Err(e) => app.toast(&format!("{e:#}")),
    }
}

// ------------------------------------------------------------------ Mosaic

pub fn mosaic(app: &Rc<App>) -> Cards {
    let s = mosaic::Settings::load().unwrap_or_default();
    let accent = crate::theme::find(&app.appearance.borrow().theme).palette.accent;
    let gnome = app.caps.desktop == Desktop::Gnome;
    let mut cards: Cards = vec![backend_card(app, "mosaic")];

    // ---- how to use
    let (how, how_body) = w::card("How to use it", "");
    let steps: Vec<(&str, &str, &str)> = if gnome {
        vec![
            (
                "drag-drop",
                "Hold Shift while dragging a window",
                "Zones appear; let go over one and the window fills it.",
            ),
            ("keyboard", "Or use a keyboard shortcut", "Give each zone a shortcut below, e.g. Super+Alt+1."),
        ]
    } else {
        vec![("keyboard", "Use keyboard shortcuts", "Give each zone a shortcut below, e.g. Super+Alt+1.")]
    };
    for (icon, title, text) in steps {
        let r = hbox(12);
        r.append(&w::icon_tile(icon, false, "off"));
        let t = vbox(2);
        t.append(&w::label(title, &["fk-tile-title"]));
        t.append(&w::label(text, &["dim"]));
        r.append(&t);
        how_body.append(&r);
    }
    cards.push(how.upcast());

    // ---- layouts
    let (layouts, l_body) = w::card("Layout", "Pick how your screen is divided");
    let gallery = gtk::FlowBox::builder()
        .homogeneous(true)
        .min_children_per_line(2)
        .max_children_per_line(4)
        .column_spacing(6)
        .row_spacing(6)
        .selection_mode(gtk::SelectionMode::None)
        .activate_on_single_click(true)
        .css_classes(["fk-themes"])
        .build();
    let mut entries: Vec<(String, String, Vec<Zone>)> =
        PRESETS.iter().map(|(id, name, z)| (id.to_string(), name.to_string(), z.to_vec())).collect();
    entries.push(("grid".into(), format!("Grid {} × {}", s.columns, s.rows), mosaic::grid(s.columns, s.rows)));
    for (id, name, zones) in entries {
        let c = vbox(6);
        c.add_css_class("fk-theme-card");
        if id == s.layout {
            c.add_css_class("selected");
        }
        c.append(&layout_preview(zones, accent));
        let label = hbox(6);
        label.set_halign(gtk::Align::Center);
        if id == s.layout {
            label.append(&w::icon("check", 14));
        }
        label.append(&gtk::Label::builder().label(&name).css_classes(["fk-theme-name"]).build());
        c.append(&label);
        gallery.append(&gtk::FlowBoxChild::builder().child(&c).name(&id).build());
    }
    {
        let a = app.clone();
        gallery.connect_child_activated(move |_, child| {
            let mut s = mosaic::Settings::load().unwrap_or_default();
            s.layout = child.widget_name().to_string();
            save(&a, &s);
            a.rebuild_page("mosaic");
        });
    }
    l_body.append(&gallery);

    let rows = boxed_list();
    let cols = adw::SpinRow::with_range(1.0, 6.0, 1.0);
    cols.set_title("Grid columns");
    cols.set_value(s.columns as f64);
    let rws = adw::SpinRow::with_range(1.0, 4.0, 1.0);
    rws.set_title("Grid rows");
    rws.set_value(s.rows as f64);
    let gap = adw::SpinRow::with_range(0.0, 64.0, 2.0);
    gap.set_title("Gap between windows");
    gap.set_subtitle("pixels");
    gap.set_value(s.gap as f64);
    for r in [&cols, &rws] {
        let (a, cols, rws) = (app.clone(), cols.clone(), rws.clone());
        r.connect_value_notify(move |_| {
            let mut s = mosaic::Settings::load().unwrap_or_default();
            s.columns = cols.value() as u8;
            s.rows = rws.value() as u8;
            s.layout = "grid".into();
            save(&a, &s);
            // Redraw after the spin button settles.
            let a = a.clone();
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || a.rebuild_page("mosaic"));
        });
    }
    {
        let a = app.clone();
        gap.connect_value_notify(move |g| {
            let mut s = mosaic::Settings::load().unwrap_or_default();
            s.gap = g.value() as u32;
            save(&a, &s);
        });
    }
    rows.append(&cols);
    rows.append(&rws);
    rows.append(&gap);
    if gnome {
        let drag = adw::SwitchRow::builder().title("Shift-drag to snap").active(s.shift_drag).build();
        let a = app.clone();
        drag.connect_active_notify(move |r| {
            let mut s = mosaic::Settings::load().unwrap_or_default();
            s.shift_drag = r.is_active();
            save(&a, &s);
        });
        rows.append(&drag);
    }
    l_body.append(&rows);
    cards.push(layouts.upcast());

    // ---- per-zone shortcuts
    let (keys, k_body) = w::card("Zone shortcuts", "Move the window you're using into a zone from the keyboard");
    let list = boxed_list();
    for n in 1..=s.zones().len() {
        let r = adw::ActionRow::builder().title(format!("Zone {n}")).build();
        r.add_prefix(&gtk::Label::builder().label(n.to_string()).css_classes(["fk-kbd"]).build());
        let control = shortcut::row_for(Spec::zone(n), app);
        control.set_valign(gtk::Align::Center);
        r.add_suffix(&control);
        list.append(&r);
    }
    k_body.append(&list);
    cards.push(keys.upcast());
    cards
}

// ------------------------------------------------------------------ Pin

pub fn pin(app: &Rc<App>) -> Cards {
    let gnome = app.caps.desktop == Desktop::Gnome;
    let mut cards: Cards = vec![backend_card(app, "pin")];

    let hero = hbox(16);
    hero.add_css_class("fk-card");
    hero.append(&w::icon_tile("pinned", true, ""));
    let t = vbox(4);
    t.set_hexpand(true);
    t.set_valign(gtk::Align::Center);
    t.append(&w::label("Keep a window above all others", &["fk-card-title"]));
    t.append(&w::label(
        "Focus the window, then press your Pin shortcut (set it at the top of this page). Press it again to unpin.",
        &["dim"],
    ));
    hero.append(&t);
    cards.push(hero.upcast());

    if gnome {
        let p = PinSettings::load().unwrap_or_default();
        let (look, body) = w::card("Pinned windows look", "");
        let rows = boxed_list();
        let border = adw::SwitchRow::builder()
            .title("Colored border")
            .subtitle("So you can tell which windows stay on top")
            .active(p.border)
            .build();
        {
            let a = app.clone();
            border.connect_active_notify(move |r| {
                let mut p = PinSettings::load().unwrap_or_default();
                p.border = r.is_active();
                match p.save() {
                    Ok(()) => desktop::write_config(None),
                    Err(e) => a.toast(&format!("{e:#}")),
                }
            });
        }
        rows.append(&border);
        let color_row = adw::ActionRow::builder().title("Border and zone color").build();
        let swatches = hbox(6);
        swatches.set_valign(gtk::Align::Center);
        for (name, c) in crate::theme::ACCENTS {
            let area = gtk::DrawingArea::builder().content_width(22).content_height(22).build();
            let (r, g, b) = hex(c);
            area.set_draw_func(move |_, cr, wd, ht| {
                cr.set_source_rgb(r, g, b);
                cr.arc(wd as f64 / 2.0, ht as f64 / 2.0, wd.min(ht) as f64 / 2.0, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
            });
            let btn =
                gtk::Button::builder().child(&area).tooltip_text(*name).css_classes(["flat", "fk-swatch"]).build();
            if p.color.eq_ignore_ascii_case(c) {
                btn.add_css_class("selected");
            }
            let a = app.clone();
            btn.connect_clicked(move |_| {
                let mut p = PinSettings::load().unwrap_or_default();
                p.color = c.to_string();
                match p.save() {
                    Ok(()) => desktop::write_config(None),
                    Err(e) => a.toast(&format!("{e:#}")),
                }
                a.rebuild_page("pin");
            });
            swatches.append(&btn);
        }
        color_row.add_suffix(&swatches);
        rows.append(&color_row);
        body.append(&rows);
        cards.push(look.upcast());
    }
    cards
}
