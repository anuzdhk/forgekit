//! Spark as a Spotlight-style bar: a floating search box near the top middle
//! of the screen. Type, use the arrow keys, Enter runs the result, Esc (or
//! clicking elsewhere) puts it away.
//!
//! Closing only hides the window for a while, so pressing the shortcut again
//! shows it instantly instead of starting the app over.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use forgekit_core::tools::spark;
use gtk::{gdk, glib};

use crate::util;
use crate::widgets::{self as w, hbox, vbox};

/// The window title the GNOME extension looks for to place the bar.
pub const TITLE: &str = "Spark";
const MAX_RESULTS: usize = 8;
/// Hidden this long, the window goes away and the app can exit.
const KEEP_WARM: Duration = Duration::from_secs(10 * 60);

struct Popup {
    window: gtk::Window,
    entry: gtk::Entry,
    list: gtk::ListBox,
    hint: gtk::Label,
    apps: RefCell<Vec<spark::App>>,
    items: RefCell<Vec<spark::Item>>,
    expire: RefCell<Option<glib::SourceId>>,
}

thread_local! {
    static POPUP: RefCell<Option<Rc<Popup>>> = const { RefCell::new(None) };
}

/// Show the bar, or hide it when it's already up (pressing the shortcut twice).
pub fn toggle(app: &adw::Application) {
    let p = POPUP.with_borrow(|p| p.clone()).unwrap_or_else(|| {
        let p = build(app);
        POPUP.set(Some(p.clone()));
        p
    });
    if p.window.is_visible() {
        hide(&p);
    } else {
        show(&p);
    }
}

fn show(p: &Rc<Popup>) {
    if let Some(id) = p.expire.take() {
        id.remove();
    }
    *p.apps.borrow_mut() = spark::load_apps();
    p.entry.set_text("");
    refresh(p);
    p.window.present();
    p.entry.grab_focus();
}

fn hide(p: &Rc<Popup>) {
    p.window.set_visible(false);
    if let Some(id) = p.expire.take() {
        id.remove();
    }
    let id = glib::timeout_add_local_once(KEEP_WARM, || {
        if let Some(p) = POPUP.take() {
            p.expire.take();
            p.window.destroy();
        }
    });
    p.expire.replace(Some(id));
}

fn build(app: &adw::Application) -> Rc<Popup> {
    let window = gtk::Window::builder()
        .application(app)
        .title(TITLE)
        .decorated(false)
        .resizable(false)
        .default_width(700)
        .css_classes(["fk-spark"])
        .build();

    let panel = vbox(0);
    panel.add_css_class("fk-spark-panel");

    let bar = hbox(14);
    bar.add_css_class("fk-spark-bar");
    let mark = w::icon("bolt", 26);
    mark.add_css_class("fk-spark-mark");
    bar.append(&mark);
    let entry = gtk::Entry::builder()
        .placeholder_text("Search apps, do maths, convert units…")
        .hexpand(true)
        .css_classes(["fk-spark-entry"])
        .build();
    bar.append(&entry);
    panel.append(&bar);

    let list = gtk::ListBox::builder()
        .css_classes(["fk-spark-list"])
        .selection_mode(gtk::SelectionMode::Single)
        .visible(false)
        .build();
    panel.append(&list);

    let hint = w::label("", &["dim", "fk-spark-hint"]);
    hint.set_xalign(0.0);
    panel.append(&hint);
    window.set_child(Some(&panel));

    let p = Rc::new(Popup {
        window,
        entry,
        list,
        hint,
        apps: RefCell::default(),
        items: RefCell::default(),
        expire: RefCell::default(),
    });

    {
        let wp = Rc::downgrade(&p);
        p.entry.connect_changed(move |_| {
            if let Some(p) = wp.upgrade() {
                refresh(&p);
            }
        });
    }
    {
        let wp = Rc::downgrade(&p);
        p.entry.connect_activate(move |_| {
            if let Some(p) = wp.upgrade()
                && let Some(r) = p.list.selected_row()
            {
                run(&p, r.index() as usize);
            }
        });
    }
    {
        let wp = Rc::downgrade(&p);
        p.list.connect_row_activated(move |_, row| {
            if let Some(p) = wp.upgrade() {
                run(&p, row.index() as usize);
            }
        });
    }
    // Arrow keys move through results while typing; Esc puts the bar away.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let wp = Rc::downgrade(&p);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(p) = wp.upgrade() else { return glib::Propagation::Proceed };
            let step = match key {
                gdk::Key::Escape => {
                    hide(&p);
                    return glib::Propagation::Stop;
                }
                gdk::Key::Down | gdk::Key::Tab => 1,
                gdk::Key::Up | gdk::Key::ISO_Left_Tab => -1,
                _ => return glib::Propagation::Proceed,
            };
            let n = p.items.borrow().len() as i32;
            if n > 0 {
                let cur = p.list.selected_row().map_or(0, |r| r.index());
                if let Some(r) = p.list.row_at_index((cur + step).rem_euclid(n)) {
                    p.list.select_row(Some(&r));
                    r.grab_focus();
                    p.entry.grab_focus_without_selecting();
                }
            }
            glib::Propagation::Stop
        });
    }
    p.window.add_controller(keys);
    // Clicking another window dismisses it, like Spotlight.
    {
        let wp = Rc::downgrade(&p);
        p.window.connect_is_active_notify(move |win| {
            if !win.is_active()
                && win.is_visible()
                && let Some(p) = wp.upgrade()
            {
                hide(&p);
            }
        });
    }
    p.window.connect_close_request(move |win| {
        if let Some(p) = POPUP.with_borrow(|p| p.clone())
            && &p.window == win
        {
            hide(&p);
        }
        glib::Propagation::Stop
    });
    p
}

fn refresh(p: &Rc<Popup>) {
    p.list.remove_all();
    let q = p.entry.text();
    let mut items = spark::query(&q, &p.apps.borrow());
    items.truncate(MAX_RESULTS);
    for it in &items {
        p.list.append(&result_row(it));
    }
    p.list.set_visible(!items.is_empty());
    if let Some(first) = p.list.row_at_index(0) {
        p.list.select_row(Some(&first));
    }
    p.hint.set_text(if q.trim().is_empty() {
        "Try  firefox  ·  12*7  ·  10 km to mi  ·  install ~/Downloads/app.deb"
    } else {
        "↑ ↓ to choose  ·  Enter to open  ·  Esc to close"
    });
    *p.items.borrow_mut() = items;
}

fn result_row(it: &spark::Item) -> gtk::ListBoxRow {
    let row = hbox(14);
    row.add_css_class("fk-spark-row");
    let icon = match &it.action {
        spark::Action::Copy { .. } => w::icon("calculator", 22),
        spark::Action::Install { .. } => w::icon("package", 22),
        spark::Action::Open { .. } => w::icon("world", 22),
        spark::Action::Launch { .. } if it.icon.starts_with('/') => gtk::Image::from_file(&it.icon),
        spark::Action::Launch { .. } => gtk::Image::from_icon_name(&it.icon),
    };
    if matches!(it.action, spark::Action::Launch { .. }) {
        icon.set_pixel_size(32);
    } else {
        icon.add_css_class("fk-spark-glyph");
    }
    row.append(&icon);
    let text = vbox(1);
    text.set_valign(gtk::Align::Center);
    text.set_hexpand(true);
    let title = w::label(&it.title, &["fk-spark-title"]);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&title);
    if !it.subtitle.is_empty() {
        let sub = w::label(&it.subtitle, &["dim", "caption"]);
        sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&sub);
    }
    row.append(&text);
    gtk::ListBoxRow::builder().child(&row).build()
}

fn run(p: &Rc<Popup>, i: usize) {
    let Some(it) = p.items.borrow().get(i).cloned() else { return };
    hide(p);
    // Copying asks the desktop which clipboard tool to use, which can take a moment.
    util::background(
        move || spark::run(&it.action).map_err(|e| format!("{e:#}")),
        |r| {
            if let Err(e) = r {
                forgekit_core::tools::notify("Spark", &e);
            }
        },
    );
}
