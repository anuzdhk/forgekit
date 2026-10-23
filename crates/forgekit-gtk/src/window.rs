//! The main window: sidebar navigation on the left, pages on the right.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use forgekit_core::catalog::Category;
use forgekit_core::settings::General;
use forgekit_core::{CATALOG, Capabilities, Status, Tool};
use gtk::glib;

use crate::theme::{self, Appearance};
use crate::widgets::{self as w, hbox, vbox};

pub struct App {
    pub window: adw::ApplicationWindow,
    pub caps: Capabilities,
    pub toasts: adw::ToastOverlay,
    pub appearance: RefCell<Appearance>,
    stack: gtk::Stack,
    nav: gtk::ListBox,
    title: adw::WindowTitle,
    split: adw::OverlaySplitView,
    search: gtk::SearchEntry,
    /// Page id of each sidebar row, by row index.
    row_ids: RefCell<Vec<Option<String>>>,
    built: RefCell<HashMap<String, gtk::Widget>>,
    me: Weak<App>,
}

impl App {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        let caps = Capabilities::detect();
        let toasts = adw::ToastOverlay::new();
        let stack =
            gtk::Stack::builder().transition_type(gtk::StackTransitionType::Crossfade).transition_duration(120).build();
        let nav = gtk::ListBox::builder().css_classes(["fk-nav"]).build();
        let title = adw::WindowTitle::new("Home", "");
        let split = adw::OverlaySplitView::builder().min_sidebar_width(250.0).max_sidebar_width(290.0).build();
        let search = gtk::SearchEntry::builder().placeholder_text("Search tools…").css_classes(["fk-search"]).build();

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Forgekit")
            .default_width(1180)
            .default_height(780)
            .width_request(360)
            .height_request(400)
            .build();

        let me = Rc::new_cyclic(|me| App {
            window,
            caps,
            toasts,
            appearance: RefCell::new(Appearance::load()),
            stack,
            nav,
            title,
            split,
            search,
            row_ids: RefCell::default(),
            built: RefCell::default(),
            me: me.clone(),
        });
        me.build();
        me.navigate("home");
        me
    }

    pub fn rc(&self) -> Rc<App> {
        self.me.upgrade().expect("app alive")
    }

    pub fn toast(&self, msg: &str) {
        let t = adw::Toast::new(msg);
        t.set_timeout(3);
        self.toasts.add_toast(t);
    }

    pub fn enabled(&self, id: &str) -> bool {
        General::load().unwrap_or_default().is_enabled(id)
    }

    fn build(&self) {
        // ---- sidebar
        let brand = hbox(10);
        brand.add_css_class("fk-brand");
        let mark = w::icon("hammer", 20);
        mark.add_css_class("fk-brand-mark");
        mark.set_valign(gtk::Align::Center);
        brand.append(&mark);
        let names = vbox(0);
        names.set_valign(gtk::Align::Center);
        names.append(&w::label("Forgekit", &["fk-brand-name"]));
        names.append(&w::label(&format!("v{} · power tools", env!("CARGO_PKG_VERSION")), &["fk-brand-version"]));
        names.set_hexpand(true);
        brand.append(&names);
        let collapse = w::icon_button("layout-sidebar-left-collapse", "Hide sidebar");
        {
            let split = self.split.clone();
            collapse.connect_clicked(move |_| split.set_show_sidebar(false));
        }
        brand.append(&collapse);

        let sidebar = vbox(0);
        sidebar.add_css_class("fk-sidebar");
        sidebar.append(&brand);
        sidebar.append(&self.search);
        sidebar.append(
            &gtk::ScrolledWindow::builder()
                .child(&self.nav)
                .vexpand(true)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .build(),
        );

        let footer = gtk::ListBox::builder().css_classes(["fk-nav", "fk-sidebar-footer"]).build();
        footer.append(&nav_row("settings", "Settings", None));
        {
            let me = self.me.clone();
            let nav = self.nav.clone();
            footer.connect_row_activated(move |f, _| {
                f.unselect_all();
                nav.unselect_all();
                if let Some(me) = me.upgrade() {
                    me.navigate("settings");
                }
            });
        }
        sidebar.append(&footer);
        self.split.set_sidebar(Some(&sidebar));

        self.fill_nav();
        {
            let me = self.me.clone();
            self.nav.connect_row_selected(move |_, row| {
                let (Some(me), Some(row)) = (me.upgrade(), row) else { return };
                let id = me.row_ids.borrow().get(row.index() as usize).cloned().flatten();
                if let Some(id) = id {
                    me.show(&id);
                }
            });
        }
        {
            let me = self.me.clone();
            self.search.connect_search_changed(move |_| {
                if let Some(me) = me.upgrade() {
                    me.nav.invalidate_filter();
                }
            });
            let me = self.me.clone();
            self.nav.set_filter_func(move |row| me.upgrade().is_none_or(|me| me.row_visible(row)));
            let nav = self.nav.clone();
            self.search.connect_activate(move |_| {
                // Enter opens the first match.
                let mut i = 0;
                while let Some(r) = nav.row_at_index(i) {
                    if r.is_child_visible() && r.is_selectable() {
                        nav.select_row(Some(&r));
                        break;
                    }
                    i += 1;
                }
            });
        }

        // ---- content
        let header = adw::HeaderBar::builder().title_widget(&self.title).build();
        let expand = w::icon_button("layout-sidebar-left-expand", "Show sidebar");
        self.split.bind_property("show-sidebar", &expand, "visible").invert_boolean().sync_create().build();
        {
            let split = self.split.clone();
            expand.connect_clicked(move |_| split.set_show_sidebar(true));
        }
        header.pack_start(&expand);
        header.pack_end(&self.theme_button());

        let content = adw::ToolbarView::new();
        content.add_top_bar(&header);
        content.set_content(Some(&self.stack));
        self.toasts.set_child(Some(&content));
        self.split.set_content(Some(&self.toasts));
        self.window.set_content(Some(&self.split));

        let bp = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            760.0,
            adw::LengthUnit::Sp,
        ));
        bp.add_setter(&self.split, "collapsed", Some(&true.to_value()));
        self.window.add_breakpoint(bp);

        // Ctrl+K searches tools, Ctrl+, opens settings.
        let keys = gtk::ShortcutController::new();
        keys.set_scope(gtk::ShortcutScope::Global);
        let search = self.search.clone();
        let split = self.split.clone();
        keys.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("<Control>k"),
            Some(gtk::CallbackAction::new(move |_, _| {
                split.set_show_sidebar(true);
                search.grab_focus();
                glib::Propagation::Stop
            })),
        ));
        let me = self.me.clone();
        keys.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("<Control>comma"),
            Some(gtk::CallbackAction::new(move |_, _| {
                if let Some(me) = me.upgrade() {
                    me.navigate("settings");
                }
                glib::Propagation::Stop
            })),
        ));
        self.window.add_controller(keys);
    }

    /// Popover with every theme, for switching without opening Settings.
    fn theme_button(&self) -> gtk::MenuButton {
        let list = gtk::ListBox::builder().css_classes(["navigation-sidebar"]).build();
        for t in theme::THEMES {
            let row = hbox(10);
            row.set_margin_top(2);
            row.set_margin_bottom(2);
            let dot = gtk::Label::builder().use_markup(true).build();
            let color = t.palette.map(|p| p.accent).unwrap_or("#888888");
            dot.set_markup(&format!("<span foreground='{color}'>●</span>"));
            row.append(&dot);
            row.append(&w::label(t.name, &[]));
            list.append(&row);
        }
        let pop = gtk::Popover::builder()
            .child(
                &gtk::ScrolledWindow::builder()
                    .child(&list)
                    .max_content_height(420)
                    .propagate_natural_height(true)
                    .min_content_width(220)
                    .build(),
            )
            .build();
        let me = self.me.clone();
        let p = pop.clone();
        list.connect_row_activated(move |_, row| {
            let Some(me) = me.upgrade() else { return };
            me.set_theme(theme::THEMES[row.index() as usize].id);
            p.popdown();
        });
        gtk::MenuButton::builder().icon_name(w::icon_name("palette")).tooltip_text("Theme").popover(&pop).build()
    }

    pub fn set_theme(&self, id: &str) {
        let mut a = self.appearance.borrow_mut();
        a.theme = id.to_string();
        a.save();
        crate::apply_theme(&a);
        drop(a);
        self.rebuild_page("settings");
    }

    /// Rebuild the sidebar (after a tool is switched on/off or visibility settings change).
    pub fn fill_nav(&self) {
        let current = self.stack.visible_child_name().map(|s| s.to_string());
        while let Some(r) = self.nav.row_at_index(0) {
            self.nav.remove(&r);
        }
        let mut ids = vec![Some("home".to_string())];
        self.nav.append(&nav_row("home", "Home", None));
        let a = self.appearance.borrow();
        let general = General::load().unwrap_or_default();
        for cat in Category::ALL {
            let tools: Vec<&Tool> = CATALOG
                .iter()
                .filter(|t| t.category == cat)
                .filter(|t| !(a.hide_planned && t.is_planned()))
                .filter(|t| {
                    !(a.hide_unavailable && matches!(t.status(&self.caps), Status::Unsupported(_)) && !t.is_planned())
                })
                .collect();
            if tools.is_empty() {
                continue;
            }
            let head = gtk::ListBoxRow::builder().selectable(false).activatable(false).build();
            head.set_child(Some(&w::label(&cat.title().to_uppercase(), &["fk-nav-header"])));
            self.nav.append(&head);
            ids.push(None);
            for t in tools {
                let s = t.status(&self.caps);
                let row = nav_row(t.id, t.name, Some((t, &s, general.is_enabled(t.id))));
                self.nav.append(&row);
                ids.push(Some(t.id.to_string()));
            }
        }
        *self.row_ids.borrow_mut() = ids;
        if let Some(c) = current {
            self.select_in_nav(&c);
        }
    }

    fn row_visible(&self, row: &gtk::ListBoxRow) -> bool {
        let q = self.search.text().to_lowercase();
        if q.is_empty() {
            return true;
        }
        let ids = self.row_ids.borrow();
        match ids.get(row.index() as usize).cloned().flatten() {
            None => false,
            Some(id) => match forgekit_core::catalog::find(&id) {
                Some(t) => {
                    t.name.to_lowercase().contains(&q) || t.summary.to_lowercase().contains(&q) || t.id.contains(&q)
                }
                None => id.contains(&q),
            },
        }
    }

    fn select_in_nav(&self, id: &str) {
        let index = self.row_ids.borrow().iter().position(|r| r.as_deref() == Some(id));
        match index.and_then(|i| self.nav.row_at_index(i as i32)) {
            Some(row) if self.nav.selected_row().as_ref() != Some(&row) => self.nav.select_row(Some(&row)),
            Some(_) => {}
            None => self.nav.unselect_all(),
        }
    }

    /// Go to a page by id ("home", "settings" or a tool id).
    pub fn navigate(&self, id: &str) {
        self.select_in_nav(id);
        self.show(id);
    }

    fn show(&self, id: &str) {
        let page = self.built.borrow().get(id).cloned();
        let page = page.unwrap_or_else(|| {
            let p = self.build_page(id);
            self.stack.add_named(&p, Some(id));
            self.built.borrow_mut().insert(id.to_string(), p.clone());
            p
        });
        self.stack.set_visible_child(&page);
        let (title, subtitle) = match id {
            "home" => ("Home".to_string(), "Your toolkit at a glance".to_string()),
            "settings" => ("Settings".to_string(), "Appearance and behavior".to_string()),
            _ => match forgekit_core::catalog::find(id) {
                Some(t) => (t.name.to_string(), t.category.title().to_string()),
                None => (id.to_string(), String::new()),
            },
        };
        self.title.set_title(&title);
        self.title.set_subtitle(&subtitle);
        if self.split.is_collapsed() {
            self.split.set_show_sidebar(false);
        }
    }

    fn build_page(&self, id: &str) -> gtk::Widget {
        let me = self.rc();
        match id {
            "home" => crate::home::build(&me),
            "settings" => crate::settings_page::build(&me),
            _ => match forgekit_core::catalog::find(id) {
                Some(t) => crate::pages::tool_page(t, &me),
                None => w::empty_state("info-circle", "Nothing here", "").upcast(),
            },
        }
    }

    /// Throw away a page so it's rebuilt fresh next time (or now, if visible).
    pub fn rebuild_page(&self, id: &str) {
        let old = self.built.borrow_mut().remove(id);
        if let Some(old) = old {
            let visible = self.stack.visible_child().as_ref() == Some(&old);
            if visible {
                self.show(id);
            }
            self.stack.remove(&old);
        }
    }

    /// A tool was switched on or off.
    pub fn tool_toggled(&self) {
        self.fill_nav();
        self.rebuild_page("home");
    }
}

fn nav_row(id: &str, title: &str, tool: Option<(&Tool, &Status, bool)>) -> gtk::ListBoxRow {
    let b = hbox(11);
    let icon = match (id, tool) {
        ("home", _) => "home",
        ("settings", _) => "settings",
        (_, Some((t, _, _))) => t.icon,
        _ => "circle-dashed",
    };
    b.append(&w::icon(icon, 18));
    let l = gtk::Label::builder().label(title).xalign(0.0).hexpand(true).css_classes(["title"]).build();
    b.append(&l);
    let row = gtk::ListBoxRow::builder().child(&b).build();
    if let Some((t, status, enabled)) = tool {
        if t.is_planned() {
            b.append(&gtk::Label::builder().label("soon").css_classes(["dim", "caption"]).build());
            row.add_css_class("unavailable");
        } else if !enabled {
            b.append(&gtk::Label::builder().label("off").css_classes(["dim", "caption"]).build());
            row.add_css_class("unavailable");
        } else {
            match status {
                Status::NeedsSetup(r) => {
                    let dot = gtk::Label::builder()
                        .label("●")
                        .css_classes(["fk-nav-dot", "warn"])
                        .tooltip_text(r.as_str())
                        .build();
                    b.append(&dot);
                }
                Status::Unsupported(r) => {
                    row.add_css_class("unavailable");
                    row.set_tooltip_text(Some(r));
                }
                Status::Supported => {}
            }
        }
    }
    row
}
