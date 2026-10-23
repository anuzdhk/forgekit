//! Small building blocks shared by every page.

use adw::prelude::*;
use forgekit_core::{Status, Tool};
use gtk::glib;

/// A Tabler icon (`tabler-<name>-symbolic`).
pub fn icon(name: &str, size: i32) -> gtk::Image {
    let i = gtk::Image::from_icon_name(&format!("tabler-{name}-symbolic"));
    i.set_pixel_size(size);
    i
}

pub fn icon_name(name: &str) -> String {
    format!("tabler-{name}-symbolic")
}

pub fn label(text: &str, classes: &[&str]) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(0.0).wrap(true).css_classes(classes.to_vec()).build()
}

pub fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

pub fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

/// A rounded tile with an icon in it, tinted by `tone` ("", "ok", "warn", "off", "err").
/// It's a single styled image (sized by CSS padding), so it never stretches its parents.
pub fn icon_tile(name: &str, big: bool, tone: &str) -> gtk::Image {
    let i = icon(name, if big { 30 } else { 20 });
    i.set_halign(gtk::Align::Center);
    i.set_valign(gtk::Align::Center);
    i.add_css_class("fk-icon-tile");
    i.add_css_class(if big { "big" } else { "small" });
    if !tone.is_empty() {
        i.add_css_class(tone);
    }
    i
}

/// What a tool's status looks like: (pill text, css tone).
pub fn status_style(tool: &Tool, status: &Status, enabled: bool) -> (String, &'static str) {
    if tool.is_planned() {
        return (format!("Coming in v{}", tool.since), "soon");
    }
    match status {
        _ if !enabled => ("Off".into(), "off"),
        Status::Supported => ("Ready".into(), "ok"),
        Status::NeedsSetup(_) => ("Needs setup".into(), "warn"),
        Status::Unsupported(_) => ("Not available".into(), "na"),
    }
}

pub fn pill(text: &str, tone: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Start)
        .css_classes(["fk-pill", tone])
        .build()
}

/// A card with an optional title/subtitle header and a body box.
pub fn card(title: &str, subtitle: &str) -> (gtk::Box, gtk::Box) {
    let card = vbox(14);
    card.add_css_class("fk-card");
    if !title.is_empty() {
        let head = vbox(2);
        head.append(&label(title, &["fk-card-title"]));
        if !subtitle.is_empty() {
            head.append(&label(subtitle, &["fk-card-subtitle"]));
        }
        card.append(&head);
    }
    let body = vbox(10);
    card.append(&body);
    (card, body)
}

/// Same as [`card`] but with a widget (buttons, a switch…) on the right of the header.
pub fn card_with_action(title: &str, subtitle: &str, action: &impl IsA<gtk::Widget>) -> (gtk::Box, gtk::Box) {
    let card = vbox(14);
    card.add_css_class("fk-card");
    let head = hbox(12);
    let text = vbox(2);
    text.set_hexpand(true);
    text.append(&label(title, &["fk-card-title"]));
    if !subtitle.is_empty() {
        text.append(&label(subtitle, &["fk-card-subtitle"]));
    }
    head.append(&text);
    action.set_valign(gtk::Align::Center);
    head.append(action);
    card.append(&head);
    let body = vbox(10);
    card.append(&body);
    (card, body)
}

pub fn button(label: &str, icon_name: Option<&str>, style: &str) -> gtk::Button {
    let b = gtk::Button::builder().valign(gtk::Align::Center).build();
    match icon_name {
        Some(i) => {
            let c = adw::ButtonContent::builder().label(label).icon_name(self::icon_name(i)).build();
            b.set_child(Some(&c));
        }
        None => b.set_label(label),
    }
    if !style.is_empty() {
        b.add_css_class(style);
    }
    b
}

pub fn icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(self::icon_name(icon_name))
        .tooltip_text(tooltip)
        .valign(gtk::Align::Center)
        .css_classes(["flat", "circular"])
        .build()
}

/// `forgekit eyedrop` with a copy button.
pub fn command_chip(command: &str, on_copied: impl Fn() + 'static) -> gtk::Box {
    let b = hbox(4);
    b.add_css_class("fk-chip");
    b.set_halign(gtk::Align::Start);
    b.set_valign(gtk::Align::Center);
    b.append(&icon("terminal-2", 14));
    b.append(&gtk::Label::builder().label(command).build());
    let copy = icon_button("copy", "Copy command");
    let cmd = command.to_string();
    copy.connect_clicked(move |_| {
        crate::util::copy(&cmd);
        on_copied();
    });
    b.append(&copy);
    b
}

/// Big centered icon, title and text for empty lists and unavailable tools.
pub fn empty_state(icon_name: &str, title: &str, text: &str) -> gtk::Box {
    let b = vbox(8);
    b.add_css_class("fk-empty");
    b.set_halign(gtk::Align::Center);
    let tile = icon_tile(icon_name, true, "off");
    b.append(&tile);
    let t =
        gtk::Label::builder().label(title).css_classes(["fk-empty-title"]).justify(gtk::Justification::Center).build();
    b.append(&t);
    if !text.is_empty() {
        b.append(
            &gtk::Label::builder()
                .label(text)
                .wrap(true)
                .max_width_chars(48)
                .justify(gtk::Justification::Center)
                .css_classes(["dim"])
                .build(),
        );
    }
    b
}

/// A simple table: a header row and a list of rows whose columns line up
/// through size groups. `widths` are minimum widths; `-1` = expand.
pub struct Table {
    pub root: gtk::Box,
    pub list: gtk::ListBox,
    groups: Vec<gtk::SizeGroup>,
    widths: Vec<i32>,
}

impl Table {
    pub fn new(headers: &[&str], widths: &[i32]) -> Self {
        let root = vbox(0);
        root.add_css_class("fk-card");
        root.add_css_class("flush");
        let groups: Vec<_> = headers.iter().map(|_| gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal)).collect();
        let head = hbox(12);
        head.add_css_class("fk-table-head");
        for (i, h) in headers.iter().enumerate() {
            let l = gtk::Label::builder().label(h.to_uppercase()).xalign(0.0).build();
            Self::size(&l, widths[i]);
            groups[i].add_widget(&l);
            head.append(&l);
        }
        root.append(&head);
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["fk-table"]).build();
        root.append(&list);
        Table { root, list, groups, widths: widths.to_vec() }
    }

    fn size(w: &impl IsA<gtk::Widget>, width: i32) {
        if width < 0 {
            w.set_hexpand(true);
        } else {
            w.set_size_request(width, -1);
        }
    }

    pub fn add(&self, cells: Vec<gtk::Widget>) -> gtk::ListBoxRow {
        let row_box = hbox(12);
        for (i, c) in cells.into_iter().enumerate() {
            c.set_valign(gtk::Align::Center);
            if let Some(&w) = self.widths.get(i) {
                Self::size(&c, w);
                self.groups[i].add_widget(&c);
            }
            row_box.append(&c);
        }
        let row = gtk::ListBoxRow::builder().child(&row_box).activatable(false).build();
        self.list.append(&row);
        row
    }

    pub fn clear(&self) {
        while let Some(row) = self.list.row_at_index(0) {
            if let Some(child) = row.child() {
                // Drop the size-group links before the widgets go away.
                let mut c = child.first_child();
                while let Some(w) = c {
                    for g in &self.groups {
                        g.remove_widget(&w);
                    }
                    c = w.next_sibling();
                }
            }
            self.list.remove(&row);
        }
    }
}

/// A plain text cell for [`Table::add`].
pub fn cell(text: &str, classes: &[&str]) -> gtk::Widget {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(classes.to_vec())
        .build()
        .upcast()
}

pub fn escape(s: &str) -> String {
    glib::markup_escape_text(s).to_string()
}

/// "3 min ago"
pub fn ago(unix_ms: i64) -> String {
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    let s = ((now - unix_ms) / 1000).max(0);
    match s {
        0..=44 => "just now".into(),
        45..=5399 => format!("{} min ago", (s / 60).max(1)),
        5400..=86399 => format!("{} h ago", s / 3600),
        _ => format!("{} d ago", s / 86400),
    }
}

/// Wrap page content so it doesn't stretch across very wide windows.
pub fn page(content: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let clamp = adw::Clamp::builder().maximum_size(1120).tightening_threshold(800).child(content).build();
    gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).child(&clamp).build()
}
