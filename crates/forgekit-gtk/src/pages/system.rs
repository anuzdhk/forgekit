//! Nightowl, Hostbook and Envy pages.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use forgekit_core::tools::{envy, hostbook, nightowl};
use gtk::{gio, glib};

use super::boxed_list;
use crate::util;
use crate::widgets::{self as w, hbox, vbox};
use crate::window::App;

type Cards = Vec<gtk::Widget>;

// ------------------------------------------------------------------ Nightowl

thread_local! {
    // Used when the daemon isn't running; lives with the app, not the page.
    static LOCAL: RefCell<Option<nightowl::Inhibitor>> = const { RefCell::new(None) };
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

fn daemon(method: &str, args: Option<glib::Variant>) -> Result<glib::Variant, String> {
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).map_err(|e| e.to_string())?;
    bus.call_sync(
        Some("org.forgekit.Daemon"),
        "/org/forgekit/Daemon",
        "org.forgekit.Daemon1",
        method,
        args.as_ref(),
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        5000,
        gio::Cancellable::NONE,
    )
    .map_err(|e| e.to_string())
}

/// Whether the daemon is keeping the PC awake; `None` if it isn't running.
fn daemon_awake() -> Option<bool> {
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()?;
    let r = bus
        .call_sync(
            Some("org.forgekit.Daemon"),
            "/org/forgekit/Daemon",
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&("org.forgekit.Daemon1", "NightowlActive").to_variant()),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            2000,
            gio::Cancellable::NONE,
        )
        .ok()?;
    r.child_value(0).as_variant()?.get::<bool>()
}

pub fn nightowl(app: &Rc<App>) -> Cards {
    let via_daemon = daemon_awake();
    let active = via_daemon.unwrap_or_else(|| LOCAL.with_borrow(|i| i.is_some()));

    let big = hbox(18);
    big.add_css_class("fk-card");
    let tile = w::icon_tile(if active { "sun" } else { "moon-stars" }, true, if active { "ok" } else { "off" });
    big.append(&tile);
    let t = vbox(4);
    t.set_hexpand(true);
    t.set_valign(gtk::Align::Center);
    let title = w::label(if active { "Staying awake" } else { "Normal sleep settings" }, &["fk-hero-title"]);
    t.append(&title);
    let sub = w::label(
        match via_daemon {
            Some(_) => "Sleep and screen blanking are blocked while this is on, even if you close Forgekit.",
            None => "Sleep and screen blanking are blocked while this is on and Forgekit stays open.",
        },
        &["dim"],
    );
    t.append(&sub);
    big.append(&t);
    let switch = gtk::Switch::builder().active(active).valign(gtk::Align::Center).build();
    switch.set_size_request(-1, 28);
    big.append(&switch);

    let (opts, opts_body) = w::card("For how long", "");
    let rows = boxed_list();
    let durations = [
        ("Until I turn it off", 0u32),
        ("30 minutes", 1800),
        ("1 hour", 3600),
        ("2 hours", 7200),
        ("4 hours", 14400),
        ("8 hours", 28800),
    ];
    let combo =
        adw::ComboRow::builder().title("Keep awake").model(&gtk::StringList::new(&durations.map(|d| d.0))).build();
    rows.append(&combo);
    let a = app.clone();
    let cli =
        adw::ActionRow::builder().title("While a command runs").subtitle("Stays awake until the process exits").build();
    cli.add_suffix(&w::command_chip("forgekit nightowl --while PID", move || a.toast("Command copied")));
    rows.append(&cli);
    opts_body.append(&rows);

    let a = app.clone();
    switch.connect_active_notify(move |s| {
        let on = s.is_active();
        let secs = durations[combo.selected() as usize].1;
        let set_look = |on: bool| {
            title.set_label(if on { "Staying awake" } else { "Normal sleep settings" });
            for c in ["ok", "off"] {
                tile.remove_css_class(c);
            }
            tile.add_css_class(if on { "ok" } else { "off" });
            tile.set_icon_name(Some(&w::icon_name(if on { "sun" } else { "moon-stars" })));
        };
        if via_daemon.is_some() {
            let r = if on {
                daemon("NightowlStart", Some(("Forgekit Nightowl", secs).to_variant()))
            } else {
                daemon("NightowlStop", None)
            };
            if let Err(e) = r {
                a.toast(&e);
            }
            set_look(on);
            return;
        }
        let generation = GENERATION.with(|g| {
            g.set(g.get() + 1);
            g.get()
        });
        if !on {
            LOCAL.set(None);
            set_look(false);
            return;
        }
        match nightowl::Inhibitor::acquire("Forgekit Nightowl") {
            Ok(i) => {
                a.toast(&format!("Nightowl: {}", i.describe()));
                LOCAL.set(Some(i));
                set_look(true);
            }
            Err(e) => {
                a.toast(&format!("{e:#}"));
                s.set_active(false);
                return;
            }
        }
        if secs > 0 {
            let s = s.downgrade();
            glib::timeout_add_seconds_local_once(secs, move || {
                if GENERATION.get() == generation {
                    LOCAL.set(None);
                    if let Some(s) = s.upgrade() {
                        s.set_active(false);
                    }
                }
            });
        }
    });
    vec![big.upcast(), opts.upcast()]
}

// ------------------------------------------------------------------ Hostbook

pub fn hostbook(app: &Rc<App>) -> Cards {
    let path = PathBuf::from(hostbook::HOSTS);
    let hosts = match hostbook::Hosts::load(&path) {
        Ok(h) => Rc::new(RefCell::new(h)),
        Err(e) => {
            return vec![w::empty_state("alert-triangle", "Couldn't read /etc/hosts", &format!("{e:#}")).upcast()];
        }
    };
    let save = w::button("Save changes", Some("check"), "suggested-action");
    save.set_sensitive(false);
    let (card, body) = w::card_with_action(
        "Entries",
        "Switch entries off instead of deleting them. Saving asks for your password and keeps a backup.",
        &save,
    );
    let table = Rc::new(w::Table::new(&["On", "Host names", "Address", "Comment", ""], &[60, -1, 160, 140, 40]));
    body.append(&table.root);

    let fill: super::SelfRef = Rc::default();
    let f: Rc<dyn Fn()> = {
        let (hosts, save, table, fill) = (hosts.clone(), save.clone(), table.clone(), fill.clone());
        Rc::new(move || {
            table.clear();
            for e in hosts.borrow().entries() {
                let sw = gtk::Switch::builder()
                    .active(e.enabled)
                    .valign(gtk::Align::Center)
                    .halign(gtk::Align::Start)
                    .build();
                let name = e.names[0].clone();
                {
                    let (hosts, save) = (hosts.clone(), save.clone());
                    sw.connect_active_notify(move |s| {
                        hosts.borrow_mut().set_enabled(&name, s.is_active());
                        save.set_sensitive(true);
                    });
                }
                let del = w::icon_button("trash", "Remove");
                {
                    let (hosts, save, fill) = (hosts.clone(), save.clone(), fill.clone());
                    let names = e.names.clone();
                    del.connect_clicked(move |_| {
                        for n in &names {
                            hosts.borrow_mut().remove(n);
                        }
                        save.set_sensitive(true);
                        if let Some(f) = fill.borrow().clone() {
                            glib::idle_add_local_once(move || f());
                        }
                    });
                }
                table.add(vec![
                    sw.upcast(),
                    w::cell(&e.names.join(" "), &["fk-tile-title"]),
                    w::cell(&e.ip.to_string(), &["dim", "mono"]),
                    w::cell(e.comment.as_deref().unwrap_or(""), &["dim"]),
                    del.upcast(),
                ]);
            }
        })
    };
    *fill.borrow_mut() = Some(f.clone());
    f();

    let add_btn = w::button("Add", Some("plus"), "");
    let (add, add_body) = w::card_with_action(
        "Add an entry",
        "Point a name at an address, e.g. a local server or a blocked site → 0.0.0.0",
        &add_btn,
    );
    let rows = boxed_list();
    let ip = adw::EntryRow::builder().title("IP address").build();
    let names = adw::EntryRow::builder().title("Host names (space-separated)").build();
    rows.append(&ip);
    rows.append(&names);
    add_body.append(&rows);
    {
        let (a, hosts, save, f) = (app.clone(), hosts.clone(), save.clone(), f.clone());
        add_btn.connect_clicked(move |_| {
            let Ok(addr) = ip.text().trim().parse() else {
                a.toast("That isn't a valid IP address");
                return;
            };
            let n: Vec<String> = names.text().split_whitespace().map(str::to_string).collect();
            if let Err(e) = hosts.borrow_mut().add(addr, n) {
                a.toast(&format!("{e:#}"));
                return;
            }
            ip.set_text("");
            names.set_text("");
            save.set_sensitive(true);
            f();
        });
    }
    let a = app.clone();
    save.connect_clicked(move |b| {
        b.set_sensitive(false);
        let h = hosts.borrow().clone();
        let (path, a, b) = (path.clone(), a.clone(), b.clone());
        util::background(
            move || h.save(&path).map_err(|e| format!("{e:#}")),
            move |r| match r {
                Ok(_) => a.toast("Saved /etc/hosts (backup kept)"),
                Err(e) => {
                    b.set_sensitive(true);
                    a.toast(&e);
                }
            },
        );
    });
    vec![card.upcast(), add.upcast()]
}

// ------------------------------------------------------------------ Envy

pub fn envy(app: &Rc<App>) -> Cards {
    let mut cards: Cards = Vec::new();
    for scope in [envy::Scope::User, envy::Scope::System] {
        let path = scope.path();
        let file = Rc::new(RefCell::new(envy::EnvFile::load(&path).unwrap_or_default()));
        let save = w::button("Save", Some("check"), "suggested-action");
        save.set_sensitive(false);
        let (title, sub) = match scope {
            envy::Scope::User => ("Your variables", "Applied to your desktop session at next login"),
            envy::Scope::System => {
                ("System variables", "/etc/environment, for every user. Saving asks for your password.")
            }
        };
        let (card, body) = w::card_with_action(title, sub, &save);
        let list = boxed_list();
        body.append(&list);

        let fill: super::SelfRef = Rc::default();
        let f: Rc<dyn Fn()> = {
            let (file, list, save, fill) = (file.clone(), list.clone(), save.clone(), fill.clone());
            Rc::new(move || {
                list.remove_all();
                let vars: Vec<_> = file.borrow().vars().cloned().collect();
                for v in vars {
                    let row = adw::EntryRow::builder().title(&v.key).text(&v.value).build();
                    let del = w::icon_button("trash", "Remove");
                    row.add_suffix(&del);
                    {
                        let (file, save, key) = (file.clone(), save.clone(), v.key.clone());
                        row.connect_changed(move |r| {
                            if file.borrow_mut().set(&key, &r.text()).is_ok() {
                                save.set_sensitive(true);
                            }
                        });
                    }
                    {
                        let (file, save, fill, key) = (file.clone(), save.clone(), fill.clone(), v.key.clone());
                        del.connect_clicked(move |_| {
                            file.borrow_mut().remove(&key);
                            save.set_sensitive(true);
                            if let Some(f) = fill.borrow().clone() {
                                glib::idle_add_local_once(move || f());
                            }
                        });
                    }
                    list.append(&row);
                }
                // The "add" row is always last.
                let key = adw::EntryRow::builder().title("New variable: NAME=value").build();
                let add = w::icon_button("plus", "Add");
                key.add_suffix(&add);
                let (file, save, fill) = (file.clone(), save.clone(), fill.clone());
                let k2 = key.clone();
                let add_fn = move || {
                    let text = k2.text().to_string();
                    let Some((n, v)) = text.split_once('=') else { return };
                    if file.borrow_mut().set(n.trim(), v.trim()).is_ok() {
                        save.set_sensitive(true);
                        if let Some(f) = fill.borrow().clone() {
                            glib::idle_add_local_once(move || f());
                        }
                    }
                };
                let add_fn = Rc::new(add_fn);
                {
                    let a = add_fn.clone();
                    add.connect_clicked(move |_| a());
                }
                key.connect_entry_activated(move |_| add_fn());
                list.append(&key);
            })
        };
        *fill.borrow_mut() = Some(f.clone());
        f();

        let a = app.clone();
        save.connect_clicked(move |b| {
            b.set_sensitive(false);
            let fcopy = file.borrow().clone();
            let (a, b) = (a.clone(), b.clone());
            util::background(
                move || fcopy.save(scope).map_err(|e| format!("{e:#}")),
                move |r| match r {
                    Ok(_) => a.toast("Saved. Log out and back in to apply."),
                    Err(e) => {
                        b.set_sensitive(true);
                        a.toast(&e);
                    }
                },
            );
        });
        cards.push(card.upcast());
    }

    // Read-only view of other environment.d files.
    let others: Vec<_> = envy::sources().into_iter().filter(|s| s.editable.is_none() && !s.vars.is_empty()).collect();
    if !others.is_empty() {
        let (card, body) = w::card("Set elsewhere", "Other files in ~/.config/environment.d (read-only here)");
        let t = w::Table::new(&["Name", "Value", "File"], &[180, -1, 200]);
        for s in others {
            for v in s.vars {
                t.add(vec![
                    w::cell(&v.key, &["mono"]),
                    w::cell(&v.value, &["dim"]),
                    w::cell(&util::file_name(&s.path), &["dim"]),
                ]);
            }
        }
        body.append(&t.root);
        cards.push(card.upcast());
    }
    cards
}
