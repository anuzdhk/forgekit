//! Spark, Clipstack, DropShelf, Eyedrop, Glyphgrab and Glance pages.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use forgekit_core::tools::clipstack::{self, store::Store};
use forgekit_core::tools::{eyedrop, shelf, spark};
use gtk::{gdk, glib};

use super::boxed_list;
use crate::util;
use crate::widgets::{self as w, hbox, vbox};
use crate::window::App;

type Cards = Vec<gtk::Widget>;

// ------------------------------------------------------------------ Spark

pub fn spark(app: &Rc<App>) -> Cards {
    let (card, body) =
        w::card("Search everything", "Apps, maths (12*7), units (10 km to mi), install <file>, or the web");
    let entry = gtk::SearchEntry::builder().placeholder_text("Type to search…").build();
    entry.set_size_request(-1, 42);
    body.append(&entry);
    let results =
        gtk::ListBox::builder().css_classes(["boxed-list"]).selection_mode(gtk::SelectionMode::Single).build();
    results.set_visible(false);
    body.append(&results);

    let apps = Rc::new(spark::load_apps());
    let items: Rc<RefCell<Vec<spark::Item>>> = Rc::default();
    {
        let (results, items) = (results.clone(), items.clone());
        entry.connect_search_changed(move |e| {
            results.remove_all();
            *items.borrow_mut() = spark::query(&e.text(), &apps);
            results.set_visible(!items.borrow().is_empty());
            for it in items.borrow().iter() {
                let row = adw::ActionRow::builder()
                    .title(w::escape(&it.title))
                    .subtitle(w::escape(&it.subtitle))
                    .activatable(true)
                    .build();
                let icon = match &it.action {
                    spark::Action::Copy { .. } => w::icon_tile("calculator", false, ""),
                    spark::Action::Install { .. } => w::icon_tile("package", false, ""),
                    spark::Action::Open { .. } => w::icon_tile("world", false, ""),
                    spark::Action::Launch { .. } => {
                        let img = if it.icon.starts_with('/') {
                            gtk::Image::from_file(&it.icon)
                        } else {
                            gtk::Image::from_icon_name(&it.icon)
                        };
                        img.set_pixel_size(32);
                        img.set_margin_start(3);
                        img.set_margin_end(3);
                        img
                    }
                };
                row.add_prefix(&icon);
                row.add_suffix(&w::icon("arrow-right", 16));
                results.append(&row);
            }
            if let Some(first) = results.row_at_index(0) {
                results.select_row(Some(&first));
            }
        });
    }
    let run = {
        let (a, items) = (app.clone(), items.clone());
        Rc::new(move |i: usize| {
            let Some(it) = items.borrow().get(i).cloned() else { return };
            match &it.action {
                spark::Action::Copy { text } => {
                    util::copy(text);
                    a.toast(&format!("Copied {text}"));
                }
                action => match spark::run(action) {
                    Ok(()) => a.toast(&format!("Opening {}", it.title)),
                    Err(e) => a.toast(&format!("{e:#}")),
                },
            }
        })
    };
    {
        let run = run.clone();
        results.connect_row_activated(move |_, row| run(row.index() as usize));
    }
    {
        let results = results.clone();
        entry.connect_activate(move |_| {
            if let Some(r) = results.selected_row() {
                run(r.index() as usize);
            }
        });
    }

    vec![card.upcast()]
}

// ------------------------------------------------------------------ Clipstack

pub fn clipstack(app: &Rc<App>) -> Cards {
    let mut cards: Cards = Vec::new();
    let running = crate::settings_page::daemon_running();

    // ---- status
    let status = hbox(14);
    status.add_css_class("fk-card");
    if running {
        status.append(&w::icon_tile("circle-check", false, "ok"));
        let t = vbox(2);
        t.set_hexpand(true);
        t.append(&w::label("Recording your clipboard", &["fk-card-title"]));
        let how = if app.caps.has_data_control() {
            "Using the compositor's clipboard protocol"
        } else {
            "Using XWayland's clipboard bridge"
        };
        t.append(&w::label(&format!("{how}. Passwords from password managers are skipped."), &["dim"]));
        status.append(&t);
        let private = gtk::Switch::builder()
            .active(clipstack::Settings::load().unwrap_or_default().private)
            .valign(gtk::Align::Center)
            .build();
        let pb = vbox(4);
        pb.set_valign(gtk::Align::Center);
        pb.append(&private);
        pb.append(&gtk::Label::builder().label("Private").css_classes(["dim", "caption"]).build());
        let a = app.clone();
        private.connect_active_notify(move |s| {
            let mut st = clipstack::Settings::load().unwrap_or_default();
            st.private = s.is_active();
            match st.save() {
                Ok(()) => a.toast(if st.private { "Private mode: nothing is recorded" } else { "Recording again" }),
                Err(e) => a.toast(&format!("{e:#}")),
            }
        });
        status.append(&pb);
    } else {
        status.append(&w::icon_tile("alert-triangle", false, "warn"));
        let t = vbox(2);
        t.set_hexpand(true);
        t.append(&w::label("The background service isn't running", &["fk-card-title"]));
        t.append(&w::label(
            "Clipstack records your clipboard through forgekitd. Start it, and turn on “Start at login” in Settings.",
            &["dim"],
        ));
        status.append(&t);
        let start = w::button("Start", Some("player-play"), "suggested-action");
        let a = app.clone();
        start.connect_clicked(move |_| match crate::settings_page::start_daemon() {
            Ok(()) => {
                a.toast("Started forgekitd");
                let a = a.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(900), move || {
                    a.rebuild_page("clipstack")
                });
            }
            Err(e) => a.toast(&e),
        });
        status.append(&start);
    }
    cards.push(status.upcast());

    // ---- history
    let search = gtk::SearchEntry::builder().placeholder_text("Search history…").hexpand(true).build();
    let clear = w::button("Clear", Some("trash"), "flat");
    let tools = hbox(8);
    tools.append(&search);
    tools.append(&clear);
    let (hist, hist_body) = w::card("History", "Click an item to copy it. Pinned items stay until you unpin them.");
    hist_body.append(&tools);
    let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["fk-table"]).build();
    let list_card = vbox(0);
    list_card.add_css_class("fk-card");
    list_card.add_css_class("flush");
    list_card.append(&list);
    hist_body.append(&list_card);
    let empty =
        w::empty_state("clipboard-list", "Nothing here yet", "Copy some text or an image and it shows up here.");
    hist_body.append(&empty);

    let fill: Rc<dyn Fn()> = {
        let (a, list, search, empty, list_card) =
            (app.clone(), list.clone(), search.clone(), empty.clone(), list_card.clone());
        Rc::new(move || {
            list.remove_all();
            let items = Store::open_default().and_then(|s| s.list(&search.text(), 100)).unwrap_or_default();
            empty.set_visible(items.is_empty());
            list_card.set_visible(!items.is_empty());
            for item in items {
                list.append(&clip_row(&a, item));
            }
        })
    };
    fill();
    {
        let fill = fill.clone();
        search.connect_search_changed(move |_| fill());
    }
    {
        let (a, fill) = (app.clone(), fill.clone());
        clear.connect_clicked(move |b| {
            let dialog = adw::AlertDialog::new(
                Some("Clear clipboard history?"),
                Some("Everything except pinned items is removed. This can't be undone."),
            );
            dialog.add_responses(&[("cancel", "Cancel"), ("clear", "Clear")]);
            dialog.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
            let (a, fill) = (a.clone(), fill.clone());
            dialog.connect_response(None, move |_, r| {
                if r == "clear" {
                    match Store::open_default().and_then(|s| s.clear()) {
                        Ok(n) => a.toast(&format!("Removed {n} item(s)")),
                        Err(e) => a.toast(&format!("{e:#}")),
                    }
                    fill();
                }
            });
            dialog.present(Some(b));
        });
    }
    // Live updates; the subscription lives as long as the list.
    let sub = {
        let fill = fill.clone();
        util::on_clipstack_changed(move || fill())
    };
    list.connect_destroy(move |_| {
        let _ = &sub;
    });
    cards.push(hist.upcast());

    // ---- settings
    let (set_card, set_body) = w::card("Limits", "Older items are removed automatically. Pinned items never are.");
    let rows = boxed_list();
    let s = clipstack::Settings::load().unwrap_or_default();
    let spin = |title: &str, sub: &str, min: f64, max: f64, step: f64, value: f64| {
        let r = adw::SpinRow::with_range(min, max, step);
        r.set_title(title);
        r.set_subtitle(sub);
        r.set_value(value);
        r
    };
    let items = spin("Keep up to", "items", 10.0, 5000.0, 10.0, s.max_items as f64);
    let days = spin("Forget after", "days (0 = never)", 0.0, 365.0, 1.0, s.expire_days as f64);
    let img = spin("Largest image", "MB", 1.0, 50.0, 1.0, (s.max_image_kb / 1024) as f64);
    for r in [&items, &days, &img] {
        rows.append(r);
        let (items, days, img, a) = (items.clone(), days.clone(), img.clone(), app.clone());
        r.connect_value_notify(move |_| {
            let mut s = clipstack::Settings::load().unwrap_or_default();
            s.max_items = items.value() as usize;
            s.expire_days = days.value() as u32;
            s.max_image_kb = img.value() as usize * 1024;
            if let Err(e) = s.save() {
                a.toast(&format!("{e:#}"));
            }
        });
    }
    set_body.append(&rows);
    cards.push(set_card.upcast());
    cards
}

fn clip_row(app: &Rc<App>, item: clipstack::store::Item) -> gtk::ListBoxRow {
    let b = hbox(12);
    let store = Store::open_default().ok();
    let thumb: gtk::Widget = if item.is_image() {
        match store.as_ref().and_then(|s| s.get(item.id).ok().flatten()).and_then(|c| util::thumbnail(&c.data, 96)) {
            Some(t) => util::thumb_widget(&t, 40).upcast(),
            None => w::icon_tile("photo", false, "").upcast(),
        }
    } else {
        w::icon_tile("file-text", false, "off").upcast()
    };
    b.append(&thumb);
    let text = vbox(3);
    text.set_hexpand(true);
    let preview: String = item.preview.split_whitespace().collect::<Vec<_>>().join(" ");
    text.append(
        &gtk::Label::builder()
            .label(&preview)
            .xalign(0.0)
            .lines(2)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build(),
    );
    let meta = format!(
        "{} · {}{}",
        w::ago(item.last_used),
        forgekit_core::tools::glance::human_size(item.size as u64),
        if item.pinned { " · pinned" } else { "" }
    );
    text.append(&w::label(&meta, &["dim", "caption"]));
    b.append(&text);

    let pin = w::icon_button(if item.pinned { "pinned" } else { "pin" }, if item.pinned { "Unpin" } else { "Pin" });
    if item.pinned {
        pin.add_css_class("accent");
    }
    let del = w::icon_button("trash", "Delete");
    let copy = w::icon_button("copy", "Copy");
    b.append(&copy);
    b.append(&pin);
    b.append(&del);
    let row = gtk::ListBoxRow::builder().child(&b).activatable(true).build();

    let id = item.id;
    let do_copy = {
        let a = app.clone();
        Rc::new(move || match util::clipstack_copy(id) {
            Ok(()) => a.toast("Copied"),
            Err(_) => {
                // No daemon: copy through GTK (works while Forgekit is open).
                let clip = Store::open_default().ok().and_then(|s| s.get(id).ok().flatten());
                if let (Some(c), Some(d)) = (clip, gdk::Display::default()) {
                    if c.is_image() {
                        if let Ok(t) = gdk::Texture::from_bytes(&glib::Bytes::from(&c.data)) {
                            d.clipboard().set_texture(&t);
                        }
                    } else {
                        d.clipboard().set_text(&String::from_utf8_lossy(&c.data));
                    }
                    a.toast("Copied");
                }
            }
        })
    };
    {
        let c = do_copy.clone();
        copy.connect_clicked(move |_| c());
    }
    let r2 = row.clone();
    let gesture = gtk::GestureClick::new();
    gesture.connect_released(move |g, _, _, _| {
        // Buttons inside the row handle their own clicks.
        if g.widget().is_some_and(|w| w == r2) {
            do_copy();
        }
    });
    row.add_controller(gesture);
    {
        let a = app.clone();
        pin.connect_clicked(move |_| {
            if let Ok(s) = Store::open_default() {
                let _ = s.set_pinned(id, !item.pinned);
            }
            a.rebuild_page("clipstack");
        });
    }
    {
        let r = row.clone();
        del.connect_clicked(move |_| {
            if let Ok(s) = Store::open_default() {
                let _ = s.delete(id);
            }
            if let Some(l) = r.parent().and_downcast::<gtk::ListBox>() {
                l.remove(&r);
            }
        });
    }
    row
}

// ------------------------------------------------------------------ DropShelf

pub fn shelf(app: &Rc<App>) -> Cards {
    let hero = hbox(16);
    hero.add_css_class("fk-card");
    let t = vbox(4);
    t.set_hexpand(true);
    t.append(&w::label("Your shelf", &["fk-card-title"]));
    let n = shelf::Shelf::load().items.len();
    t.append(&w::label(
        &match n {
            0 => "Empty. Drop files, text or links on it, switch apps, then drag them out.".to_string(),
            1 => "1 item waiting on the shelf.".to_string(),
            n => format!("{n} items waiting on the shelf."),
        },
        &["dim"],
    ));
    hero.append(&w::icon_tile("stack-2", false, ""));
    hero.append(&t);
    let open = w::button("Open shelf", Some("stack-2"), "suggested-action");
    open.add_css_class("pill");
    {
        let win = app.window.clone();
        open.connect_clicked(move |_| {
            if let Some(a) = win.application().and_downcast::<adw::Application>() {
                crate::shelf::open(&a, Vec::new(), false);
            }
        });
    }
    hero.append(&open);

    let (how, how_body) = w::card("How it works", "");
    for (icon, title, text) in [
        ("drag-drop", "Drop things on it", "Files and folders, selected text, links and images."),
        ("arrows-shuffle", "Switch apps", "The shelf keeps everything until you drag it out or clear it."),
        ("download", "Drag out", "Drag one item, or the stack handle to drag all of them at once."),
    ] {
        let r = hbox(12);
        r.append(&w::icon_tile(icon, false, "off"));
        let t = vbox(2);
        t.append(&w::label(title, &["fk-tile-title"]));
        t.append(&w::label(text, &["dim"]));
        r.append(&t);
        how_body.append(&r);
    }

    let (opts, opts_body) = w::card("Options", "");
    let rows = boxed_list();
    let copy = adw::SwitchRow::builder()
        .title("Copy files into the shelf")
        .subtitle("Keeps temporary downloads safe even if the original is deleted")
        .active(shelf::Settings::load().unwrap_or_default().copy_files)
        .build();
    let a = app.clone();
    copy.connect_active_notify(move |r| {
        let mut st = shelf::Settings::load().unwrap_or_default();
        st.copy_files = r.is_active();
        if let Err(e) = st.save() {
            a.toast(&format!("{e:#}"));
        }
    });
    rows.append(&copy);
    opts_body.append(&rows);
    vec![hero.upcast(), popup_card(app), how.upcast(), opts.upcast()]
}

/// "Pop up while dragging": the GNOME Shell extension and its settings.
fn popup_card(app: &Rc<App>) -> gtk::Widget {
    use forgekit_core::capability::{Desktop, Session};
    use forgekit_core::desktop as ext;

    let gnome = app.caps.desktop == Desktop::Gnome && app.caps.session == Session::Wayland;
    let (card, body) = w::card(
        "Pop up while dragging",
        "Start dragging files and the shelf appears in a corner, on top of everything. It closes again if you don't drop anything on it.",
    );
    if !gnome {
        body.append(&w::label(
            "This needs desktop support that's only available on GNOME so far. Open the shelf with its keyboard shortcut instead.",
            &["dim"],
        ));
        return card.upcast();
    }

    let rows = boxed_list();
    rows.append(&super::extension_row(app, "shelf"));

    let s = shelf::Settings::load().unwrap_or_default();
    let auto = adw::SwitchRow::builder().title("Show when I start dragging").active(s.auto_show).build();
    let files = adw::SwitchRow::builder()
        .title("Only for files")
        .subtitle("Ignore dragged text, links and browser tabs")
        .active(s.files_only)
        .build();
    let corners = gtk::StringList::new(&shelf::CORNERS.iter().map(|(_, n)| *n).collect::<Vec<_>>());
    let corner = adw::ComboRow::builder().title("Corner").model(&corners).build();
    corner.set_selected(shelf::CORNERS.iter().position(|(id, _)| *id == s.corner).unwrap_or(0) as u32);
    files.set_sensitive(s.auto_show);

    let save = {
        let (a, auto, files, corner) = (app.clone(), auto.clone(), files.clone(), corner.clone());
        Rc::new(move || {
            let mut st = shelf::Settings::load().unwrap_or_default();
            st.auto_show = auto.is_active();
            st.files_only = files.is_active();
            st.corner = shelf::CORNERS[corner.selected() as usize].0.to_string();
            files.set_sensitive(st.auto_show);
            match st.save() {
                Ok(()) => ext::write_config(None),
                Err(e) => a.toast(&format!("{e:#}")),
            }
        })
    };
    for r in [&auto, &files] {
        let save = save.clone();
        r.connect_active_notify(move |_| save());
    }
    corner.connect_selected_notify(move |_| save());
    rows.append(&auto);
    rows.append(&files);
    rows.append(&corner);
    body.append(&rows);
    card.upcast()
}

// ------------------------------------------------------------------ Eyedrop

fn hex_rgb(c: eyedrop::Rgb) -> (f64, f64, f64) {
    (c.0 as f64 / 255.0, c.1 as f64 / 255.0, c.2 as f64 / 255.0)
}

fn swatch(c: eyedrop::Rgb, size: i32, round: bool) -> gtk::DrawingArea {
    let a = gtk::DrawingArea::builder().content_width(size).content_height(size).build();
    a.set_draw_func(move |_, cr, wd, ht| {
        let (r, g, b) = hex_rgb(c);
        cr.set_source_rgb(r, g, b);
        let (wd, ht) = (wd as f64, ht as f64);
        if round {
            cr.arc(wd / 2.0, ht / 2.0, wd.min(ht) / 2.0, 0.0, std::f64::consts::TAU);
        } else {
            let rad = 14.0;
            cr.new_sub_path();
            cr.arc(wd - rad, rad, rad, -std::f64::consts::FRAC_PI_2, 0.0);
            cr.arc(wd - rad, ht - rad, rad, 0.0, std::f64::consts::FRAC_PI_2);
            cr.arc(rad, ht - rad, rad, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
            cr.arc(rad, rad, rad, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
            cr.close_path();
        }
        let _ = cr.fill();
    });
    a
}

pub fn eyedrop(app: &Rc<App>) -> Cards {
    let pick = w::button("Pick a color", Some("color-picker"), "suggested-action");
    pick.add_css_class("pill");
    let (card, body) = w::card_with_action("Current color", "Click Pick, then click anywhere on screen", &pick);
    let current = hbox(18);
    body.append(&current);
    let (hist, hist_body) = w::card("Recent colors", "Click one to copy its HEX value");
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(12)
        .min_children_per_line(4)
        .row_spacing(10)
        .column_spacing(10)
        .build();
    hist_body.append(&grid);

    let show: Rc<dyn Fn()> = {
        let (a, current, grid, hist) = (app.clone(), current.clone(), grid.clone(), hist.clone());
        Rc::new(move || {
            while let Some(c) = current.first_child() {
                current.remove(&c);
            }
            let history = eyedrop::history();
            match history.first() {
                Some(&c) => {
                    current.append(&swatch(c, 96, false));
                    let rows = boxed_list();
                    rows.set_hexpand(true);
                    for f in [eyedrop::Format::Hex, eyedrop::Format::Rgb, eyedrop::Format::Hsl] {
                        let v = c.format(f);
                        let r = adw::ActionRow::builder()
                            .title(format!("{f:?}").to_uppercase())
                            .subtitle(&v)
                            .subtitle_selectable(true)
                            .build();
                        let b = w::icon_button("copy", "Copy");
                        let a = a.clone();
                        b.connect_clicked(move |_| {
                            util::copy(&v);
                            a.toast(&format!("Copied {v}"));
                        });
                        r.add_suffix(&b);
                        rows.append(&r);
                    }
                    current.append(&rows);
                }
                None => current.append(&w::empty_state(
                    "color-picker",
                    "No colors yet",
                    "Pick one to see it here in HEX, RGB and HSL.",
                )),
            }
            while let Some(c) = grid.first_child() {
                grid.remove(&c);
            }
            hist.set_visible(history.len() > 1);
            for c in history {
                let hex = c.format(eyedrop::Format::Hex);
                let b = vbox(4);
                b.append(&swatch(c, 40, true));
                b.append(&gtk::Label::builder().label(&hex).css_classes(["caption", "mono"]).build());
                let btn =
                    gtk::Button::builder().child(&b).css_classes(["flat"]).tooltip_text(format!("Copy {hex}")).build();
                let a = a.clone();
                btn.connect_clicked(move |_| {
                    util::copy(&hex);
                    a.toast(&format!("Copied {hex}"));
                });
                grid.append(&btn);
            }
        })
    };
    show();
    let a = app.clone();
    pick.connect_clicked(move |b| {
        b.set_sensitive(false);
        let (b, a, show) = (b.clone(), a.clone(), show.clone());
        util::background(
            || util::cli_output(&["eyedrop", "--no-copy"]),
            move |r| {
                b.set_sensitive(true);
                match r {
                    Ok(hex) => {
                        util::copy(&hex);
                        a.toast(&format!("Copied {hex}"));
                        show();
                    }
                    Err(e) => a.toast(&e),
                }
            },
        );
    });
    vec![card.upcast(), hist.upcast()]
}

// ------------------------------------------------------------------ Glyphgrab

pub fn glyphgrab(app: &Rc<App>) -> Cards {
    let grab = w::button("Select area", Some("text-scan-2"), "suggested-action");
    grab.add_css_class("pill");
    let (card, body) =
        w::card_with_action("Grab text from the screen", "Select an area; its text is recognized and copied", &grab);
    let lang = adw::EntryRow::builder().title("Language (Tesseract code, e.g. eng, deu, eng+nep)").text("eng").build();
    let rows = boxed_list();
    rows.append(&lang);
    body.append(&rows);
    let (out, out_body) = w::card("Recognized text", "");
    out.set_visible(false);
    let view = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(12)
        .bottom_margin(12)
        .left_margin(12)
        .right_margin(12)
        .css_classes(["fk-code"])
        .build();
    out_body.append(
        &gtk::ScrolledWindow::builder().child(&view).min_content_height(160).css_classes(["fk-card", "flush"]).build(),
    );
    let copy = w::button("Copy again", Some("copy"), "");
    copy.set_halign(gtk::Align::Start);
    out_body.append(&copy);
    {
        let (a, buf) = (app.clone(), view.buffer());
        copy.connect_clicked(move |_| {
            util::copy(&buf.text(&buf.start_iter(), &buf.end_iter(), false));
            a.toast("Copied");
        });
    }
    let (a, out2) = (app.clone(), out.clone());
    grab.connect_clicked(move |b| {
        b.set_sensitive(false);
        let (b, a, buf, out) = (b.clone(), a.clone(), view.buffer(), out2.clone());
        let l = lang.text().trim().to_string();
        let l = if l.is_empty() { "eng".to_string() } else { l };
        util::background(
            move || util::cli_output(&["glyphgrab", "--no-copy", "--lang", &l]),
            move |r| {
                b.set_sensitive(true);
                match r {
                    Ok(text) => {
                        buf.set_text(&text);
                        out.set_visible(true);
                        util::copy(&text);
                        a.toast("Text copied");
                    }
                    Err(e) => a.toast(&e),
                }
            },
        );
    });
    vec![card.upcast(), out.upcast()]
}

// ------------------------------------------------------------------ Glance

fn nautilus_script() -> std::path::PathBuf {
    glib::user_data_dir().join("nautilus/scripts/Preview with Glance")
}

pub fn glance(app: &Rc<App>) -> Cards {
    let zone = vbox(10);
    zone.add_css_class("fk-dropzone");
    zone.append(&w::empty_state(
        "eye",
        "Drop a file here to preview it",
        "Images, video, audio, text and code, PDFs, archives and folders.",
    ));
    let choose = w::button("Choose a file…", Some("folder-open"), "suggested-action");
    choose.add_css_class("pill");
    choose.set_halign(gtk::Align::Center);
    zone.append(&choose);
    let open = {
        let win = app.window.clone();
        Rc::new(move |v: Vec<std::path::PathBuf>| {
            if let (Some(p), Some(a)) = (v.first(), win.application().and_downcast::<adw::Application>()) {
                crate::glance::open(&a, p);
            }
        })
    };
    {
        let open = open.clone();
        choose.connect_clicked(move |b| {
            let open = open.clone();
            util::pick_files(b, false, false, move |v| open(v));
        });
    }
    let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    {
        let z = zone.clone();
        target.connect_enter(move |_, _, _| {
            z.add_css_class("drop-active");
            gdk::DragAction::COPY
        });
        let z = zone.clone();
        target.connect_leave(move |_| z.remove_css_class("drop-active"));
        let z = zone.clone();
        target.connect_drop(move |_, v, _, _| {
            z.remove_css_class("drop-active");
            let Ok(list) = v.get::<gdk::FileList>() else { return false };
            open(list.files().iter().filter_map(|f| f.path()).collect());
            true
        });
    }
    zone.add_controller(target);

    let (card, body) = w::card("Use it from your file manager", "");
    let rows = boxed_list();
    let script = adw::SwitchRow::builder()
        .title("Add to Files (Nautilus)")
        .subtitle("Right-click a file → Scripts → Preview with Glance")
        .active(nautilus_script().exists())
        .build();
    let a = app.clone();
    script.connect_active_notify(move |r| {
        let p = nautilus_script();
        let res = if r.is_active() {
            std::fs::create_dir_all(p.parent().unwrap()).and_then(|_| {
                std::fs::write(&p, "#!/bin/sh\n# Added by Forgekit\nexec forgekit glance \"$1\"\n")?;
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))
            })
        } else {
            std::fs::remove_file(&p)
        };
        if let Err(e) = res {
            a.toast(&format!("{e}"));
        }
    });
    rows.append(&script);
    let cmd = adw::ActionRow::builder().title("From a terminal or shortcut").build();
    let a = app.clone();
    cmd.add_suffix(&w::command_chip("forgekit glance FILE", move || a.toast("Command copied")));
    rows.append(&cmd);
    body.append(&rows);
    vec![zone.upcast(), card.upcast()]
}
