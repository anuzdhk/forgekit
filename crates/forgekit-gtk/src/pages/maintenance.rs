//! Sweep, Startup Apps, Services, Settings Explorer, Scenes and Roam pages.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use forgekit_core::tools::glance::human_size;
use forgekit_core::tools::servicedeck::{self, Action, Scope, Service};
use forgekit_core::tools::{bootpilot, roam, scenes, settings_explorer, spark, sweep};
use gtk::{gio, glib};

use super::boxed_list;
use crate::util::{self, Out};
use crate::widgets::{self as w, hbox, vbox};
use crate::window::App;

type Cards = Vec<gtk::Widget>;

fn app_icon(name: &str, fallback: &str) -> gtk::Image {
    let i = if name.starts_with('/') {
        gtk::Image::from_file(name)
    } else if !name.is_empty() && gtk::IconTheme::for_display(&gtk::gdk::Display::default().unwrap()).has_icon(name) {
        gtk::Image::from_icon_name(name)
    } else {
        return w::icon_tile(fallback, false, "off");
    };
    i.set_pixel_size(32);
    i.set_margin_start(3);
    i.set_margin_end(3);
    i
}

// ------------------------------------------------------------------ Sweep

pub fn sweep(app: &Rc<App>) -> Cards {
    let clean = w::button("Clean selected", Some("vacuum-cleaner"), "suggested-action");
    clean.set_sensitive(false);
    let (card, body) = w::card_with_action("Free up space", "Scanning…", &clean);
    let list = boxed_list();
    body.append(&list);
    let picks: Rc<RefCell<Vec<(sweep::Item, gtk::CheckButton)>>> = Rc::default();

    let summary = {
        let (picks, clean, card) = (picks.clone(), clean.clone(), card.clone());
        Rc::new(move || {
            let total: u64 = picks.borrow().iter().filter(|(_, c)| c.is_active()).filter_map(|(i, _)| i.size).sum();
            let n = picks.borrow().iter().filter(|(_, c)| c.is_active()).count();
            clean.set_sensitive(n > 0);
            if let Some(sub) = card
                .first_child()
                .and_then(|h| h.first_child())
                .and_then(|t| t.last_child())
                .and_downcast::<gtk::Label>()
            {
                sub.set_label(&if n == 0 {
                    "Pick what to clean".to_string()
                } else {
                    format!("{n} selected · about {} freed", human_size(total))
                });
            }
        })
    };
    let scan: Rc<dyn Fn()> = {
        let (list, picks, summary) = (list.clone(), picks.clone(), summary.clone());
        Rc::new(move || {
            list.remove_all();
            picks.borrow_mut().clear();
            let (list, picks, summary) = (list.clone(), picks.clone(), summary.clone());
            util::background(sweep::scan, move |items| {
                let saved = sweep::Settings::load().unwrap_or_default();
                for item in items {
                    let row = adw::ActionRow::builder()
                        .title(&item.title)
                        .subtitle(w::escape(&item.detail))
                        .activatable(true)
                        .build();
                    let check =
                        gtk::CheckButton::builder().active(saved.picks(&item)).valign(gtk::Align::Center).build();
                    row.add_prefix(&check);
                    row.set_activatable_widget(Some(&check));
                    if item.root {
                        let lock = w::icon("lock", 16);
                        lock.set_tooltip_text(Some("Asks for your password"));
                        lock.add_css_class("dim");
                        row.add_suffix(&lock);
                    }
                    let size = item.size.map(human_size).unwrap_or_else(|| "—".into());
                    row.add_suffix(&gtk::Label::builder().label(size).css_classes(["fk-tile-title"]).build());
                    let (s, picks2) = (summary.clone(), picks.clone());
                    check.connect_toggled(move |_| {
                        // Remember the ticks for "Clean PC" on Home.
                        let selected =
                            picks2.borrow().iter().filter(|(_, c)| c.is_active()).map(|(i, _)| i.kind).collect();
                        let _ = sweep::Settings { selected: Some(selected) }.save();
                        s();
                    });
                    list.append(&row);
                    picks.borrow_mut().push((item, check));
                }
                summary();
            });
        })
    };
    scan();

    let a = app.clone();
    clean.connect_clicked(move |b| {
        let chosen: Vec<sweep::Kind> =
            picks.borrow().iter().filter(|(_, c)| c.is_active()).map(|(i, _)| i.kind).collect();
        let trash = chosen.contains(&sweep::Kind::Trash);
        let dialog = adw::AlertDialog::new(
            Some("Clean the selected items?"),
            Some(if trash {
                "Emptying the trash deletes those files for good."
            } else {
                "Everything selected is rebuilt or redownloaded when needed."
            }),
        );
        dialog.add_responses(&[("cancel", "Cancel"), ("clean", "Clean")]);
        dialog.set_response_appearance(
            "clean",
            if trash { adw::ResponseAppearance::Destructive } else { adw::ResponseAppearance::Suggested },
        );
        let (a, scan, b2) = (a.clone(), scan.clone(), b.clone());
        dialog.connect_response(None, move |_, r| {
            if r != "clean" {
                return;
            }
            b2.set_sensitive(false);
            let chosen = chosen.clone();
            let (a, scan) = (a.clone(), scan.clone());
            util::background(
                move || sweep::clean(&chosen).map_err(|e| format!("{e:#}")),
                move |r| {
                    match r {
                        Ok(done) => a.toast(&done.join(" · ")),
                        Err(e) => a.toast(&e),
                    }
                    scan();
                },
            );
        });
        dialog.present(Some(b));
    });
    vec![card.upcast()]
}

// ------------------------------------------------------------------ Bootpilot

pub fn bootpilot(app: &Rc<App>) -> Cards {
    let add = w::button("Add an app", Some("plus"), "suggested-action");
    let (card, body) = w::card_with_action(
        "Apps that start when you log in",
        "Switching one off takes effect at your next login.",
        &add,
    );
    let list = boxed_list();
    body.append(&list);
    for e in bootpilot::list() {
        let row = adw::ActionRow::builder()
            .title(w::escape(&e.name))
            .subtitle(w::escape(if e.other_desktop { "Only runs on other desktops" } else { &e.comment }))
            .build();
        row.add_prefix(&app_icon(&e.icon, "rocket"));
        if e.other_desktop {
            row.add_css_class("dim");
        }
        if !e.system {
            let rm = w::icon_button("trash", "Remove from startup");
            let (a, id) = (app.clone(), e.id.clone());
            rm.connect_clicked(move |_| {
                match bootpilot::remove(&id) {
                    Ok(()) => a.toast("Removed from startup"),
                    Err(err) => a.toast(&format!("{err:#}")),
                }
                a.rebuild_page("bootpilot");
            });
            row.add_suffix(&rm);
        }
        let sw = gtk::Switch::builder().active(e.enabled).valign(gtk::Align::Center).build();
        let (a, id) = (app.clone(), e.id.clone());
        sw.connect_active_notify(move |s| {
            if let Err(err) = bootpilot::set_enabled(&id, s.is_active()) {
                a.toast(&format!("{err:#}"));
            }
        });
        row.add_suffix(&sw);
        list.append(&row);
    }

    // Add: pick from installed apps.
    let pop_list = gtk::ListBox::builder().css_classes(["navigation-sidebar"]).build();
    let search = gtk::SearchEntry::builder().placeholder_text("Search apps…").build();
    let pop_box = vbox(6);
    pop_box.append(&search);
    pop_box.append(
        &gtk::ScrolledWindow::builder().child(&pop_list).min_content_height(320).min_content_width(300).build(),
    );
    let pop = gtk::Popover::builder().child(&pop_box).build();
    pop.set_parent(&add);
    let apps: Rc<Vec<spark::App>> = Rc::new({
        let mut v = spark::load_apps();
        v.sort_by_key(|a| a.name.to_lowercase());
        v
    });
    let fill = {
        let (apps, pop_list) = (apps.clone(), pop_list.clone());
        Rc::new(move |q: &str| {
            pop_list.remove_all();
            let q = q.to_lowercase();
            for a in apps.iter().filter(|a| q.is_empty() || a.name.to_lowercase().contains(&q)).take(60) {
                let r = hbox(8);
                r.append(&app_icon(&a.icon, "apps"));
                r.append(&w::label(&a.name, &[]));
                let row = gtk::ListBoxRow::builder().child(&r).name(a.path.to_string_lossy()).build();
                pop_list.append(&row);
            }
        })
    };
    fill("");
    {
        let fill = fill.clone();
        search.connect_search_changed(move |s| fill(&s.text()));
    }
    {
        let (a, pop) = (app.clone(), pop.clone());
        pop_list.connect_row_activated(move |_, row| {
            pop.popdown();
            match bootpilot::add(&PathBuf::from(row.widget_name().as_str())) {
                Ok(_) => a.toast("Added to startup"),
                Err(e) => a.toast(&format!("{e:#}")),
            }
            a.rebuild_page("bootpilot");
        });
    }
    add.connect_clicked(move |_| pop.popup());

    let services = bootpilot::user_services();
    let mut cards: Cards = vec![card.upcast()];
    if !services.is_empty() {
        let (svc, s_body) = w::card(
            "Your background services",
            "systemd user services. Many belong to the desktop itself (sound, sync…); only switch off ones you recognise.",
        );
        let list = boxed_list();
        for s in services {
            let row = adw::SwitchRow::builder().title(w::escape(&s.unit)).active(s.enabled).build();
            let (a, unit) = (app.clone(), s.unit.clone());
            row.connect_active_notify(move |r| {
                if let Err(e) = bootpilot::set_user_service(&unit, r.is_active()) {
                    a.toast(&format!("{e:#}"));
                }
            });
            list.append(&row);
        }
        s_body.append(&list);
        cards.push(svc.upcast());
    }
    cards
}

// ------------------------------------------------------------------ Servicedeck

fn systemd_call(scope: Scope, action: Action, unit: &str) -> Result<(), String> {
    let bus = gio::bus_get_sync(
        if scope == Scope::System { gio::BusType::System } else { gio::BusType::Session },
        gio::Cancellable::NONE,
    )
    .map_err(|e| e.to_string())?;
    let (method, file_call) = servicedeck::dbus_method(action);
    let args = match (action, file_call) {
        (Action::Enable, _) => (vec![unit.to_string()], false, true).to_variant(),
        (Action::Disable, _) => (vec![unit.to_string()], false).to_variant(),
        _ => (unit, "replace").to_variant(),
    };
    let call = |m: &str, a: Option<&glib::Variant>| {
        bus.call_sync(
            Some("org.freedesktop.systemd1"),
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            m,
            a,
            None,
            gio::DBusCallFlags::ALLOW_INTERACTIVE_AUTHORIZATION,
            120_000,
            gio::Cancellable::NONE,
        )
        .map_err(|e| e.message().to_string())
    };
    call(method, Some(&args))?;
    if file_call {
        call("Reload", None)?;
    }
    Ok(())
}

fn status_tone(s: &Service) -> (&'static str, &'static str) {
    match (s.active.as_str(), s.sub.as_str()) {
        ("active", "running") => ("Running", "ok"),
        ("active", _) => ("Done", "soon"),
        ("failed", _) => ("Failed", "warn"),
        ("activating" | "reloading", _) => ("Starting", "soon"),
        _ => ("Stopped", "na"),
    }
}

pub fn servicedeck(app: &Rc<App>) -> Cards {
    let search = gtk::SearchEntry::builder().placeholder_text("Search services…").hexpand(true).build();
    let scopes = gtk::StringList::new(&["System", "Mine"]);
    let scope_dd = gtk::DropDown::builder().model(&scopes).build();
    let filters = gtk::StringList::new(&["All", "Running", "Failed", "Starts at boot"]);
    let filter_dd = gtk::DropDown::builder().model(&filters).build();
    let bar = hbox(8);
    bar.append(&search);
    bar.append(&scope_dd);
    bar.append(&filter_dd);
    let (card, body) = w::card("Services", "Starting, stopping or changing system services asks for your password.");
    body.append(&bar);
    let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["fk-table"]).build();
    let list_card = vbox(0);
    list_card.add_css_class("fk-card");
    list_card.add_css_class("flush");
    list_card.append(&list);
    body.append(&list_card);

    let data: Rc<RefCell<Vec<Service>>> = Rc::default();
    let render: Rc<dyn Fn()> = {
        let (data, list, search, filter_dd, scope_dd, a) =
            (data.clone(), list.clone(), search.clone(), filter_dd.clone(), scope_dd.clone(), app.clone());
        Rc::new(move || {
            list.remove_all();
            let q = search.text().to_lowercase();
            let scope = if scope_dd.selected() == 0 { Scope::System } else { Scope::User };
            let shown: Vec<Service> = data
                .borrow()
                .iter()
                .filter(|s| {
                    q.is_empty() || s.unit.to_lowercase().contains(&q) || s.description.to_lowercase().contains(&q)
                })
                .filter(|s| match filter_dd.selected() {
                    1 => s.sub == "running",
                    2 => s.active == "failed",
                    3 => s.file_state == "enabled",
                    _ => true,
                })
                .take(250)
                .cloned()
                .collect();
            for s in shown {
                let row = hbox(10);
                let (text, tone) = status_tone(&s);
                let pill = w::pill(text, tone);
                // Same width for every status, so the names line up.
                pill.set_width_request(74);
                pill.set_halign(gtk::Align::Fill);
                row.append(&pill);
                let t = vbox(1);
                t.set_hexpand(true);
                t.append(
                    &gtk::Label::builder()
                        .label(&s.unit)
                        .xalign(0.0)
                        .ellipsize(gtk::pango::EllipsizeMode::End)
                        .css_classes(["fk-tile-title"])
                        .build(),
                );
                t.append(
                    &gtk::Label::builder()
                        .label(&s.description)
                        .xalign(0.0)
                        .ellipsize(gtk::pango::EllipsizeMode::End)
                        .css_classes(["dim", "caption"])
                        .build(),
                );
                row.append(&t);
                if s.file_state == "enabled" {
                    row.append(&gtk::Label::builder().label("at boot").css_classes(["dim", "caption"]).build());
                }
                let running = s.active == "active";
                let actions: Vec<(Action, &str, &str)> = if running {
                    vec![(Action::Restart, "refresh", "Restart"), (Action::Stop, "player-stop", "Stop")]
                } else {
                    vec![(Action::Start, "player-play", "Start")]
                };
                for (action, icon, tip) in actions {
                    let b = w::icon_button(icon, tip);
                    let (a, unit) = (a.clone(), s.unit.clone());
                    b.connect_clicked(move |b| run_action(&a, b, scope, action, &unit));
                    row.append(&b);
                }
                let logs = w::icon_button("file-text", "Recent log");
                {
                    let (a, unit) = (a.clone(), s.unit.clone());
                    let enabled = s.file_state == "enabled";
                    let can_toggle = matches!(s.file_state.as_str(), "enabled" | "disabled");
                    logs.connect_clicked(move |b| show_logs(&a, b, scope, &unit, can_toggle.then_some(enabled)));
                }
                row.append(&logs);
                list.append(&gtk::ListBoxRow::builder().child(&row).activatable(false).build());
            }
        })
    };
    let load: Rc<dyn Fn()> = {
        let (data, render, scope_dd, a) = (data.clone(), render.clone(), scope_dd.clone(), app.clone());
        Rc::new(move || {
            let scope = if scope_dd.selected() == 0 { Scope::System } else { Scope::User };
            let (data, render, a) = (data.clone(), render.clone(), a.clone());
            util::background(
                move || servicedeck::list(scope).map_err(|e| format!("{e:#}")),
                move |r| match r {
                    Ok(v) => {
                        *data.borrow_mut() = v;
                        render();
                    }
                    Err(e) => a.toast(&e),
                },
            );
        })
    };
    load();
    {
        let render = render.clone();
        search.connect_search_changed(move |_| render());
    }
    {
        let render = render.clone();
        filter_dd.connect_selected_notify(move |_| render());
    }
    {
        let load = load.clone();
        scope_dd.connect_selected_notify(move |_| load());
    }
    REFRESH.with_borrow_mut(|r| *r = Some(load));
    vec![card.upcast()]
}

thread_local! {
    static REFRESH: RefCell<Option<Rc<dyn Fn()>>> = RefCell::new(None);
}

fn run_action(app: &Rc<App>, anchor: &impl IsA<gtk::Widget>, scope: Scope, action: Action, unit: &str) {
    anchor.set_sensitive(false);
    let (a, unit, anchor) = (app.clone(), unit.to_string(), anchor.clone().upcast::<gtk::Widget>());
    let u = unit.clone();
    util::background(
        move || systemd_call(scope, action, &u),
        move |r| {
            anchor.set_sensitive(true);
            match r {
                Ok(()) => a.toast(&format!("{}: {}", unit, action.label())),
                Err(e) => a.toast(&format!("Couldn't {} {unit}: {e}", action.label())),
            }
            // Give systemd a moment, then refresh the list.
            glib::timeout_add_local_once(std::time::Duration::from_millis(600), || {
                REFRESH.with_borrow(|r| {
                    if let Some(f) = r {
                        f();
                    }
                });
            });
        },
    );
}

fn show_logs(app: &Rc<App>, anchor: &gtk::Button, scope: Scope, unit: &str, enabled: Option<bool>) {
    let dialog = adw::Dialog::builder().title(unit).content_width(860).content_height(560).build();
    let (view, buf) = util::log_view();
    view.set_vexpand(true);
    buf.set_text("Loading…");
    let body = vbox(10);
    body.set_margin_top(12);
    body.set_margin_bottom(12);
    body.set_margin_start(12);
    body.set_margin_end(12);
    if let Some(on) = enabled {
        let rows = boxed_list();
        let boot = adw::SwitchRow::builder().title("Start at boot").active(on).build();
        let (a, u, b) = (app.clone(), unit.to_string(), boot.clone());
        boot.connect_active_notify(move |r| {
            run_action(&a, &b, scope, if r.is_active() { Action::Enable } else { Action::Disable }, &u)
        });
        rows.append(&boot);
        body.append(&rows);
    }
    body.append(&view);
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    tv.set_content(Some(&body));
    dialog.set_child(Some(&tv));
    let u = unit.to_string();
    util::background(
        move || servicedeck::logs(scope, &u, 300).unwrap_or_else(|e| format!("{e:#}")),
        move |text| buf.set_text(&text),
    );
    dialog.present(Some(anchor));
}

// ------------------------------------------------------------------ Settings Explorer

pub fn settings_explorer(app: &Rc<App>) -> Cards {
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search settings, e.g. “font”, “dark”, “touchpad”…")
        .hexpand(true)
        .build();
    let changed_only = gtk::ToggleButton::builder().label("Only changed").css_classes(["pill"]).build();
    let bar = hbox(8);
    bar.append(&search);
    bar.append(&changed_only);
    let (card, body) = w::card(
        "All settings",
        "Every GNOME setting, including hidden ones. Changes apply immediately; Reset brings back the default.",
    );
    body.append(&bar);
    let count = w::label("Loading…", &["dim", "caption"]);
    body.append(&count);
    let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["fk-table"]).build();
    let list_card = vbox(0);
    list_card.add_css_class("fk-card");
    list_card.add_css_class("flush");
    list_card.append(&list);
    body.append(&list_card);

    let data: Rc<RefCell<Vec<settings_explorer::Key>>> = Rc::default();
    let render: Rc<dyn Fn()> = {
        let (data, list, search, changed_only, count, a) =
            (data.clone(), list.clone(), search.clone(), changed_only.clone(), count.clone(), app.clone());
        Rc::new(move || {
            list.remove_all();
            let q = search.text().to_lowercase();
            let all = data.borrow();
            let mut matches: Vec<&settings_explorer::Key> = all
                .iter()
                .filter(|k| !changed_only.is_active() || k.modified)
                .filter(|k| {
                    q.is_empty()
                        || k.key.contains(&q)
                        || k.schema.to_lowercase().contains(&q)
                        || k.value.to_lowercase().contains(&q)
                })
                .collect();
            // Settings you've changed first: those are the ones you come back to.
            matches.sort_by_key(|k| !k.modified);
            count.set_label(&if matches.len() > 150 {
                format!("{} matches, showing the first 150. Narrow it down with search.", matches.len())
            } else {
                format!("{} settings", matches.len())
            });
            for k in matches.into_iter().take(150) {
                list.append(&key_row(&a, k));
            }
        })
    };
    {
        let (data, render, a) = (data.clone(), render.clone(), app.clone());
        util::background(
            || settings_explorer::all().map_err(|e| format!("{e:#}")),
            move |r| match r {
                Ok(v) => {
                    *data.borrow_mut() = v;
                    render();
                }
                Err(e) => a.toast(&e),
            },
        );
    }
    {
        let render = render.clone();
        search.connect_search_changed(move |_| render());
    }
    changed_only.connect_toggled(move |_| render());
    vec![card.upcast()]
}

fn key_row(app: &Rc<App>, k: &settings_explorer::Key) -> gtk::ListBoxRow {
    let row = hbox(10);
    let t = vbox(1);
    t.set_hexpand(true);
    let title = hbox(6);
    title.append(&gtk::Label::builder().label(&k.key).xalign(0.0).css_classes(["fk-tile-title"]).build());
    if k.modified {
        title.append(&w::pill("changed", "soon"));
    }
    t.append(&title);
    t.append(
        &gtk::Label::builder()
            .label(&k.schema)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .css_classes(["dim", "caption"])
            .build(),
    );
    row.append(&t);

    let (schema, key) = (k.schema.clone(), k.key.clone());
    let a = app.clone();
    let apply = Rc::new(move |value: String| match settings_explorer::set(&schema, &key, &value) {
        Ok(()) => a.toast(&format!("{key} = {value}")),
        Err(e) => a.toast(&format!("{e:#}")),
    });
    let editor: gtk::Widget = match k.value.as_str() {
        "true" | "false" => {
            let sw = gtk::Switch::builder().active(k.value == "true").valign(gtk::Align::Center).build();
            let apply = apply.clone();
            sw.connect_active_notify(move |s| apply(s.is_active().to_string()));
            sw.upcast()
        }
        _ => {
            let e = gtk::Entry::builder()
                .text(&k.value)
                .width_chars(24)
                .max_width_chars(36)
                .valign(gtk::Align::Center)
                .build();
            e.set_tooltip_text(Some("Press Enter to apply (GVariant text: strings in 'quotes')"));
            let apply = apply.clone();
            e.connect_activate(move |e| apply(e.text().to_string()));
            e.upcast()
        }
    };
    row.append(&editor);

    let info = w::icon_button("info-circle", "What is this?");
    {
        let (schema, key) = (k.schema.clone(), k.key.clone());
        info.connect_clicked(move |b| {
            let text = settings_explorer::describe(&schema, &key);
            let label = gtk::Label::builder()
                .label(if text.is_empty() { "No description in the schema." } else { &text })
                .wrap(true)
                .max_width_chars(48)
                .margin_top(8)
                .margin_bottom(8)
                .margin_start(8)
                .margin_end(8)
                .build();
            let pop = gtk::Popover::builder().child(&label).build();
            pop.set_parent(b);
            pop.popup();
        });
    }
    row.append(&info);
    if k.modified {
        let reset = w::icon_button("arrow-back-up", "Reset to default");
        let (a, schema, key, editor) = (app.clone(), k.schema.clone(), k.key.clone(), editor.clone());
        reset.connect_clicked(move |b| match settings_explorer::reset(&schema, &key) {
            Ok(()) => {
                let v = settings_explorer::get(&schema, &key).unwrap_or_default();
                if let Some(e) = editor.downcast_ref::<gtk::Entry>() {
                    e.set_text(&v);
                } else if let Some(s) = editor.downcast_ref::<gtk::Switch>() {
                    s.set_active(v == "true");
                }
                b.set_visible(false);
                a.toast(&format!("{key} reset to {v}"));
            }
            Err(e) => a.toast(&format!("{e:#}")),
        });
        row.append(&reset);
    }
    gtk::ListBoxRow::builder().child(&row).activatable(false).build()
}

// ------------------------------------------------------------------ Scenes

fn slug(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect()
}

pub fn scenes(app: &Rc<App>) -> Cards {
    let rows = boxed_list();
    rows.append(&super::extension_row(app, "scenes"));
    let mut cards: Cards = vec![rows.upcast()];

    let save = w::button("Save current windows", Some("plus"), "suggested-action");
    let (card, body) = w::card_with_action(
        "Your scenes",
        "Arrange your apps how you like them, save that as a scene, and bring it back in one click: missing apps start and every window goes to its place.",
        &save,
    );
    {
        let a = app.clone();
        save.connect_clicked(move |b| {
            let dialog = adw::AlertDialog::new(
                Some("Save this arrangement"),
                Some("Give the scene a name, e.g. “Coding” or “Writing”."),
            );
            let entry = gtk::Entry::builder().placeholder_text("Scene name").activates_default(true).build();
            dialog.set_extra_child(Some(&entry));
            dialog.add_responses(&[("cancel", "Cancel"), ("save", "Save")]);
            dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("save"));
            let a = a.clone();
            dialog.connect_response(None, move |_, r| {
                let name = entry.text().trim().to_string();
                if r != "save" || name.is_empty() {
                    return;
                }
                match scenes::capture(&name) {
                    Ok(scene) => {
                        let mut all = scenes::load();
                        all.retain(|s| s.name != name);
                        let n = scene.items.len();
                        all.push(scene);
                        match scenes::save(&all) {
                            Ok(()) => a.toast(&format!("Saved “{name}” with {n} window(s)")),
                            Err(e) => a.toast(&format!("{e:#}")),
                        }
                    }
                    Err(e) => a.toast(&format!("{e:#}")),
                }
                a.rebuild_page("scenes");
            });
            dialog.present(Some(b));
        });
    }
    let all = scenes::load();
    if all.is_empty() {
        body.append(&w::empty_state(
            "layout-board-split",
            "No scenes yet",
            "Open and arrange some apps, then press “Save current windows”.",
        ));
    }
    for scene in all {
        let c = vbox(8);
        c.add_css_class("fk-card");
        let top = hbox(10);
        let t = vbox(2);
        t.set_hexpand(true);
        t.append(&w::label(&scene.name, &["fk-tile-title"]));
        let names: Vec<String> = scene.items.iter().map(|i| i.name.clone()).collect();
        t.append(&w::label(&names.join(" · "), &["dim", "caption"]));
        top.append(&t);
        let restore = w::button("Restore", Some("player-play"), "suggested-action");
        {
            let (a, scene) = (app.clone(), scene.clone());
            restore.connect_clicked(move |_| match scenes::restore(&scene) {
                Ok(missing) if missing.is_empty() => a.toast(&format!("Restoring “{}”", scene.name)),
                Ok(missing) => a.toast(&format!("Couldn't start: {}", missing.join(", "))),
                Err(e) => a.toast(&format!("{e:#}")),
            });
        }
        top.append(&restore);
        let del = w::icon_button("trash", "Delete scene");
        {
            let (a, name) = (app.clone(), scene.name.clone());
            del.connect_clicked(move |_| {
                let mut all = scenes::load();
                all.retain(|s| s.name != name);
                if let Err(e) = scenes::save(&all) {
                    a.toast(&format!("{e:#}"));
                }
                a.rebuild_page("scenes");
            });
        }
        top.append(&del);
        c.append(&top);
        let cmd = format!(
            "{} scenes restore {}",
            forgekit_core::shortcuts::bin_path_for("forgekit"),
            forgekit_core::shortcuts::quote(&scene.name)
        );
        let spec = crate::shortcut::Spec {
            key: format!("scene-{}", slug(&scene.name)),
            title: format!("scene “{}”", scene.name),
            label: format!("Restore scene {}", scene.name),
            command: cmd,
        };
        c.append(&crate::shortcut::row_for(spec, app));
        body.append(&c);
    }
    cards.push(card.upcast());
    cards
}

// ------------------------------------------------------------------ Roam

pub fn roam(app: &Rc<App>) -> Cards {
    let mut cards: Cards = Vec::new();
    match roam::find() {
        Some(found) => {
            let open = w::button(&format!("Open {}", found.name), Some("external-link"), "suggested-action");
            open.add_css_class("pill");
            let (card, body) = w::card_with_action(
                &format!("{} is installed", found.name),
                "It does the input sharing; Forgekit just gets you there.",
                &open,
            );
            {
                let a = app.clone();
                open.connect_clicked(move |_| {
                    if let Err(e) = roam::launch(&found) {
                        a.toast(&format!("{e:#}"));
                    }
                });
            }
            for (n, step) in [
                "On the PC whose mouse and keyboard you'll use, choose “Use this computer's keyboard and mouse” (server).",
                "On the other PC, install it too and choose “Use another computer's keyboard and mouse” (client), with the server's IP address.",
                "In the server's screen layout, drag the other PC to where it sits on your desk. Move the pointer off that edge to cross over.",
            ]
            .iter()
            .enumerate()
            {
                let r = hbox(10);
                r.append(&gtk::Label::builder().label((n + 1).to_string()).css_classes(["fk-kbd"]).valign(gtk::Align::Start).build());
                r.append(&w::label(step, &[]));
                body.append(&r);
            }
            cards.push(card.upcast());
        }
        None => {
            let install = w::button("Install Deskflow", Some("download"), "suggested-action");
            install.add_css_class("pill");
            let (card, body) = w::card_with_action(
                "Use one mouse and keyboard on several PCs",
                "Roam uses Deskflow, the open-source project behind Synergy. Sharing input over the network needs a careful, well-tested protocol, so Forgekit doesn't reinvent it.",
                &install,
            );
            let (log, buf) = util::log_view();
            log.set_visible(false);
            body.append(&log);
            let a = app.clone();
            install.connect_clicked(move |b| {
                b.set_sensitive(false);
                log.set_visible(true);
                buf.set_text("Adding Flathub (if needed)…\n");
                let cmd = match roam::ensure_flathub().and_then(|_| roam::install_command()) {
                    Ok(c) => c,
                    Err(e) => {
                        a.toast(&format!("{e:#}"));
                        b.set_sensitive(true);
                        return;
                    }
                };
                let mut c = std::process::Command::new(&cmd[0]);
                c.args(&cmd[1..]);
                let (buf, a, b) = (buf.clone(), a.clone(), b.clone());
                util::stream(c, move |m| match m {
                    Out::Line(l) => util::append(&buf, &l),
                    Out::Done(r) => {
                        b.set_sensitive(true);
                        match r {
                            Ok(()) => {
                                a.toast("Deskflow installed");
                                a.rebuild_page("roam");
                            }
                            Err(e) => a.toast(&format!("Install didn't finish: {e}")),
                        }
                    }
                });
            });
            cards.push(card.upcast());
        }
    }
    cards
}
