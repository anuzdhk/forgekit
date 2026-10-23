//! The Forgekit app: one window, a sidebar of tools grouped by tier, and a
//! page per tool with its on/off switch and an honest status line.

mod pages;
mod util;

use std::rc::Rc;

use adw::prelude::*;
use forgekit_core::settings::General;
use forgekit_core::{CATALOG, Capabilities, Status, Tool};

pub const APP_ID: &str = "io.github.anuzdhk.Forgekit";

fn main() -> gtk::glib::ExitCode {
    // `forgekit-gtk [tool]` opens on that tool's page.
    let args: Vec<String> = std::env::args().collect();
    let start = args.get(1).and_then(|a| forgekit_core::catalog::find(a)).map(|t| t.id).unwrap_or("spark");
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(move |app| build(app, start));
    app.run_with_args(&args[..1])
}

pub struct Ctx {
    pub caps: Capabilities,
    pub toasts: adw::ToastOverlay,
}

impl Ctx {
    pub fn toast(&self, msg: &str) {
        self.toasts.add_toast(adw::Toast::new(msg));
    }
}

fn build(app: &adw::Application, start: &str) {
    if let Some(w) = app.active_window() {
        w.present();
        return;
    }
    let ctx = Rc::new(Ctx { caps: Capabilities::detect(), toasts: adw::ToastOverlay::new() });

    let split = adw::NavigationSplitView::new();
    let content_page = adw::NavigationPage::builder().title("Forgekit").build();

    let list = gtk::ListBox::builder().css_classes(["navigation-sidebar"]).build();
    let general = General::load().unwrap_or_default();
    for tool in CATALOG {
        list.append(&sidebar_row(tool, &tool.status(&ctx.caps), general.is_enabled(tool.id)));
    }
    list.set_header_func(|row, before| {
        let tier = CATALOG[row.index() as usize].tier;
        let new_tier = before.is_none_or(|b| CATALOG[b.index() as usize].tier != tier);
        row.set_header(
            new_tier
                .then(|| {
                    gtk::Label::builder()
                        .label(tier.title())
                        .xalign(0.0)
                        .css_classes(["heading", "dim-label"])
                        .margin_top(12)
                        .margin_start(12)
                        .margin_bottom(4)
                        .build()
                })
                .as_ref(),
        );
    });

    {
        let ctx = ctx.clone();
        let split = split.clone();
        let content_page = content_page.clone();
        list.connect_row_selected(move |_, row| {
            let Some(row) = row else { return };
            let tool = &CATALOG[row.index() as usize];
            content_page.set_title(tool.name);
            content_page.set_child(Some(&content(tool, &ctx)));
            split.set_show_content(true);
        });
    }

    let sidebar_view = adw::ToolbarView::new();
    sidebar_view.add_top_bar(&adw::HeaderBar::builder().title_widget(&adw::WindowTitle::new("Forgekit", "")).build());
    sidebar_view.set_content(Some(&gtk::ScrolledWindow::builder().child(&list).vexpand(true).build()));
    split.set_sidebar(Some(&adw::NavigationPage::builder().title("Forgekit").child(&sidebar_view).build()));
    split.set_content(Some(&content_page));

    ctx.toasts.set_child(Some(&split));
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Forgekit")
        .default_width(1000)
        .default_height(700)
        .content(&ctx.toasts)
        .build();

    // Collapse the sidebar on narrow windows.
    let bp = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        640.0,
        adw::LengthUnit::Sp,
    ));
    bp.add_setter(&split, "collapsed", Some(&true.to_value()));
    window.add_breakpoint(bp);

    let index = CATALOG.iter().position(|t| t.id == start).unwrap_or(0);
    list.select_row(list.row_at_index(index as i32).as_ref());
    window.present();
}

fn status_icon(s: &Status) -> &'static str {
    match s {
        Status::Supported => "object-select-symbolic",
        Status::NeedsSetup(_) => "dialog-warning-symbolic",
        Status::Unsupported(_) => "action-unavailable-symbolic",
    }
}

fn sidebar_row(tool: &Tool, status: &Status, enabled: bool) -> gtk::ListBoxRow {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.set_margin_start(6);
    b.set_margin_end(6);
    b.append(&gtk::Label::builder().label(tool.name).xalign(0.0).hexpand(true).build());
    let icon = gtk::Image::from_icon_name(status_icon(status));
    icon.set_tooltip_text(Some(status.reason().unwrap_or("Supported")));
    if !status.is_supported() || !enabled {
        icon.add_css_class("dim-label");
    }
    b.append(&icon);
    let row = gtk::ListBoxRow::builder().child(&b).build();
    if !status.is_supported() && matches!(status, Status::Unsupported(_)) {
        row.add_css_class("dim-label");
    }
    row
}

/// The page for one tool: header bar, status/enable group, then the tool's own UI.
fn content(tool: &'static Tool, ctx: &Rc<Ctx>) -> gtk::Widget {
    let status = tool.status(&ctx.caps);
    let page = adw::PreferencesPage::new();

    let head = adw::PreferencesGroup::builder().title(tool.name).description(tool.summary).build();
    let switch = adw::SwitchRow::builder()
        .title("Enabled")
        .active(General::load().unwrap_or_default().is_enabled(tool.id))
        .sensitive(!matches!(status, Status::Unsupported(_)))
        .build();
    head.add(&switch);
    let status_row = adw::ActionRow::builder()
        .title(match &status {
            Status::Supported => "Works on this desktop",
            Status::NeedsSetup(_) => "Needs setup",
            Status::Unsupported(_) => "Not available here",
        })
        .subtitle(status.reason().unwrap_or(""))
        .build();
    status_row.add_prefix(&gtk::Image::from_icon_name(status_icon(&status)));
    head.add(&status_row);
    let cli = adw::ActionRow::builder()
        .title("From the terminal")
        .subtitle(format!("forgekit {}", tool.id))
        .subtitle_selectable(true)
        .build();
    head.add(&cli);
    page.add(&head);

    let body: Vec<adw::PreferencesGroup> =
        if matches!(status, Status::Unsupported(_)) { Vec::new() } else { pages::build(tool.id, ctx) };
    for g in &body {
        g.set_sensitive(switch.is_active());
        page.add(g);
    }
    {
        let ctx = ctx.clone();
        switch.connect_active_notify(move |s| {
            let mut g = General::load().unwrap_or_default();
            g.set_enabled(tool.id, s.is_active());
            if let Err(e) = g.save() {
                ctx.toast(&format!("Couldn't save settings: {e}"));
            }
            for grp in &body {
                grp.set_sensitive(s.is_active());
            }
        });
    }

    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&page));
    view.upcast()
}
