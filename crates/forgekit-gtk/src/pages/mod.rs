//! Tool pages: a header card shared by every tool, then the tool's own cards.

mod everyday;
mod files;
mod input;
mod maintenance;
mod present;
mod system;
mod windows;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use forgekit_core::settings::General;
use forgekit_core::{Status, Tool};

use crate::util;
use crate::widgets::{self as w, hbox, vbox};
use crate::window::App;

/// A slot for a refresh callback that needs to call itself (e.g. after a row's delete button).
pub type SelfRef = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

pub fn tool_page(tool: &'static Tool, app: &Rc<App>) -> gtk::Widget {
    let root = vbox(18);
    root.add_css_class("fk-page");
    let status = tool.status(&app.caps);
    let enabled = app.enabled(tool.id);

    // ---- header card
    let hero = hbox(18);
    hero.add_css_class("fk-card");
    hero.add_css_class("fk-hero");
    hero.append(&w::icon_tile(tool.icon, true, ""));
    let info = vbox(6);
    info.set_hexpand(true);
    info.set_valign(gtk::Align::Center);
    let title_row = hbox(10);
    title_row.append(&w::label(tool.name, &["fk-hero-title"]));
    let (pill_text, tone) = w::status_style(tool, &status, enabled);
    let pill = w::pill(&pill_text, tone);
    title_row.append(&pill);
    info.append(&title_row);
    info.append(&w::label(tool.summary, &["dim"]));
    let a = app.clone();
    info.append(&w::command_chip(&format!("forgekit {}", tool.id), move || a.toast("Command copied")));
    if !tool.is_planned() && !matches!(status, Status::Unsupported(_)) {
        info.append(&crate::shortcut::row(tool, app));
    }
    hero.append(&info);

    let switch = gtk::Switch::builder()
        .active(enabled)
        .valign(gtk::Align::Center)
        .tooltip_text("Turn this tool on or off")
        .build();
    let sw_box = vbox(4);
    sw_box.set_valign(gtk::Align::Center);
    sw_box.append(&switch);
    sw_box.append(&gtk::Label::builder().label("Enabled").css_classes(["dim", "caption"]).build());
    if tool.is_planned() || matches!(status, Status::Unsupported(_)) {
        switch.set_sensitive(false);
    }
    hero.append(&sw_box);
    root.append(&hero);

    // ---- unavailable / setup
    if tool.is_planned() {
        let (card, body) = w::card("", "");
        body.append(&w::empty_state(
            "clock",
            &format!("{} is coming in v{}", tool.name, tool.since),
            "It's on the roadmap. Forgekit only shows tools as ready once they work on your desktop.",
        ));
        root.append(&card);
        return w::page(&root).upcast();
    }
    if let Status::Unsupported(reason) = &status {
        let (card, body) = w::card("", "");
        body.append(&w::empty_state("cloud-off", "Not available on this desktop", reason));
        root.append(&card);
        return w::page(&root).upcast();
    }
    if let Status::NeedsSetup(reason) = &status
        && tool.tier != forgekit_core::Tier::Helper
        && !matches!(tool.id, "mosaic" | "pin" | "beacon" | "scenes" | "roam")
    {
        root.append(&setup_card(tool, reason, app));
    }

    // ---- the tool itself
    let body: Vec<gtk::Widget> = match tool.id {
        "spark" => everyday::spark(app),
        "clipstack" => everyday::clipstack(app),
        "shelf" => everyday::shelf(app),
        "eyedrop" => everyday::eyedrop(app),
        "glyphgrab" => everyday::glyphgrab(app),
        "glance" => everyday::glance(app),
        "relabel" => files::relabel(app),
        "shrinkray" => files::shrinkray(app),
        "lockpick" => files::lockpick(app),
        "teku" => files::teku(app),
        "nightowl" => system::nightowl(app),
        "dusk" => system::dusk(app),
        "hostbook" => system::hostbook(app),
        "envy" => system::envy(app),
        "keysmith" => input::keysmith(app),
        "mosaic" => windows::mosaic(app),
        "pin" => windows::pin(app),
        "stage" => present::stage(app),
        "caliper" => present::caliper(app),
        "beacon" => present::beacon(app),
        "cheatsheet" => present::cheatsheet(app),
        "sweep" => maintenance::sweep(app),
        "bootpilot" => maintenance::bootpilot(app),
        "servicedeck" => maintenance::servicedeck(app),
        "settings-explorer" => maintenance::settings_explorer(app),
        "scenes" => maintenance::scenes(app),
        "roam" => maintenance::roam(app),
        "umlaut" => input::umlaut(app),
        _ => Vec::new(),
    };
    let content = vbox(18);
    for b in &body {
        content.append(b);
    }
    content.set_sensitive(enabled);
    root.append(&content);

    let a = app.clone();
    switch.connect_active_notify(move |s| {
        let mut g = General::load().unwrap_or_default();
        g.set_enabled(tool.id, s.is_active());
        if let Err(e) = g.save() {
            a.toast(&format!("Couldn't save settings: {e}"));
        }
        content.set_sensitive(s.is_active());
        let (text, tone) = w::status_style(tool, &tool.status(&a.caps), s.is_active());
        pill.set_label(&text);
        for t in ["ok", "warn", "off", "na", "soon"] {
            pill.remove_css_class(t);
        }
        pill.add_css_class(tone);
        if tool.id == "clipstack" {
            util::daemon_call("/org/forgekit/Clipstack", "org.forgekit.Clipstack1", "Reload");
        }
        a.tool_toggled();
    });
    w::page(&root).upcast()
}

/// Which package manager this system uses, for setup hints.
fn package_install(pkgs_apt: &str, pkgs_dnf: &str, pkgs_pacman: &str, pkgs_zypper: &str) -> Option<String> {
    let which = forgekit_core::paths::which;
    if which("apt").is_some() {
        Some(format!("sudo apt install {pkgs_apt}"))
    } else if which("dnf").is_some() {
        Some(format!("sudo dnf install {pkgs_dnf}"))
    } else if which("pacman").is_some() {
        Some(format!("sudo pacman -S {pkgs_pacman}"))
    } else if which("zypper").is_some() {
        Some(format!("sudo zypper install {pkgs_zypper}"))
    } else {
        None
    }
}

fn setup_card(tool: &Tool, reason: &str, app: &Rc<App>) -> gtk::Box {
    let card = hbox(14);
    card.add_css_class("fk-card");
    card.append(&w::icon_tile("alert-triangle", false, "warn"));
    let t = vbox(6);
    t.set_hexpand(true);
    t.append(&w::label("One step to finish setup", &["fk-card-title"]));
    t.append(&w::label(reason, &["dim"]));
    let fix = match tool.id {
        "glyphgrab" => package_install(
            "tesseract-ocr tesseract-ocr-eng",
            "tesseract tesseract-langpack-eng",
            "tesseract tesseract-data-eng",
            "tesseract-ocr tesseract-ocr-traineddata-english",
        ),
        "hostbook" => package_install("pkexec", "polkit", "polkit", "polkit"),
        "eyedrop" => {
            package_install("xdg-desktop-portal", "xdg-desktop-portal", "xdg-desktop-portal", "xdg-desktop-portal")
        }
        _ => None,
    };
    if let Some(cmd) = fix {
        t.append(&w::label("Run this in a terminal, then reopen Forgekit:", &["caption", "dim"]));
        let a = app.clone();
        t.append(&w::command_chip(&cmd, move || a.toast("Command copied")));
    }
    if tool.id == "teku" {
        let b = w::button("Get Teku", Some("external-link"), "");
        b.set_halign(gtk::Align::Start);
        b.connect_clicked(|_| util::open_uri("https://github.com/anuzdhk/teku"));
        t.append(&b);
    }
    card.append(&t);
    card
}

/// A row that holds a list of chosen/dropped files, with Choose and Clear.
pub fn files_row(files: Rc<RefCell<Vec<PathBuf>>>, title: &str, changed: impl Fn() + 'static) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).subtitle("Choose files, or drop them here").build();
    row.add_prefix(&w::icon_tile("folder-open", false, ""));
    let choose = w::button("Choose…", Some("folder"), "");
    let clear = w::icon_button("x", "Clear");
    row.add_suffix(&choose);
    row.add_suffix(&clear);
    let changed = Rc::new(changed);
    let set = {
        let (row, files, changed) = (row.clone(), files.clone(), changed.clone());
        Rc::new(move |v: Vec<PathBuf>| {
            *files.borrow_mut() = v;
            let n = files.borrow().len();
            row.set_subtitle(&match n {
                0 => "Choose files, or drop them here".to_string(),
                1 => w::escape(&util::file_name(&files.borrow()[0])),
                n => format!("{n} files"),
            });
            changed();
        })
    };
    {
        let set = set.clone();
        choose.connect_clicked(move |b| {
            let set = set.clone();
            util::pick_files(b, true, false, move |v| set(v));
        });
    }
    {
        let set = set.clone();
        clear.connect_clicked(move |_| set(Vec::new()));
    }
    util::on_drop(&row, move |v| set(v));
    row
}

pub fn boxed_list() -> gtk::ListBox {
    gtk::ListBox::builder().css_classes(["boxed-list"]).selection_mode(gtk::SelectionMode::None).build()
}

/// Status of the Forgekit GNOME extension with the one button that fixes it
/// (install, turn on, reinstall). `page` is rebuilt after a change.
pub fn extension_row(app: &Rc<App>, page: &'static str) -> adw::ActionRow {
    use forgekit_core::desktop::{self as ext, State};
    let row = adw::ActionRow::new();
    let state = ext::state();
    let (icon, tone, title, sub) = match state {
        State::Active => ("circle-check", "ok", "Ready", "The Forgekit GNOME extension is running."),
        State::NeedsRelogin => (
            "alert-triangle",
            "warn",
            "Log out and back in once",
            "GNOME loads newly installed extensions at the next login.",
        ),
        State::Outdated => (
            "refresh",
            "warn",
            "Update the GNOME extension",
            "This Forgekit has a newer extension. Update it, then log out and back in once.",
        ),
        State::Disabled => {
            ("eye-off", "off", "Extension turned off", "Turn it on in Forgekit or GNOME's Extensions app.")
        }
        State::Error => ("alert-triangle", "err", "GNOME couldn't load the extension", "Try reinstalling it."),
        State::NotInstalled => (
            "shield-check",
            "",
            "Needs a small GNOME extension",
            "GNOME doesn't let apps watch drags or move other windows. Forgekit's extension does only that, and sends nothing anywhere.",
        ),
    };
    row.set_title(title);
    row.set_subtitle(sub);
    row.add_prefix(&w::icon_tile(icon, false, tone));
    let action = match state {
        State::NotInstalled => Some(("Install", "download")),
        State::Error | State::Outdated => Some(("Update", "refresh")),
        State::Disabled => Some(("Turn on", "player-play")),
        _ => None,
    };
    if let Some((label, icon)) = action {
        let b = w::button(label, Some(icon), "suggested-action");
        let a = app.clone();
        b.connect_clicked(move |_| {
            match ext::install(Some(&crate::app_path())) {
                Ok(true) => a.toast("The GNOME extension is ready"),
                Ok(false) => a.toast("Installed. Log out and back in once to finish."),
                Err(e) => a.toast(&format!("{e:#}")),
            }
            a.rebuild_page(page);
        });
        row.add_suffix(&b);
    } else if state != State::NotInstalled {
        let rm = w::icon_button("trash", "Remove the extension");
        let a = app.clone();
        rm.connect_clicked(move |_| {
            match ext::remove() {
                Ok(()) => a.toast("Extension removed"),
                Err(e) => a.toast(&format!("{e:#}")),
            }
            a.rebuild_page(page);
        });
        row.add_suffix(&rm);
    }
    row
}
