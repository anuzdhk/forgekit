//! Settings: themes, accent, sidebar options, the background daemon, and
//! what Forgekit detected about this system.

use std::rc::Rc;

use adw::prelude::*;
use gtk::cairo;

use crate::theme::{self, Palette, THEMES};
use crate::util;
use crate::widgets::{self as w, hbox, vbox};
use crate::window::App;

pub fn build(app: &Rc<App>) -> gtk::Widget {
    let root = vbox(22);
    root.add_css_class("fk-page");
    root.append(&w::label("Settings", &["fk-page-title"]));
    root.append(&appearance(app));
    root.append(&behavior(app));
    root.append(&system(app));
    root.append(&about(app));
    w::page(&root).upcast()
}

fn hex(s: &str) -> (f64, f64, f64) {
    let h = s.trim_start_matches('#');
    let c = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("80"), 16).unwrap_or(128) as f64 / 255.0;
    (c(0), c(2), c(4))
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

/// A tiny window mock-up in the theme's colors.
fn preview_draw(p: Palette) -> impl Fn(&gtk::DrawingArea, &cairo::Context, i32, i32) {
    move |_, cr, width, height| {
        let (w, h) = (width as f64, height as f64);
        let fill = |c: &str| {
            let (r, g, b) = hex(c);
            cr.set_source_rgb(r, g, b);
        };
        rounded(cr, 0.0, 0.0, w, h, 9.0);
        cr.clip();
        fill(p.bg);
        let _ = cr.paint();
        fill(p.sidebar);
        cr.rectangle(0.0, 0.0, w * 0.3, h);
        let _ = cr.fill();
        fill(p.accent);
        rounded(cr, 6.0, 10.0, w * 0.3 - 12.0, 8.0, 4.0);
        let _ = cr.fill();
        fill(p.muted);
        for i in 0..3 {
            rounded(cr, 8.0, 26.0 + i as f64 * 11.0, w * 0.3 - 20.0, 5.0, 2.5);
            let _ = cr.fill();
        }
        fill(p.surface);
        rounded(cr, w * 0.36, 10.0, w * 0.6, h * 0.42, 6.0);
        let _ = cr.fill();
        rounded(cr, w * 0.36, h * 0.42 + 16.0, w * 0.6, h - h * 0.42 - 26.0, 6.0);
        let _ = cr.fill();
        fill(p.fg);
        rounded(cr, w * 0.41, 18.0, w * 0.3, 5.0, 2.5);
        let _ = cr.fill();
        fill(p.accent);
        rounded(cr, w * 0.41, 29.0, w * 0.18, 8.0, 4.0);
        let _ = cr.fill();
        fill(p.success);
        cr.arc(w * 0.88, 21.0, 3.5, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    }
}

fn appearance(app: &Rc<App>) -> gtk::Box {
    let (card, body) = w::card("Appearance", "Pick a theme. Changes apply instantly.");
    let current = app.appearance.borrow().clone();

    let gallery = gtk::FlowBox::builder()
        .homogeneous(true)
        .min_children_per_line(2)
        .max_children_per_line(6)
        .column_spacing(6)
        .row_spacing(6)
        .selection_mode(gtk::SelectionMode::None)
        .activate_on_single_click(true)
        .css_classes(["fk-themes"])
        .build();
    for t in THEMES {
        let c = vbox(6);
        c.add_css_class("fk-theme-card");
        if t.id == current.theme {
            c.add_css_class("selected");
        }
        let area =
            gtk::DrawingArea::builder().content_height(76).content_width(132).css_classes(["fk-theme-preview"]).build();
        area.set_draw_func(preview_draw(t.palette));
        c.append(&area);
        let name = hbox(6);
        name.set_halign(gtk::Align::Center);
        if t.id == current.theme {
            name.append(&w::icon("check", 14));
        }
        name.append(&gtk::Label::builder().label(t.name).css_classes(["fk-theme-name"]).build());
        c.append(&name);
        gallery.append(&gtk::FlowBoxChild::builder().child(&c).name(t.id).tooltip_text(t.name).build());
    }
    {
        let a = app.clone();
        gallery.connect_child_activated(move |_, child| a.set_theme(child.widget_name().as_str()));
    }
    body.append(&gallery);

    let rows = gtk::ListBox::builder().css_classes(["boxed-list"]).selection_mode(gtk::SelectionMode::None).build();
    rows.set_margin_top(6);

    // Accent swatches
    let accent_row = adw::ActionRow::builder().title("Accent color").subtitle("Override the theme's accent").build();
    let swatches = hbox(6);
    swatches.set_valign(gtk::Align::Center);
    let options: Vec<(String, Option<String>)> = std::iter::once(("Theme default".to_string(), None))
        .chain(theme::ACCENTS.iter().map(|(n, c)| (n.to_string(), Some(c.to_string()))))
        .collect();
    for (name, color) in options {
        let area = gtk::DrawingArea::builder().content_width(22).content_height(22).build();
        let c2 = color.clone();
        area.set_draw_func(move |_, cr, wd, ht| {
            let (cx, cy, r) = (wd as f64 / 2.0, ht as f64 / 2.0, wd.min(ht) as f64 / 2.0);
            match &c2 {
                Some(c) => {
                    let (r_, g, b) = hex(c);
                    cr.set_source_rgb(r_, g, b);
                    cr.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
                    let _ = cr.fill();
                }
                None => {
                    // Rainbow-ish ring for "theme default".
                    for (i, c) in ["#ef4444", "#eab308", "#22c55e", "#3b82f6"].iter().enumerate() {
                        let (r_, g, b) = hex(c);
                        cr.set_source_rgb(r_, g, b);
                        let a0 = i as f64 * std::f64::consts::FRAC_PI_2;
                        cr.move_to(cx, cy);
                        cr.arc(cx, cy, r, a0, a0 + std::f64::consts::FRAC_PI_2);
                        let _ = cr.fill();
                    }
                }
            }
        });
        let b = gtk::Button::builder().child(&area).tooltip_text(&name).css_classes(["flat", "fk-swatch"]).build();
        if current.accent == color {
            b.add_css_class("selected");
        }
        let a = app.clone();
        b.connect_clicked(move |_| {
            {
                let mut ap = a.appearance.borrow_mut();
                ap.accent = color.clone();
                ap.save();
                crate::apply_theme(&ap);
            }
            a.rebuild_page("settings");
        });
        swatches.append(&b);
    }
    let sw = gtk::ScrolledWindow::builder()
        .child(&swatches)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_width(true)
        .build();
    accent_row.add_suffix(&sw);
    rows.append(&accent_row);

    // Glass opacity
    let glass = theme::find(&current.theme).glass;
    let op_row = adw::ActionRow::builder()
        .title("Glass opacity")
        .subtitle(if glass { "How see-through Glass themes are" } else { "Pick a Glass theme to use this" })
        .sensitive(glass)
        .build();
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.4, 1.0, 0.02);
    scale.set_value(current.opacity);
    scale.set_size_request(200, -1);
    scale.set_valign(gtk::Align::Center);
    {
        let a = app.clone();
        scale.connect_value_changed(move |s| {
            let mut ap = a.appearance.borrow_mut();
            ap.opacity = s.value();
            ap.save();
            crate::apply_theme(&ap);
        });
    }
    op_row.add_suffix(&scale);
    rows.append(&op_row);

    for (title, subtitle, get, set) in [
        (
            "Hide unavailable tools",
            "Tools this desktop can't run",
            current.hide_unavailable,
            (|a: &mut theme::Appearance, v| a.hide_unavailable = v) as fn(&mut theme::Appearance, bool),
        ),
        ("Hide upcoming tools", "Tools from later releases", current.hide_planned, |a, v| a.hide_planned = v),
    ] {
        let r = adw::SwitchRow::builder().title(title).subtitle(subtitle).active(get).build();
        let a = app.clone();
        r.connect_active_notify(move |r| {
            {
                let mut ap = a.appearance.borrow_mut();
                set(&mut ap, r.is_active());
                ap.save();
            }
            a.fill_nav();
            a.rebuild_page("home");
        });
        rows.append(&r);
    }
    body.append(&rows);
    card
}

fn autostart_file() -> std::path::PathBuf {
    forgekit_core::paths::config_dir()
        .parent()
        .unwrap_or(std::path::Path::new("/tmp"))
        .join("autostart/forgekitd.desktop")
}

fn daemon_path() -> Option<std::path::PathBuf> {
    let sibling = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("forgekitd")));
    sibling.filter(|p| p.exists()).or_else(|| forgekit_core::paths::which("forgekitd"))
}

pub fn daemon_running() -> bool {
    let Ok(conn) = zbus::blocking::Connection::session() else { return false };
    let Ok(proxy) = zbus::blocking::fdo::DBusProxy::new(&conn) else { return false };
    proxy.name_has_owner("org.forgekit.Daemon".try_into().unwrap()).unwrap_or(false)
}

pub fn start_daemon() -> Result<(), String> {
    let path = daemon_path().ok_or("forgekitd isn't installed")?;
    std::process::Command::new(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn behavior(app: &Rc<App>) -> gtk::Box {
    let (card, body) = w::card(
        "Background service",
        "forgekitd runs Clipstack, Dusk's schedule and Nightowl timers while the app is closed",
    );
    let rows = gtk::ListBox::builder().css_classes(["boxed-list"]).selection_mode(gtk::SelectionMode::None).build();

    let running = daemon_running();
    let status = adw::ActionRow::builder()
        .title(if running { "Running" } else { "Not running" })
        .subtitle(if running {
            "org.forgekit.Daemon is on the session bus"
        } else {
            "Start it to record clipboard history"
        })
        .build();
    status.add_prefix(&w::icon_tile(
        if running { "circle-check" } else { "alert-triangle" },
        false,
        if running { "ok" } else { "warn" },
    ));
    if !running {
        let start = w::button("Start", Some("player-play"), "suggested-action");
        let a = app.clone();
        start.connect_clicked(move |_| match start_daemon() {
            Ok(()) => {
                a.toast("Started forgekitd");
                let a = a.clone();
                gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(800), move || {
                    a.rebuild_page("settings")
                });
            }
            Err(e) => a.toast(&e),
        });
        status.add_suffix(&start);
    }
    rows.append(&status);

    let auto = adw::SwitchRow::builder()
        .title("Start at login")
        .subtitle("Adds forgekitd to your autostart folder")
        .active(autostart_file().exists())
        .build();
    let a = app.clone();
    auto.connect_active_notify(move |r| {
        let f = autostart_file();
        let res = if r.is_active() {
            let exec = daemon_path().map(|p| p.display().to_string()).unwrap_or_else(|| "forgekitd".into());
            std::fs::create_dir_all(f.parent().unwrap()).and_then(|_| {
                std::fs::write(
                    &f,
                    format!("[Desktop Entry]\nType=Application\nName=Forgekit service\nExec={exec}\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n"),
                )
            })
        } else {
            std::fs::remove_file(&f).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) })
        };
        if let Err(e) = res {
            a.toast(&format!("Couldn't update autostart: {e}"));
        }
    });
    rows.append(&auto);
    body.append(&rows);
    card
}

fn system(app: &Rc<App>) -> gtk::Box {
    let (card, body) = w::card("This system", "What Forgekit detected; tools use this to decide what works");
    let caps = &app.caps;
    let t = w::Table::new(&["Item", "Value"], &[180, -1]);
    let rows = [
        ("Session", format!("{:?}", caps.session)),
        ("Desktop", if caps.desktop_raw.is_empty() { "unknown".into() } else { caps.desktop_raw.clone() }),
        ("Portals", format!("{} available", caps.portals.len())),
        ("Privileged helper", if caps.helper_installed { "installed".into() } else { "not installed".into() }),
        (
            "Clipboard access",
            if caps.has_data_control() {
                "data-control protocol".into()
            } else if std::env::var_os("DISPLAY").is_some() {
                "through XWayland".into()
            } else {
                "none".into()
            },
        ),
        (
            "Helpers found",
            if caps.binaries.is_empty() {
                "none".into()
            } else {
                caps.binaries.iter().cloned().collect::<Vec<_>>().join(", ")
            },
        ),
    ];
    for (k, v) in rows {
        t.add(vec![w::cell(k, &["dim"]), w::cell(&v, &[])]);
    }
    body.append(&t.root);

    let portals = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(8)
        .row_spacing(6)
        .column_spacing(6)
        .build();
    for (name, v) in &caps.portals {
        portals.append(&w::pill(&format!("{name} v{v}"), "na"));
    }
    body.append(&portals);
    card
}

fn about(app: &Rc<App>) -> gtk::Box {
    let (card, body) = w::card("About", "");
    let top = hbox(14);
    let mark = gtk::Image::from_icon_name(crate::APP_ID);
    mark.set_pixel_size(56);
    mark.set_valign(gtk::Align::Center);
    top.append(&mark);
    let t = vbox(2);
    t.append(&w::label(&format!("Forgekit {}", env!("CARGO_PKG_VERSION")), &["fk-card-title"]));
    t.append(&w::label("All-in-one power tools for Linux · GPL-3.0-or-later", &["dim"]));
    top.append(&t);
    body.append(&top);
    let links = hbox(8);
    for (label, icon, url) in [
        ("Source code", "brand-github", "https://github.com/anuzdhk/forgekit"),
        ("Teku", "package", "https://github.com/anuzdhk/teku"),
        ("Icons by Tabler", "external-link", "https://tabler.io/icons"),
    ] {
        let b = w::button(label, Some(icon), "flat");
        b.connect_clicked(move |_| util::open_uri(url));
        links.append(&b);
    }
    body.append(&links);
    body.append(&updates(app));
    card
}

fn updates(app: &Rc<App>) -> gtk::ListBox {
    use forgekit_core::settings::General;
    use forgekit_core::update::{self, Outcome};
    let rows = gtk::ListBox::builder().css_classes(["boxed-list"]).selection_mode(gtk::SelectionMode::None).build();
    let auto = adw::SwitchRow::builder()
        .title("Check for updates")
        .subtitle("Once a day, a single anonymous request to GitHub")
        .active(General::load().unwrap_or_default().check_updates)
        .build();
    let a = app.clone();
    auto.connect_active_notify(move |r| {
        let mut g = General::load().unwrap_or_default();
        g.check_updates = r.is_active();
        if let Err(e) = g.save() {
            a.toast(&format!("{e:#}"));
        }
    });
    rows.append(&auto);
    let last = match update::last_checked() {
        Some(d) if d.as_secs() < 120 => "Last checked just now".to_string(),
        Some(d) if d.as_secs() < 7200 => format!("Last checked {} min ago", d.as_secs() / 60),
        Some(d) if d.as_secs() < 172800 => format!("Last checked {} h ago", d.as_secs() / 3600),
        Some(d) => format!("Last checked {} days ago", d.as_secs() / 86400),
        None => "Not checked yet".to_string(),
    };
    let now =
        adw::ActionRow::builder().title(format!("Forgekit {}", env!("CARGO_PKG_VERSION"))).subtitle(&last).build();
    let btn = w::button("Check now", Some("refresh"), "");
    let row = now.clone();
    let a = app.clone();
    btn.connect_clicked(move |b| {
        b.set_sensitive(false);
        let (b, row, a) = (b.clone(), row.clone(), a.clone());
        util::background(
            || update::check_now(env!("CARGO_PKG_VERSION")).map_err(|e| format!("{e:#}")),
            move |r| {
                b.set_sensitive(true);
                match r {
                    Ok(Outcome::UpToDate) => row.set_subtitle("You have the latest version"),
                    Ok(Outcome::NoReleases) => {
                        row.set_subtitle("No releases published yet; this is a development build")
                    }
                    Ok(Outcome::Available(rel)) => {
                        row.set_subtitle(&format!("Version {} is available", rel.version));
                        let url = rel.url.clone();
                        let open = w::button("Download", Some("download"), "suggested-action");
                        open.connect_clicked(move |_| util::open_uri(&url));
                        row.add_suffix(&open);
                    }
                    Err(e) => a.toast(&e),
                }
            },
        );
    });
    now.add_suffix(&btn);
    rows.append(&now);
    rows
}
