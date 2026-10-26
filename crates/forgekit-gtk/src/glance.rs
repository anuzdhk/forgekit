//! Glance: a quick look at pictures and documents. Space or Escape closes it.
//!
//! One window is reused, so the Files app can step through a folder with the
//! arrow keys (see `previewer.rs`) without windows piling up.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use forgekit_core::tools::glance::{self, Kind};
use gtk::{gdk, gio, glib};

use crate::util;
use crate::widgets::{self as w, vbox};

const TEXT_LIMIT: usize = 512 * 1024;

/// Only pictures and documents get a preview.
pub fn previewable(path: &Path) -> bool {
    path.is_file() && matches!(glance::kind(path), Kind::Image | Kind::Pdf | Kind::Text)
}

struct Glance {
    window: adw::ApplicationWindow,
    title: adw::WindowTitle,
    view: adw::ToolbarView,
    path: RefCell<PathBuf>,
    kind: RefCell<Kind>,
}

type Hook<T> = RefCell<Option<Box<dyn Fn(T)>>>;

thread_local! {
    static GLANCE: RefCell<Option<Rc<Glance>>> = const { RefCell::new(None) };
    /// Arrow keys in the window (the previewer tells Files to move its selection).
    static ON_ARROW: Hook<gtk::DirectionType> = RefCell::new(None);
    /// The window was shown or hidden.
    static ON_VISIBLE: Hook<bool> = RefCell::new(None);
}

pub fn on_arrow(f: impl Fn(gtk::DirectionType) + 'static) {
    ON_ARROW.set(Some(Box::new(f)));
}

pub fn on_visible(f: impl Fn(bool) + 'static) {
    ON_VISIBLE.set(Some(Box::new(f)));
}

fn notify_visible(v: bool) {
    ON_VISIBLE.with_borrow(|f| {
        if let Some(f) = f {
            f(v)
        }
    });
}

pub fn is_visible() -> bool {
    GLANCE.with_borrow(|g| g.as_ref().is_some_and(|g| g.window.is_visible()))
}

pub fn close() {
    if let Some(g) = GLANCE.with_borrow(|g| g.clone()) {
        g.window.close();
    }
}

/// Preview `path`, replacing what the window shows. `false` (and nothing
/// shown) for files that aren't pictures or documents.
pub fn open(app: &adw::Application, path: &Path) -> bool {
    if !previewable(path) {
        return false;
    }
    let g = GLANCE.with_borrow(|g| g.clone()).unwrap_or_else(|| {
        let g = build(app);
        GLANCE.set(Some(g.clone()));
        g
    });
    let kind = glance::kind(path);
    let size = std::fs::metadata(path).ok().map(|m| glance::human_size(m.len()));
    let kind_name = match kind {
        Kind::Image => "Image",
        Kind::Pdf => "PDF",
        _ => "Text",
    };
    g.title.set_title(&util::file_name(path));
    g.title.set_subtitle(&match size {
        Some(s) => format!("{kind_name} · {s}"),
        None => kind_name.to_string(),
    });
    g.window.set_title(Some(&util::file_name(path)));
    let content: gtk::Widget = match kind {
        Kind::Image => gtk::Picture::builder()
            .file(&gio::File::for_path(path))
            .content_fit(gtk::ContentFit::Contain)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build()
            .upcast(),
        Kind::Pdf => pdf_view(path),
        _ => text_view(path),
    };
    g.view.set_content(Some(&content));
    *g.path.borrow_mut() = path.to_path_buf();
    *g.kind.borrow_mut() = kind;
    let was_visible = g.window.is_visible();
    g.window.present();
    if !was_visible {
        notify_visible(true);
    }
    true
}

fn build(app: &adw::Application) -> Rc<Glance> {
    let window = adw::ApplicationWindow::builder().application(app).default_width(920).default_height(680).build();
    let title = adw::WindowTitle::new("", "");
    let header = adw::HeaderBar::builder().title_widget(&title).build();
    let open_btn = w::button("Open", Some("external-link"), "suggested-action");
    let reveal = w::icon_button("folder-open", "Show in folder");
    let copy_path = w::icon_button("copy", "Copy path");
    header.pack_end(&open_btn);
    header.pack_start(&reveal);
    header.pack_start(&copy_path);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    window.set_content(Some(&view));
    let g = Rc::new(Glance { window, title, view, path: RefCell::default(), kind: RefCell::new(Kind::Other) });
    {
        let wg = Rc::downgrade(&g);
        open_btn.connect_clicked(move |_| {
            if let Some(g) = wg.upgrade() {
                util::open_path(&g.path.borrow());
                g.window.close();
            }
        });
        let wg = Rc::downgrade(&g);
        reveal.connect_clicked(move |_| {
            if let Some(g) = wg.upgrade() {
                util::reveal(&g.path.borrow());
            }
        });
        let wg = Rc::downgrade(&g);
        copy_path.connect_clicked(move |_| {
            if let Some(g) = wg.upgrade() {
                util::copy(&g.path.borrow().display().to_string());
            }
        });
    }

    // Space and Escape close, like Quick Look; arrows step through the folder.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let wg = Rc::downgrade(&g);
    keys.connect_key_pressed(move |_, key, _, _| {
        let Some(g) = wg.upgrade() else { return glib::Propagation::Proceed };
        let is_text = *g.kind.borrow() == Kind::Text;
        if key == gdk::Key::Escape || (key == gdk::Key::space && !is_text) {
            g.window.close();
            return glib::Propagation::Stop;
        }
        let dir = match key {
            gdk::Key::Left if !is_text => gtk::DirectionType::Left,
            gdk::Key::Right if !is_text => gtk::DirectionType::Right,
            gdk::Key::Up if !is_text => gtk::DirectionType::Up,
            gdk::Key::Down if !is_text => gtk::DirectionType::Down,
            _ => return glib::Propagation::Proceed,
        };
        let handled = ON_ARROW.with_borrow(|f| f.as_ref().map(|f| f(dir)).is_some());
        if handled { glib::Propagation::Stop } else { glib::Propagation::Proceed }
    });
    g.window.add_controller(keys);
    // Closing hides, so the next file opens instantly in the same window.
    g.window.connect_close_request(|win| {
        win.set_visible(false);
        notify_visible(false);
        glib::Propagation::Stop
    });
    g
}

fn text_view(path: &Path) -> gtk::Widget {
    let (text, truncated) = glance::read_text(path, TEXT_LIMIT).unwrap_or_else(|e| (e.to_string(), false));
    let view = gtk::TextView::builder()
        .editable(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::None)
        .top_margin(14)
        .bottom_margin(14)
        .left_margin(16)
        .right_margin(16)
        .css_classes(["fk-code"])
        .build();
    view.buffer().set_text(&text);
    let b = vbox(0);
    if truncated {
        let banner = adw::Banner::builder().title("Showing the first 512 KB").revealed(true).build();
        b.append(&banner);
    }
    b.append(&gtk::ScrolledWindow::builder().child(&view).vexpand(true).build());
    b.upcast()
}

/// First page through `pdftoppm` when it's installed; otherwise details.
fn pdf_view(path: &Path) -> gtk::Widget {
    let out = glib::user_cache_dir().join("forgekit/glance");
    let _ = std::fs::create_dir_all(&out);
    let stem = out.join("pdf-page");
    let ok = std::process::Command::new("pdftoppm")
        .args(["-png", "-r", "110", "-f", "1", "-l", "1", "-singlefile"])
        .arg(path)
        .arg(&stem)
        .status()
        .is_ok_and(|s| s.success());
    let png = stem.with_extension("png");
    if ok && png.exists() {
        // Load into memory so the temp file can be reused for the next PDF.
        if let Ok(t) = gdk::Texture::from_filename(&png) {
            return gtk::Picture::builder()
                .paintable(&t)
                .content_fit(gtk::ContentFit::Contain)
                .margin_top(12)
                .margin_bottom(12)
                .build()
                .upcast();
        }
    }
    details(path, std::fs::metadata(path).ok().as_ref())
}

fn details(path: &Path, meta: Option<&std::fs::Metadata>) -> gtk::Widget {
    let b = vbox(16);
    b.set_valign(gtk::Align::Center);
    b.set_halign(gtk::Align::Center);
    let ctype = gio::content_type_guess(Some(path), None).0;
    let desc = gio::content_type_get_description(&ctype);
    let icon = gtk::Image::from_gicon(&gio::content_type_get_icon(&ctype));
    icon.set_pixel_size(96);
    b.append(&icon);
    b.append(
        &gtk::Label::builder()
            .label(util::file_name(path))
            .css_classes(["title-2"])
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build(),
    );
    let mut lines = vec![desc.to_string()];
    if let Some(m) = meta {
        lines.push(glance::human_size(m.len()));
        if let Ok(t) = m.modified() {
            let dt: chrono::DateTime<chrono::Local> = t.into();
            lines.push(format!("Modified {}", dt.format("%e %b %Y, %H:%M")));
        }
    }
    b.append(&gtk::Label::builder().label(lines.join(" · ")).css_classes(["dim"]).build());
    b.upcast()
}
