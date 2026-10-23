//! The Forgekit app.
//!
//! `forgekit-gtk [tool]` opens the main window (on a tool's page),
//! `forgekit-gtk --glance FILE` previews a file, and
//! `forgekit-gtk --shelf [FILE…]` opens DropShelf. One instance handles all of
//! them, so a second `--shelf FILE` adds to the shelf that's already open.

mod glance;
mod home;
mod pages;
mod settings_page;
mod shelf;
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
const RESOURCES: &str = "/io/github/anuzdhk/Forgekit";

fn main() -> glib::ExitCode {
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
            Some("--shelf") => shelf::open(app, args[1..].iter().map(abs).collect()),
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

/// Icons, base stylesheet and the saved theme.
fn setup_display() {
    let display = gdk::Display::default().expect("a display");
    gtk::IconTheme::for_display(&display).add_resource_path(&format!("{RESOURCES}/icons"));
    let base = gtk::CssProvider::new();
    base.load_from_resource(&format!("{RESOURCES}/style.css"));
    gtk::style_context_add_provider_for_display(&display, &base, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    apply_theme(&theme::Appearance::load());
}

thread_local! {
    static THEME_CSS: gtk::CssProvider = gtk::CssProvider::new();
}

/// Apply a theme to every window.
pub fn apply_theme(a: &theme::Appearance) {
    let t = theme::find(&a.theme);
    let style = adw::StyleManager::default();
    style.set_color_scheme(match t.palette {
        None => adw::ColorScheme::Default,
        Some(p) if p.dark => adw::ColorScheme::ForceDark,
        Some(_) => adw::ColorScheme::ForceLight,
    });
    THEME_CSS.with(|p| {
        p.load_from_string(&theme::css(t, a));
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&display, p, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
        }
    });
}
