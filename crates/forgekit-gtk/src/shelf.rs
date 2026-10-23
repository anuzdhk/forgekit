//! DropShelf: a small window that holds files, text and links while you move
//! between apps. Drop things on it; drag them out (one, or all at once).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use forgekit_core::tools::shelf::{Item, Settings, Shelf};
use gtk::{gdk, gio, glib};

use crate::util;
use crate::widgets::{self as w, hbox, vbox};

struct ShelfWin {
    window: adw::ApplicationWindow,
    shelf: RefCell<Shelf>,
    list: gtk::ListBox,
    empty: gtk::Box,
    drag_all: gtk::Box,
    count: gtk::Label,
    toasts: adw::ToastOverlay,
    /// Opened by a drag and nothing dropped yet: close again when the drag ends.
    auto: std::cell::Cell<bool>,
}

thread_local! {
    static OPEN: RefCell<Option<Rc<ShelfWin>>> = const { RefCell::new(None) };
}

const PACKAGES: &[&str] = &["deb", "rpm", "appimage", "flatpakref"];
const IMAGES: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff", "gif"];

fn ext(p: &std::path::Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// Open (or raise) the shelf. `auto` = opened because a drag started.
pub fn open(app: &adw::Application, files: Vec<PathBuf>, auto: bool) {
    let existing = OPEN.with_borrow(|o| o.clone());
    let s = match existing {
        Some(s) => s,
        None => {
            let s = build(app);
            s.auto.set(auto);
            OPEN.set(Some(s.clone()));
            s
        }
    };
    if !files.is_empty() {
        s.add_files(files);
    }
    s.window.present();
}

/// A drag that opened the shelf ended: close it unless something was dropped.
pub fn dismiss() {
    if let Some(s) = OPEN.with_borrow(|o| o.clone())
        && s.auto.get()
    {
        s.window.close();
    }
}

fn build(app: &adw::Application) -> Rc<ShelfWin> {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("DropShelf")
        .default_width(340)
        .default_height(440)
        .build();
    let header =
        adw::HeaderBar::builder().title_widget(&adw::WindowTitle::new("DropShelf", "Drop now, drag out later")).build();
    let clear = w::icon_button("trash", "Clear shelf");
    header.pack_start(&clear);

    let root = vbox(10);
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.set_margin_bottom(12);
    root.set_margin_top(4);

    let drag_all = hbox(10);
    drag_all.add_css_class("fk-quick");
    drag_all.append(&w::icon("drag-drop", 20));
    let count = w::label("", &["fk-tile-title"]);
    count.set_hexpand(true);
    drag_all.append(&count);
    drag_all.append(&w::label("drag all", &["dim", "caption"]));
    drag_all.set_tooltip_text(Some("Drag this to move everything at once"));
    root.append(&drag_all);

    let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["fk-table"]).build();
    let scroll =
        gtk::ScrolledWindow::builder().child(&list).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
    root.append(&scroll);

    let empty = vbox(0);
    empty.add_css_class("fk-dropzone");
    empty.set_vexpand(true);
    empty.set_valign(gtk::Align::Fill);
    let e = w::empty_state(
        "drag-drop",
        "Drop files here",
        "Files, folders, text and links. They stay here until you drag them out.",
    );
    e.set_vexpand(true);
    e.set_valign(gtk::Align::Center);
    empty.append(&e);
    root.append(&empty);

    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&root));
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&toasts));
    window.set_content(Some(&view));

    let s = Rc::new(ShelfWin {
        window,
        shelf: RefCell::new(Shelf::load()),
        list,
        empty,
        drag_all,
        count,
        toasts,
        auto: std::cell::Cell::new(false),
    });

    // Drops: files, or text (links become link items).
    let target = gtk::DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY);
    target.set_types(&[gdk::FileList::static_type(), glib::Type::STRING]);
    {
        let z = s.empty.clone();
        target.connect_enter(move |_, _, _| {
            z.add_css_class("drop-active");
            gdk::DragAction::COPY
        });
        let z = s.empty.clone();
        target.connect_leave(move |_| z.remove_css_class("drop-active"));
        let weak = Rc::downgrade(&s);
        target.connect_drop(move |_, value, _, _| {
            let Some(s) = weak.upgrade() else { return false };
            s.empty.remove_css_class("drop-active");
            if let Ok(list) = value.get::<gdk::FileList>() {
                s.add_files(list.files().iter().filter_map(|f| f.path()).collect());
                return true;
            }
            if let Ok(text) = value.get::<String>() {
                s.shelf.borrow_mut().add(Item::from_text(&text));
                s.changed();
                return true;
            }
            false
        });
    }
    s.window.add_controller(target);

    // Drag everything out at once.
    let all = gtk::DragSource::builder().actions(gdk::DragAction::COPY).build();
    {
        let weak = Rc::downgrade(&s);
        all.connect_prepare(move |_, _, _| {
            let s = weak.upgrade()?;
            let files: Vec<gio::File> = s
                .shelf
                .borrow()
                .items
                .iter()
                .filter_map(|i| match i {
                    Item::File { path, .. } if path.exists() => Some(gio::File::for_path(path)),
                    _ => None,
                })
                .collect();
            if files.is_empty() {
                return None;
            }
            Some(gdk::ContentProvider::for_value(&gdk::FileList::from_array(&files).to_value()))
        });
    }
    s.drag_all.add_controller(all);

    {
        let weak = Rc::downgrade(&s);
        clear.connect_clicked(move |_| {
            if let Some(s) = weak.upgrade() {
                s.shelf.borrow_mut().clear();
                s.changed();
            }
        });
    }
    // Re-check for moved/deleted files whenever the shelf comes back into focus.
    {
        let weak = Rc::downgrade(&s);
        s.window.connect_is_active_notify(move |w| {
            if w.is_active()
                && let Some(s) = weak.upgrade()
            {
                s.render();
            }
        });
    }
    s.window.connect_close_request(|_| {
        OPEN.set(None);
        glib::Propagation::Proceed
    });
    s.render();
    s
}

impl ShelfWin {
    fn toast(&self, m: &str) {
        self.toasts.add_toast(adw::Toast::new(m));
    }

    fn add_files(self: &Rc<Self>, files: Vec<PathBuf>) {
        let copy = Settings::load().unwrap_or_default().copy_files;
        let r = self.shelf.borrow_mut().add_files(&files, copy);
        match r {
            Ok(0) => self.toast("Already on the shelf"),
            Ok(_) => {}
            Err(e) => self.toast(&format!("{e:#}")),
        }
        self.changed();
    }

    fn changed(self: &Rc<Self>) {
        // Something was dropped or changed: the shelf stays open now.
        self.auto.set(false);
        if let Err(e) = self.shelf.borrow().save() {
            self.toast(&format!("Couldn't save the shelf: {e:#}"));
        }
        self.render();
    }

    fn render(self: &Rc<Self>) {
        self.list.remove_all();
        let items = self.shelf.borrow().items.clone();
        self.empty.set_visible(items.is_empty());
        self.list.set_visible(!items.is_empty());
        self.drag_all.set_visible(items.iter().filter(|i| matches!(i, Item::File { .. })).count() > 1);
        self.count.set_label(&format!("{} items", items.len()));
        for (i, item) in items.into_iter().enumerate() {
            self.list.append(&self.row(i, item));
        }
    }

    fn row(self: &Rc<Self>, index: usize, item: Item) -> gtk::ListBoxRow {
        let b = hbox(10);
        b.add_css_class("fk-shelf-item");
        let thumb: gtk::Widget = match &item {
            Item::File { path, .. } if IMAGES.contains(&ext(path).as_str()) => match util::thumbnail_file(path, 96) {
                Some(t) => util::thumb_widget(&t, 40).upcast(),
                None => w::icon_tile("photo", false, "").upcast(),
            },
            Item::File { path, .. } if path.is_dir() => w::icon_tile("folder", false, "").upcast(),
            Item::File { path, .. } if PACKAGES.contains(&ext(path).as_str()) => {
                w::icon_tile("package", false, "").upcast()
            }
            Item::File { .. } => w::icon_tile("file", false, "off").upcast(),
            Item::Text { .. } => w::icon_tile("file-text", false, "off").upcast(),
            Item::Link { .. } => w::icon_tile("link", false, "").upcast(),
        };
        b.append(&thumb);
        let t = vbox(2);
        t.set_hexpand(true);
        t.append(
            &gtk::Label::builder()
                .label(item.label())
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::Middle)
                .css_classes(["fk-tile-title"])
                .build(),
        );
        let sub = match &item {
            _ if item.is_missing() => w::label("Missing: moved or deleted", &["fk-missing"]),
            Item::File { path, copied } => {
                let size = std::fs::metadata(path)
                    .ok()
                    .filter(|m| m.is_file())
                    .map(|m| forgekit_core::tools::glance::human_size(m.len()));
                let mut s = size.unwrap_or_else(|| "Folder".into());
                if *copied {
                    s.push_str(" · copy");
                }
                w::label(&s, &["dim", "caption"])
            }
            Item::Text { text } => {
                w::label(&format!("Text · {} characters", text.chars().count()), &["dim", "caption"])
            }
            Item::Link { .. } => w::label("Link", &["dim", "caption"]),
        };
        t.append(&sub);
        b.append(&t);
        b.append(&self.menu(index, &item));
        let row = gtk::ListBoxRow::builder().child(&b).activatable(false).build();

        // Drag this item out.
        let drag = gtk::DragSource::builder().actions(gdk::DragAction::COPY).build();
        let it = item.clone();
        drag.connect_prepare(move |_, _, _| {
            Some(match &it {
                Item::File { path, .. } if path.exists() => {
                    gdk::ContentProvider::for_value(&gdk::FileList::from_array(&[gio::File::for_path(path)]).to_value())
                }
                Item::File { .. } => return None,
                Item::Text { text } => gdk::ContentProvider::for_value(&text.to_value()),
                Item::Link { url } => gdk::ContentProvider::for_value(&url.to_value()),
            })
        });
        let th = thumb.clone();
        drag.connect_drag_begin(move |src, _| {
            let paintable = gtk::WidgetPaintable::new(Some(&th));
            src.set_icon(Some(&paintable), 16, 16);
        });
        row.add_controller(drag);
        row
    }

    fn menu(self: &Rc<Self>, index: usize, item: &Item) -> gtk::MenuButton {
        let pop_box = vbox(2);
        let pop = gtk::Popover::builder().child(&pop_box).build();
        let add = |label: &str, icon: &str, f: Box<dyn Fn()>| {
            let b = w::button(label, Some(icon), "flat");
            b.set_halign(gtk::Align::Fill);
            if let Some(c) = b.child().and_downcast::<adw::ButtonContent>() {
                c.set_halign(gtk::Align::Start);
            }
            let p = pop.clone();
            b.connect_clicked(move |_| {
                p.popdown();
                f();
            });
            pop_box.append(&b);
        };
        match item {
            Item::File { path, .. } => {
                let p = path.clone();
                add("Open", "external-link", Box::new(move || util::open_path(&p)));
                let p = path.clone();
                add("Show in folder", "folder-open", Box::new(move || util::reveal(&p)));
                let p = path.clone();
                add("Copy path", "copy", Box::new(move || util::copy(&p.display().to_string())));
                if PACKAGES.contains(&ext(path).as_str()) {
                    let p = path.clone();
                    add(
                        "Install with Teku",
                        "package",
                        Box::new(move || {
                            let _ = std::process::Command::new("teku").arg(&p).spawn();
                        }),
                    );
                }
                if IMAGES.contains(&ext(path).as_str()) {
                    let (p, me) = (path.clone(), Rc::downgrade(self));
                    add(
                        "Resize to 1920 px (Shrinkray)",
                        "photo-scan",
                        Box::new(move || {
                            let (p, me) = (p.clone(), me.clone());
                            let opts = forgekit_core::tools::shrinkray::Options {
                                width: Some(1920),
                                height: Some(1920),
                                ..Default::default()
                            };
                            util::background(
                                move || {
                                    forgekit_core::tools::shrinkray::process(&p, &opts).map_err(|e| format!("{e:#}"))
                                },
                                move |r| {
                                    let Some(me) = me.upgrade() else { return };
                                    match r {
                                        Ok(out) => {
                                            me.add_files(vec![out]);
                                            me.toast("Resized copy added to the shelf");
                                        }
                                        Err(e) => me.toast(&e),
                                    }
                                },
                            );
                        }),
                    );
                }
            }
            Item::Text { text } => {
                let t = text.clone();
                add("Copy text", "copy", Box::new(move || util::copy(&t)));
            }
            Item::Link { url } => {
                let u = url.clone();
                add("Open link", "external-link", Box::new(move || util::open_uri(&u)));
                let u = url.clone();
                add("Copy link", "copy", Box::new(move || util::copy(&u)));
            }
        }
        let me = Rc::downgrade(self);
        add(
            "Remove from shelf",
            "x",
            Box::new(move || {
                if let Some(me) = me.upgrade() {
                    me.shelf.borrow_mut().remove(index);
                    me.changed();
                }
            }),
        );
        gtk::MenuButton::builder()
            .icon_name(w::icon_name("dots-vertical"))
            .popover(&pop)
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .build()
    }
}
