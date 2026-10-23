//! Glance: a quick-look window. Space or Escape closes it.

use std::path::Path;

use adw::prelude::*;
use forgekit_core::tools::glance::{self, Kind};
use gtk::{gdk, gio, glib};

use crate::util;
use crate::widgets::{self as w, hbox, vbox};

const TEXT_LIMIT: usize = 512 * 1024;

pub fn open(app: &adw::Application, path: &Path) {
    let kind = glance::kind(path);
    let meta = std::fs::metadata(path).ok();
    let size = meta.as_ref().filter(|m| m.is_file()).map(|m| glance::human_size(m.len()));
    let kind_name = match kind {
        Kind::Image => "Image",
        Kind::Video => "Video",
        Kind::Audio => "Audio",
        Kind::Text => "Text",
        Kind::Pdf => "PDF",
        Kind::Archive => "Archive",
        Kind::Folder => "Folder",
        Kind::Other => "File",
    };
    let subtitle = match size {
        Some(s) => format!("{kind_name} · {s}"),
        None => kind_name.to_string(),
    };

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(util::file_name(path))
        .default_width(if kind == Kind::Audio { 520 } else { 920 })
        .default_height(if kind == Kind::Audio { 260 } else { 680 })
        .build();
    let header =
        adw::HeaderBar::builder().title_widget(&adw::WindowTitle::new(&util::file_name(path), &subtitle)).build();
    let open_btn = w::button("Open", Some("external-link"), "suggested-action");
    let reveal = w::icon_button("folder-open", "Show in folder");
    let copy_path = w::icon_button("copy", "Copy path");
    header.pack_end(&open_btn);
    header.pack_start(&reveal);
    header.pack_start(&copy_path);
    {
        let p = path.to_path_buf();
        let win = window.clone();
        open_btn.connect_clicked(move |_| {
            util::open_path(&p);
            win.close();
        });
        let p = path.to_path_buf();
        reveal.connect_clicked(move |_| util::reveal(&p));
        let p = path.to_path_buf();
        copy_path.connect_clicked(move |_| util::copy(&p.display().to_string()));
    }

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
        Kind::Video | Kind::Audio => {
            let v = gtk::Video::for_file(Some(&gio::File::for_path(path)));
            v.set_autoplay(true);
            v.upcast()
        }
        Kind::Text => text_view(path),
        Kind::Folder => list_view(
            glance::list_folder(path, 2000)
                .map(|v| v.into_iter().map(|(n, d)| (n, if d { "folder" } else { "file" })).collect())
                .map_err(|e| e.to_string()),
        ),
        Kind::Archive => list_view(
            glance::list_archive(path)
                .map(|v| v.into_iter().map(|n| (n, "file-zip")).collect())
                .map_err(|e| format!("{e:#}")),
        ),
        Kind::Pdf => pdf_view(path),
        Kind::Other => details(path, meta.as_ref()),
    };

    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&content));
    window.set_content(Some(&view));

    // Space and Escape close, like Quick Look.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let win = window.clone();
    let is_text = kind == Kind::Text;
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape || (key == gdk::Key::space && !is_text) {
            win.close();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(keys);
    window.present();
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

fn list_view(items: Result<Vec<(String, &'static str)>, String>) -> gtk::Widget {
    match items {
        Err(e) => w::empty_state("alert-triangle", "Can't show the contents", &e).upcast(),
        Ok(v) if v.is_empty() => w::empty_state("folder", "Empty", "").upcast(),
        Ok(v) => {
            let list = gtk::ListBox::builder()
                .css_classes(["navigation-sidebar"])
                .selection_mode(gtk::SelectionMode::None)
                .build();
            let n = v.len();
            for (name, icon) in v {
                let r = hbox(10);
                r.set_margin_top(2);
                r.set_margin_bottom(2);
                r.append(&w::icon(icon, 16));
                r.append(
                    &gtk::Label::builder()
                        .label(&name)
                        .xalign(0.0)
                        .ellipsize(gtk::pango::EllipsizeMode::Middle)
                        .build(),
                );
                list.append(&r);
            }
            let b = vbox(0);
            let count = w::label(&format!("{n} items"), &["dim", "caption"]);
            count.set_margin_start(16);
            count.set_margin_top(8);
            b.append(&count);
            b.append(&gtk::ScrolledWindow::builder().child(&list).vexpand(true).build());
            b.upcast()
        }
    }
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
