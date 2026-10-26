//! The clipboard pop-up (`forgekit-gtk --clipboard`): a small window with
//! recent clips, like Windows' Win+V. Click one to copy it back.
//!
//! On GNOME the top-bar menu from the Forgekit extension does this job; this
//! window is for desktops without it. Like Spark, closing only hides it for a
//! while so it reopens instantly.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use forgekit_core::tools::clipstack::{self, store::Store};
use gtk::{gdk, glib};

use crate::util;
use crate::widgets::{self as w, hbox, vbox};

pub const TITLE: &str = "Clipboard";
const KEEP_WARM: Duration = Duration::from_secs(10 * 60);

struct Popup {
    window: gtk::Window,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    empty: gtk::Box,
    expire: RefCell<Option<glib::SourceId>>,
    _sub: Option<gtk::gio::SignalSubscription>,
}

thread_local! {
    static POPUP: RefCell<Option<Rc<Popup>>> = const { RefCell::new(None) };
}

pub fn toggle(app: &adw::Application) {
    let p = POPUP.with_borrow(|p| p.clone()).unwrap_or_else(|| {
        let p = build(app);
        POPUP.set(Some(p.clone()));
        p
    });
    if p.window.is_visible() {
        hide(&p);
        return;
    }
    if let Some(id) = p.expire.take() {
        id.remove();
    }
    p.search.set_text("");
    fill(&p);
    p.window.present();
    p.search.grab_focus();
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
        .default_width(400)
        .default_height(540)
        .css_classes(["fk-spark", "fk-clip-popup"])
        .build();
    let panel = vbox(10);
    panel.add_css_class("fk-spark-panel");
    panel.add_css_class("fk-clip-panel");

    let head = hbox(8);
    let icon = w::icon("clipboard-list", 20);
    icon.add_css_class("fk-spark-mark");
    head.append(&icon);
    let title = w::label("Clipboard", &["fk-card-title"]);
    title.set_hexpand(true);
    head.append(&title);
    let clear = w::button("Clear all", None, "flat");
    head.append(&clear);
    panel.append(&head);

    let search = gtk::SearchEntry::builder().placeholder_text("Search…").build();
    panel.append(&search);

    let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).css_classes(["fk-clip-list"]).build();
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .min_content_height(420)
        .child(&list)
        .build();
    panel.append(&scroll);
    let empty = w::empty_state("clipboard-list", "Nothing copied yet", "Copy text or a picture and it shows up here.");
    empty.set_vexpand(true);
    panel.append(&empty);
    window.set_child(Some(&panel));

    let p = Rc::new_cyclic(|wp: &std::rc::Weak<Popup>| {
        let wp = wp.clone();
        Popup {
            window,
            search,
            list,
            empty,
            expire: RefCell::default(),
            _sub: util::on_clipstack_changed(move || {
                if let Some(p) = wp.upgrade()
                    && p.window.is_visible()
                {
                    fill(&p);
                }
            }),
        }
    });

    {
        let wp = Rc::downgrade(&p);
        p.search.connect_search_changed(move |_| {
            if let Some(p) = wp.upgrade() {
                fill(&p);
            }
        });
    }
    {
        let wp = Rc::downgrade(&p);
        p.search.connect_activate(move |_| {
            if let Some(p) = wp.upgrade()
                && let Some(r) = p.list.row_at_index(0)
            {
                r.activate();
            }
        });
    }
    {
        let wp = Rc::downgrade(&p);
        clear.connect_clicked(move |_| {
            if let Ok(s) = Store::open_default() {
                let _ = s.clear();
            }
            if let Some(p) = wp.upgrade() {
                fill(&p);
            }
        });
    }
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let wp = Rc::downgrade(&p);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape
                && let Some(p) = wp.upgrade()
            {
                hide(&p);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    p.window.add_controller(keys);
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
    p.window.connect_close_request(|_| {
        if let Some(p) = POPUP.with_borrow(|p| p.clone()) {
            hide(&p);
        }
        glib::Propagation::Stop
    });
    p
}

fn fill(p: &Rc<Popup>) {
    p.list.remove_all();
    let store = Store::open_default().ok();
    let items = store.as_ref().and_then(|s| s.list(&p.search.text(), 60).ok()).unwrap_or_default();
    p.empty.set_visible(items.is_empty());
    p.list.parent().and_then(|v| v.parent()).inspect(|s| s.set_visible(!items.is_empty()));
    for item in items {
        p.list.append(&row(p, store.as_ref(), item));
    }
}

fn row(p: &Rc<Popup>, store: Option<&Store>, item: clipstack::store::Item) -> gtk::ListBoxRow {
    let card = hbox(10);
    card.add_css_class("fk-clip-item");
    let body: gtk::Widget = match item.is_image() {
        true => match store.and_then(|s| s.get(item.id).ok().flatten()).and_then(|c| util::thumbnail(&c.data, 240)) {
            Some(t) => {
                let pic = gtk::Picture::for_paintable(&t);
                pic.set_content_fit(gtk::ContentFit::Cover);
                pic.set_size_request(-1, 110);
                pic.set_hexpand(true);
                pic.add_css_class("fk-thumb");
                pic.upcast()
            }
            None => w::label("Picture", &["dim"]).upcast(),
        },
        false => {
            let text: String = item.preview.split_whitespace().collect::<Vec<_>>().join(" ");
            gtk::Label::builder()
                .label(&text)
                .xalign(0.0)
                .hexpand(true)
                .lines(3)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build()
                .upcast()
        }
    };
    card.append(&body);
    let side = vbox(2);
    side.set_valign(gtk::Align::Start);
    let pin = w::icon_button(if item.pinned { "pinned" } else { "pin" }, if item.pinned { "Unpin" } else { "Pin" });
    if item.pinned {
        pin.add_css_class("accent");
    }
    let del = w::icon_button("x", "Remove");
    side.append(&pin);
    side.append(&del);
    card.append(&side);
    let row = gtk::ListBoxRow::builder().child(&card).activatable(true).build();

    let id = item.id;
    let pinned = item.pinned;
    {
        let wp = Rc::downgrade(p);
        pin.connect_clicked(move |_| {
            if let Ok(s) = Store::open_default() {
                let _ = s.set_pinned(id, !pinned);
            }
            if let Some(p) = wp.upgrade() {
                fill(&p);
            }
        });
    }
    {
        let wp = Rc::downgrade(p);
        del.connect_clicked(move |_| {
            if let Ok(s) = Store::open_default() {
                let _ = s.delete(id);
            }
            if let Some(p) = wp.upgrade() {
                fill(&p);
            }
        });
    }
    {
        let wp = Rc::downgrade(p);
        row.connect_activate(move |_| copy(&wp, id));
    }
    let wp = Rc::downgrade(p);
    let click = gtk::GestureClick::new();
    click.connect_released(move |g, _, x, y| {
        // The pin and remove buttons handle their own clicks.
        if let Some(w) = g.widget()
            && let Some(hit) = w.pick(x, y, gtk::PickFlags::DEFAULT)
            && hit.ancestor(gtk::Button::static_type()).is_some()
        {
            return;
        }
        copy(&wp, id);
    });
    row.add_controller(click);
    row
}

fn copy(wp: &std::rc::Weak<Popup>, id: i64) {
    let Some(p) = wp.upgrade() else { return };
    if util::clipstack_copy(id).is_err() {
        // No daemon: GTK holds it while the (hidden) window lives.
        let clip = Store::open_default().ok().and_then(|s| s.get(id).ok().flatten());
        if let (Some(c), Some(d)) = (clip, gdk::Display::default()) {
            if c.is_image() {
                if let Ok(t) = gdk::Texture::from_bytes(&glib::Bytes::from(&c.data)) {
                    d.clipboard().set_texture(&t);
                }
            } else {
                d.clipboard().set_text(&String::from_utf8_lossy(&c.data));
            }
        }
    }
    hide(&p);
}
