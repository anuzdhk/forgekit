//! The Cheatsheet page.

use std::rc::Rc;

use adw::prelude::*;
use forgekit_core::tools::cheatsheet;

use crate::widgets::{self as w, hbox, vbox};
use crate::window::App;

type Cards = Vec<gtk::Widget>;

pub fn cheatsheet(app: &Rc<App>) -> Cards {
    let card = hbox(16);
    card.add_css_class("fk-card");
    let t = vbox(4);
    t.set_hexpand(true);
    t.append(&w::label("All your shortcuts in one window", &["fk-card-title"]));
    t.append(&w::label(
        "Give it a shortcut above (Super+/ works well) to pull it up whenever you forget one.",
        &["dim"],
    ));
    card.append(&t);
    let open = w::button("Open", Some("keyboard-show"), "suggested-action");
    open.add_css_class("pill");
    let win = app.window.clone();
    open.connect_clicked(move |_| {
        if let Some(a) = win.application().and_downcast::<adw::Application>() {
            crate::cheatsheet::open(&a);
        }
    });
    card.append(&open);
    let all = cheatsheet::collect(app.caps.desktop);
    let list: gtk::Widget = if all.is_empty() {
        w::empty_state("keyboard", "No shortcuts found", "This desktop's shortcut settings couldn't be read.").upcast()
    } else {
        crate::cheatsheet::grid(&all, "").upcast()
    };
    vec![card.upcast(), list]
}
