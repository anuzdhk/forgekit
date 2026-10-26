//! The Forgekit app.
//!
//! `forgekit-gtk [tool]` opens the main window (on a tool's page),
//! `forgekit-gtk --glance FILE` previews a file, and
//! `forgekit-gtk --shelf [FILE…]` opens DropShelf, and `--stage`,
//! `--caliper` and `--cheatsheet` open those tools' windows. One instance handles all of
//! them, so a second `--shelf FILE` adds to the shelf that's already open.

mod caliper;
mod cheatsheet;
mod freeze;
mod glance;
mod home;
mod pages;
mod settings_page;
mod shelf;
mod shortcut;
mod stage;
mod theme;
mod util;
mod widgets;
mod window;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

pub const APP_ID: &str = "io.github.anuzdhk.Forgekit";
/// Above GTK's per-user stylesheet (~/.config/gtk-4.0/gtk.css, where system
/// themes like WhiteSur install themselves), so Forgekit keeps its own look.
const BASE_PRIORITY: u32 = gtk::STYLE_PROVIDER_PRIORITY_USER + 1;
const RESOURCES: &str = "/io/github/anuzdhk/Forgekit";

fn main() -> glib::ExitCode {
    quiet_gtk_baseline_warnings();
    gio::resources_register_include!("forgekit.gresource").expect("bundled resources");
    let app =
        adw::Application::builder().application_id(APP_ID).flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE).build();
    let state: Rc<RefCell<Option<Rc<window::App>>>> = Rc::default();
    app.connect_startup(|_| setup_display());
    app.connect_command_line(move |app, cmd| {
        let args: Vec<String> = cmd.arguments().iter().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
        let cwd = cmd.cwd().unwrap_or_default();
        let abs = |p: &String| -> PathBuf {
            let p = PathBuf::from(p);
            if p.is_absolute() { p } else { cwd.join(p) }
        };
        match args.first().map(String::as_str) {
            Some("--glance") => match args.get(1) {
                Some(f) => glance::open(app, &abs(f)),
                None => eprintln!("usage: forgekit-gtk --glance FILE"),
            },
            Some("--shelf") => {
                let auto = args.iter().any(|a| a == "--auto");
                shelf::open(app, args[1..].iter().filter(|a| !a.starts_with("--")).map(abs).collect(), auto)
            }
            Some("--shelf-dismiss") => shelf::dismiss(),
            Some("--stage") => stage::open(app),
            Some("--caliper") => caliper::open(app),
            Some("--cheatsheet") => cheatsheet::open(app),
            first => {
                let main = state.borrow_mut().get_or_insert_with(|| window::App::new(app)).clone();
                if let Some(id) = first.and_then(forgekit_core::catalog::find).map(|t| t.id) {
                    main.navigate(id);
                } else if first == Some("settings") {
                    main.navigate("settings");
                }
                main.window.present();
            }
        }
        0.into()
    });
    app.run()
}

/// GTK 4.22 warns "GtkImage … reported baselines of minimum -2147483648" for
/// ordinary icons inside boxes. It's harmless but floods the terminal, so drop
/// exactly that message and pass every other GTK warning through.
fn quiet_gtk_baseline_warnings() {
    // GTK 4 logs through GLib's structured writer, so filter there.
    glib::log_set_writer_func(|level, fields| {
        let noise = fields.iter().any(|f| {
            f.key() == "MESSAGE"
                && f.value_str().is_some_and(|m| m.starts_with("GtkImage") && m.contains("reported baselines"))
        });
        if noise { glib::LogWriterOutput::Handled } else { glib::log_writer_default(level, fields) }
    });
}

/// Icons, base stylesheet and the saved theme.
fn setup_display() {
    let display = gdk::Display::default().expect("a display");
    gtk::IconTheme::for_display(&display).add_resource_path(&format!("{RESOURCES}/icons"));
    let base = gtk::CssProvider::new();
    base.load_from_resource(&format!("{RESOURCES}/style.css"));
    gtk::style_context_add_provider_for_display(&display, &base, BASE_PRIORITY);
    apply_theme(&theme::Appearance::load());
    // Keep the desktop extension pointed at this copy of the app.
    if forgekit_core::desktop::state() != forgekit_core::desktop::State::NotInstalled {
        forgekit_core::desktop::write_config(Some(&app_path()));
    }
}

/// This program's path, recorded for the GNOME extension and shortcuts.
pub fn app_path() -> String {
    std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()
}

thread_local! {
    static THEME_CSS: gtk::CssProvider = gtk::CssProvider::new();
}

/// Apply a theme to every window.
pub fn apply_theme(a: &theme::Appearance) {
    let t = theme::find(&a.theme);
    let style = adw::StyleManager::default();
    // Our own light/dark, whatever the desktop prefers.
    style.set_color_scheme(if t.palette.dark { adw::ColorScheme::ForceDark } else { adw::ColorScheme::ForceLight });
    THEME_CSS.with(|p| {
        p.load_from_string(&theme::css(t, a));
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&display, p, BASE_PRIORITY + 1);
        }
    });
}
