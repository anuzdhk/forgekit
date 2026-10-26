//! Cheatsheet: a searchable window with this desktop's keyboard shortcuts.

use std::rc::Rc;

use adw::prelude::*;
use forgekit_core::Capabilities;
use forgekit_core::tools::cheatsheet::{self, Shortcut};
use gtk::{gdk, glib};

use crate::widgets::{self as w, hbox, vbox};

/// Key caps for "Super+Shift+Page Down".
pub fn keys_widget(keys: &str) -> gtk::Box {
    let b = hbox(4);
    b.set_valign(gtk::Align::Center);
    for k in keys.split('+').filter(|k| !k.is_empty()) {
        b.append(&gtk::Label::builder().label(k).css_classes(["fk-kbd"]).build());
    }
    b
}

/// Shortcuts grouped into cards, filtered by `query`.
pub fn grid(all: &[Shortcut], query: &str) -> gtk::FlowBox {
    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .min_children_per_line(1)
        .max_children_per_line(3)
        .column_spacing(14)
        .row_spacing(14)
        .valign(gtk::Align::Start)
        .build();
    let q = query.to_lowercase();
    let mut groups: Vec<&str> = all.iter().map(|s| s.group.as_str()).collect();
    groups.dedup();
    for g in groups {
        let items: Vec<&Shortcut> = all
            .iter()
            .filter(|s| s.group == g)
            .filter(|s| q.is_empty() || s.action.to_lowercase().contains(&q) || s.keys.to_lowercase().contains(&q))
            .collect();
        if items.is_empty() {
            continue;
        }
        let card = vbox(8);
        card.add_css_class("fk-card");
        card.set_valign(gtk::Align::Start);
        card.append(&w::label(g, &["fk-card-title"]));
        for s in items {
            let row = hbox(12);
            let l = w::label(&s.action, &[]);
            l.set_hexpand(true);
            row.append(&l);
            row.append(&keys_widget(&s.keys));
            card.append(&row);
        }
        flow.append(&card);
    }
    flow
}

pub fn open(app: &adw::Application) {
    let win = adw::ApplicationWindow::builder()
        .application(app)
        .title("Keyboard shortcuts")
        .default_width(1100)
        .default_height(720)
        .build();
    let search = gtk::SearchEntry::builder().placeholder_text("Search shortcuts…").width_request(320).build();
    let header = adw::HeaderBar::builder().title_widget(&search).build();
    let all: Rc<Vec<Shortcut>> = Rc::new(cheatsheet::collect(Capabilities::detect().desktop));
    let holder = vbox(0);
    holder.set_margin_top(18);
    holder.set_margin_bottom(18);
    holder.set_margin_start(18);
    holder.set_margin_end(18);
    let fill = {
        let (all, holder) = (all.clone(), holder.clone());
        move |q: &str| {
            while let Some(c) = holder.first_child() {
                holder.remove(&c);
            }
            if all.is_empty() {
                holder.append(&w::empty_state(
                    "keyboard",
                    "No shortcuts found",
                    "This desktop's shortcut settings couldn't be read.",
                ));
            } else {
                holder.append(&grid(&all, q));
            }
        }
    };
    fill("");
    search.connect_search_changed(move |s| fill(&s.text()));
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(
        &gtk::ScrolledWindow::builder().child(&holder).hscrollbar_policy(gtk::PolicyType::Never).build(),
    ));
    win.set_content(Some(&view));

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let win = win.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                win.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    win.add_controller(keys);
    win.present();
    search.grab_focus();
}
