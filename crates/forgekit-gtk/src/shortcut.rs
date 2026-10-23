//! "Keyboard shortcut" control on tool pages: record keys in a small dialog,
//! warn about clashes, and save into the desktop's own shortcut settings.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use forgekit_core::Tool;
use forgekit_core::shortcuts::{self, Backend};
use gtk::{gdk, glib};

use crate::widgets::{self as w, hbox, vbox};
use crate::window::App;

/// What a shortcut is for: its settings key, a name for the dialog title,
/// and the action it runs.
#[derive(Clone)]
pub struct Spec {
    pub key: String,
    pub title: String,
    pub label: String,
    pub command: String,
}

impl Spec {
    pub fn tool(tool: &Tool) -> Self {
        let a = shortcuts::action(tool);
        Spec { key: tool.id.to_string(), title: tool.name.to_string(), label: a.label, command: a.command }
    }

    /// "Move the focused window into Mosaic zone `n`" (1-based).
    pub fn zone(n: usize) -> Self {
        let a = shortcuts::zone_action(n);
        Spec { key: shortcuts::zone_key(n), title: format!("zone {n}"), label: a.label, command: a.command }
    }
}

/// The row under a tool's description. Rebuilds itself after changes.
pub fn row(tool: &'static Tool, app: &Rc<App>) -> gtk::Box {
    row_for(Spec::tool(tool), app)
}

pub fn row_for(spec: Spec, app: &Rc<App>) -> gtk::Box {
    let holder = hbox(0);
    fill(&holder, Rc::new(spec), app);
    holder
}

fn fill(holder: &gtk::Box, spec: Rc<Spec>, app: &Rc<App>) {
    while let Some(c) = holder.first_child() {
        holder.remove(&c);
    }
    let b = hbox(8);
    b.set_valign(gtk::Align::Center);
    b.append(&w::icon("keyboard", 16));
    let action = spec.clone();
    let Some(backend) = shortcuts::backend(app.caps.desktop) else {
        // No supported backend: show the command to bind by hand.
        b.append(&w::label("Shortcut: bind this command in your desktop's keyboard settings", &["dim", "caption"]));
        let a = app.clone();
        b.append(&w::command_chip(&action.command, move || a.toast("Command copied")));
        holder.append(&b);
        return;
    };
    match shortcuts::get(backend, &spec.key) {
        Some(accel) => {
            b.append(&accel_label(&accel));
            b.append(&w::label(&action.label, &["dim", "caption"]));
            let change = w::button("Change", None, "flat");
            let remove = w::icon_button("x", "Remove shortcut");
            {
                let (h, a, sp) = (holder.clone(), app.clone(), spec.clone());
                change.connect_clicked(move |btn| record(btn, sp.clone(), backend, &a, &h));
            }
            {
                let (h, a, sp) = (holder.clone(), app.clone(), spec.clone());
                remove.connect_clicked(move |_| match shortcuts::remove(backend, &sp.key) {
                    Ok(()) => {
                        a.toast("Shortcut removed");
                        fill(&h, sp.clone(), &a);
                    }
                    Err(e) => a.toast(&format!("{e:#}")),
                });
            }
            b.append(&change);
            b.append(&remove);
        }
        None => {
            let add = w::button("Add keyboard shortcut", None, "flat");
            add.add_css_class("fk-link");
            add.set_tooltip_text(Some(&action.label));
            let (h, a, sp) = (holder.clone(), app.clone(), spec.clone());
            add.connect_clicked(move |btn| record(btn, sp.clone(), backend, &a, &h));
            b.append(&add);
        }
    }
    holder.append(&b);
}

/// `<Super><Shift>c` as key caps.
pub fn accel_label(accel: &str) -> gtk::Box {
    let b = hbox(4);
    let text = gtk::accelerator_parse(accel)
        .map(|(k, m)| gtk::accelerator_get_label(k, m).to_string())
        .unwrap_or_else(|| accel.to_string());
    for part in text.split('+').filter(|p| !p.is_empty()) {
        b.append(&gtk::Label::builder().label(part).css_classes(["fk-kbd"]).build());
    }
    b
}

fn record(anchor: &gtk::Button, spec: Rc<Spec>, backend: Backend, app: &Rc<App>, holder: &gtk::Box) {
    let dialog = adw::AlertDialog::new(Some(&format!("Shortcut for {}", spec.title)), None);
    let body = vbox(12);
    body.set_size_request(320, -1);
    let prompt = w::label("Press the keys you want to use…", &["dim"]);
    prompt.set_halign(gtk::Align::Center);
    let shown = hbox(0);
    shown.set_halign(gtk::Align::Center);
    shown.set_size_request(-1, 36);
    let note = w::label("Use at least one of Ctrl, Alt or Super, e.g. Super+Shift+C.", &["dim", "caption"]);
    note.set_halign(gtk::Align::Center);
    note.set_justify(gtk::Justification::Center);
    body.append(&prompt);
    body.append(&shown);
    body.append(&note);
    dialog.set_extra_child(Some(&body));
    dialog.add_responses(&[("cancel", "Cancel"), ("save", "Save")]);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("save", false);
    dialog.set_default_response(None);
    dialog.set_close_response("cancel");

    let chosen: Rc<RefCell<Option<String>>> = Rc::default();
    let key = spec.key.clone();
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let (chosen, dialog, shown, note) = (chosen.clone(), dialog.clone(), shown.clone(), note.clone());
        keys.connect_key_pressed(move |_, keyval, _, state| {
            let mods = state
                & (gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::SHIFT_MASK
                    | gdk::ModifierType::ALT_MASK
                    | gdk::ModifierType::SUPER_MASK);
            let name = keyval.name().map(|n| n.to_string()).unwrap_or_default();
            let is_modifier = name.contains("Control")
                || name.contains("Shift")
                || name.contains("Alt")
                || name.contains("Super")
                || name.contains("Meta")
                || name == "ISO_Level3_Shift";
            if is_modifier {
                return glib::Propagation::Stop;
            }
            // Escape alone cancels, like everywhere else.
            if keyval == gdk::Key::Escape && mods.is_empty() {
                dialog.close();
                return glib::Propagation::Stop;
            }
            let fkey = name.starts_with('F') && name[1..].parse::<u8>().is_ok();
            let needs_mod = !(mods.intersects(
                gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK | gdk::ModifierType::SUPER_MASK,
            ) || fkey);
            while let Some(c) = shown.first_child() {
                shown.remove(&c);
            }
            let accel = gtk::accelerator_name(keyval.to_lower(), mods).to_string();
            shown.append(&accel_label(&accel));
            if needs_mod {
                note.set_label("Add Ctrl, Alt or Super so the shortcut doesn't block normal typing.");
                note.remove_css_class("dim");
                note.add_css_class("warning");
                dialog.set_response_enabled("save", false);
                *chosen.borrow_mut() = None;
                return glib::Propagation::Stop;
            }
            let clashes = shortcuts::conflicts(backend, &key, &accel);
            if clashes.is_empty() {
                note.set_label("Free to use.");
                note.remove_css_class("warning");
                note.add_css_class("success");
            } else {
                note.set_label(&format!(
                    "Already used by: {}. Saving makes both fire; pick another if unsure.",
                    clashes.join(", ")
                ));
                note.remove_css_class("success");
                note.add_css_class("warning");
            }
            note.remove_css_class("dim");
            *chosen.borrow_mut() = Some(accel);
            dialog.set_response_enabled("save", true);
            glib::Propagation::Stop
        });
    }
    dialog.add_controller(keys);

    let (a, h) = (app.clone(), holder.clone());
    dialog.connect_response(None, move |_, r| {
        if r != "save" {
            return;
        }
        let Some(accel) = chosen.borrow().clone() else { return };
        match shortcuts::set(backend, &spec.key, &spec.label, &spec.command, &accel) {
            Ok(()) => {
                a.toast(&format!("Shortcut saved: {}", spec.label));
                fill(&h, spec.clone(), &a);
            }
            Err(e) => a.toast(&format!("{e:#}")),
        }
    });
    dialog.present(Some(anchor));
}
