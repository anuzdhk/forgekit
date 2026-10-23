//! The tool-specific part of each page.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use forgekit_core::tools::{eyedrop, hostbook, lockpick, nightowl, relabel, shrinkray, spark, teku};
use gtk::glib;

use crate::Ctx;
use crate::util::{self, Out};

type Groups = Vec<adw::PreferencesGroup>;

pub fn build(id: &str, ctx: &Rc<Ctx>) -> Groups {
    match id {
        "spark" => spark_page(ctx),
        "eyedrop" => eyedrop_page(ctx),
        "nightowl" => nightowl_page(ctx),
        "glyphgrab" => glyphgrab_page(ctx),
        "relabel" => relabel_page(ctx),
        "shrinkray" => shrinkray_page(ctx),
        "hostbook" => hostbook_page(ctx),
        "lockpick" => lockpick_page(ctx),
        "teku" => teku_page(ctx),
        _ => Vec::new(),
    }
}

fn group(title: &str, description: &str) -> adw::PreferencesGroup {
    adw::PreferencesGroup::builder()
        .title(glib::markup_escape_text(title))
        .description(glib::markup_escape_text(description))
        .build()
}

fn boxed_list() -> gtk::ListBox {
    gtk::ListBox::builder().css_classes(["boxed-list"]).selection_mode(gtk::SelectionMode::None).build()
}

fn button(label: &str, suggested: bool) -> gtk::Button {
    let b = gtk::Button::builder().label(label).valign(gtk::Align::Center).build();
    if suggested {
        b.add_css_class("suggested-action");
    }
    b
}

fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build()
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// A "Files" row with Choose/Clear buttons that also accepts drops.
fn files_row(files: Rc<RefCell<Vec<PathBuf>>>, changed: impl Fn() + 'static) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title("Files").subtitle("Choose or drop files here").build();
    let choose = button("Choose…", false);
    let clear = icon_button("edit-clear-symbolic", "Clear");
    row.add_suffix(&choose);
    row.add_suffix(&clear);
    let changed = Rc::new(changed);
    let set = {
        let row = row.clone();
        let files = files.clone();
        let changed = changed.clone();
        Rc::new(move |v: Vec<PathBuf>| {
            *files.borrow_mut() = v;
            let n = files.borrow().len();
            row.set_subtitle(&match n {
                0 => "Choose or drop files here".to_string(),
                1 => file_name(&files.borrow()[0]),
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

// ------------------------------------------------------------------ Spark

fn spark_page(ctx: &Rc<Ctx>) -> Groups {
    let g = group("Search", "Apps, calculator (2+2), units (10 km to mi), install <file>, web search");
    let entry = gtk::SearchEntry::builder().placeholder_text("Type to search…").build();
    let results = boxed_list();
    results.set_selection_mode(gtk::SelectionMode::Single);
    results.set_margin_top(12);
    let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
    b.append(&entry);
    b.append(&results);
    g.add(&b);

    let apps = Rc::new(spark::load_apps());
    let items: Rc<RefCell<Vec<spark::Item>>> = Rc::default();
    {
        let results = results.clone();
        let items = items.clone();
        entry.connect_search_changed(move |e| {
            results.remove_all();
            *items.borrow_mut() = spark::query(&e.text(), &apps);
            for it in items.borrow().iter() {
                let row = adw::ActionRow::builder()
                    .title(glib::markup_escape_text(&it.title))
                    .subtitle(glib::markup_escape_text(&it.subtitle))
                    .activatable(true)
                    .build();
                row.add_prefix(&gtk::Image::from_icon_name(&it.icon));
                results.append(&row);
            }
            if let Some(first) = results.row_at_index(0) {
                results.select_row(Some(&first));
            }
        });
    }
    let run = {
        let ctx = ctx.clone();
        let items = items.clone();
        Rc::new(move |i: usize| {
            let Some(it) = items.borrow().get(i).cloned() else { return };
            match &it.action {
                spark::Action::Copy { text } => {
                    util::copy(text);
                    ctx.toast(&format!("Copied {text}"));
                }
                a => {
                    if let Err(e) = spark::run(a) {
                        ctx.toast(&format!("{e:#}"));
                    }
                }
            }
        })
    };
    {
        let run = run.clone();
        results.connect_row_activated(move |_, row| run(row.index() as usize));
    }
    {
        let results = results.clone();
        entry.connect_activate(move |_| {
            if let Some(r) = results.selected_row() {
                run(r.index() as usize);
            }
        });
    }
    vec![g]
}

// ------------------------------------------------------------------ Eyedrop

fn eyedrop_page(ctx: &Rc<Ctx>) -> Groups {
    let pick = group("Pick", "Click the button, then click anywhere on screen");
    let pick_row = adw::ActionRow::builder().title("Pick a color").build();
    let pick_btn = button("Pick", true);
    pick_row.add_suffix(&pick_btn);
    pick.add(&pick_row);

    let current = group("Color", "");
    let rows: Vec<(eyedrop::Format, adw::ActionRow)> =
        [eyedrop::Format::Hex, eyedrop::Format::Rgb, eyedrop::Format::Hsl]
            .into_iter()
            .map(|f| {
                let row =
                    adw::ActionRow::builder().title(format!("{f:?}").to_uppercase()).subtitle_selectable(true).build();
                let copy = icon_button("edit-copy-symbolic", "Copy");
                let r = row.clone();
                let ctx = ctx.clone();
                copy.connect_clicked(move |_| {
                    let t = r.subtitle().unwrap_or_default();
                    util::copy(&t);
                    ctx.toast(&format!("Copied {t}"));
                });
                row.add_suffix(&copy);
                current.add(&row);
                (f, row)
            })
            .collect();

    let history = group("History", "Click a color to copy it");
    let hist_list = boxed_list();
    history.add(&hist_list);

    let show = {
        let rows = rows.clone();
        let current = current.clone();
        Rc::new(move |c: Option<eyedrop::Rgb>| {
            current.set_visible(c.is_some());
            if let Some(c) = c {
                for (f, row) in &rows {
                    row.set_subtitle(&c.format(*f));
                }
                current.set_description(Some(&format!(
                    "<span size='xx-large' foreground='{0}'>■■■■</span> {0}",
                    c.format(eyedrop::Format::Hex)
                )));
            }
        })
    };
    let fill_history = {
        let ctx = ctx.clone();
        Rc::new(move || {
            hist_list.remove_all();
            for c in eyedrop::history() {
                let hex = c.format(eyedrop::Format::Hex);
                let row = adw::ActionRow::builder()
                    .title(format!("<span foreground='{hex}'>■</span>  {hex}"))
                    .subtitle(c.format(eyedrop::Format::Rgb))
                    .activatable(true)
                    .build();
                let ctx = ctx.clone();
                row.connect_activated(move |_| {
                    util::copy(&hex);
                    ctx.toast(&format!("Copied {hex}"));
                });
                hist_list.append(&row);
            }
        })
    };
    show(eyedrop::history().first().copied());
    fill_history();

    let ctx = ctx.clone();
    pick_btn.connect_clicked(move |b| {
        b.set_sensitive(false);
        let b = b.clone();
        let show = show.clone();
        let fill_history = fill_history.clone();
        let ctx = ctx.clone();
        util::background(
            || util::cli_output(&["eyedrop", "--no-copy"]),
            move |r| {
                b.set_sensitive(true);
                match r {
                    Ok(hex) => {
                        util::copy(&hex);
                        ctx.toast(&format!("Copied {hex}"));
                        show(eyedrop::history().first().copied());
                        fill_history();
                    }
                    Err(e) => ctx.toast(&e),
                }
            },
        );
    });
    vec![pick, current, history]
}

// ------------------------------------------------------------------ Nightowl

thread_local! {
    // Lives with the app, not the page, so switching pages keeps the PC awake.
    static INHIBITOR: RefCell<Option<nightowl::Inhibitor>> = const { RefCell::new(None) };
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

fn nightowl_page(ctx: &Rc<Ctx>) -> Groups {
    let g = group("Stay awake", "Blocks sleep and screen blanking while on");
    let durations = [
        ("Until I turn it off", 0u32),
        ("30 minutes", 30 * 60),
        ("1 hour", 3600),
        ("2 hours", 7200),
        ("4 hours", 4 * 3600),
    ];
    let combo = adw::ComboRow::builder().title("For").model(&gtk::StringList::new(&durations.map(|d| d.0))).build();
    let active = INHIBITOR.with_borrow(|i| i.is_some());
    let switch = adw::SwitchRow::builder().title("Keep awake").active(active).build();
    g.add(&switch);
    g.add(&combo);

    let ctx = ctx.clone();
    switch.connect_active_notify(move |s| {
        let generation = GENERATION.with(|g| {
            g.set(g.get() + 1);
            g.get()
        });
        if !s.is_active() {
            INHIBITOR.set(None);
            return;
        }
        let secs = durations[combo.selected() as usize].1;
        match nightowl::Inhibitor::acquire("Forgekit Nightowl") {
            Ok(i) => {
                ctx.toast(&format!("Nightowl: {}", i.describe()));
                INHIBITOR.set(Some(i));
            }
            Err(e) => {
                ctx.toast(&format!("{e:#}"));
                s.set_active(false);
                return;
            }
        }
        if secs > 0 {
            let s = s.downgrade();
            glib::timeout_add_seconds_local_once(secs, move || {
                if GENERATION.get() == generation {
                    INHIBITOR.set(None);
                    if let Some(s) = s.upgrade() {
                        s.set_active(false);
                    }
                }
            });
        }
    });
    vec![g]
}

// ------------------------------------------------------------------ Glyphgrab

fn glyphgrab_page(ctx: &Rc<Ctx>) -> Groups {
    let g = group("Grab text", "Select an area of the screen; its text is copied");
    let row = adw::ActionRow::builder().title("Select area").build();
    let btn = button("Grab", true);
    row.add_suffix(&btn);
    g.add(&row);
    let out = group("Text", "");
    let (view, buf) = util::log_view();
    out.add(&view);

    let ctx = ctx.clone();
    btn.connect_clicked(move |b| {
        b.set_sensitive(false);
        let b = b.clone();
        let buf = buf.clone();
        let ctx = ctx.clone();
        util::background(
            || util::cli_output(&["glyphgrab", "--no-copy"]),
            move |r| {
                b.set_sensitive(true);
                match r {
                    Ok(text) => {
                        buf.set_text(&text);
                        util::copy(&text);
                        ctx.toast("Text copied");
                    }
                    Err(e) => ctx.toast(&e),
                }
            },
        );
    });
    vec![g, out]
}

// ------------------------------------------------------------------ Relabel

fn relabel_page(ctx: &Rc<Ctx>) -> Groups {
    let files: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
    let opts = group("Rename", "Template fields: {name} {ext} {n} {n:3}");
    let find = adw::EntryRow::builder().title("Find").build();
    let replace = adw::EntryRow::builder().title("Replace with").build();
    let template = adw::EntryRow::builder().title("Template (optional)").build();
    let regex = adw::SwitchRow::builder().title("Regular expression").build();
    let icase = adw::SwitchRow::builder().title("Ignore case").build();
    let ext = adw::SwitchRow::builder().title("Include extension").build();

    let preview = group("Preview", "");
    let list = boxed_list();
    preview.add(&list);
    let apply = button("Rename", true);
    preview.set_header_suffix(Some(&apply));

    let plan: Rc<RefCell<Vec<relabel::Rename>>> = Rc::default();
    let refresh = {
        let (files, find, replace, template, regex, icase, ext, plan, apply) = (
            files.clone(),
            find.clone(),
            replace.clone(),
            template.clone(),
            regex.clone(),
            icase.clone(),
            ext.clone(),
            plan.clone(),
            apply.clone(),
        );
        let preview = preview.clone();
        Rc::new(move || {
            list.remove_all();
            let o = relabel::Options {
                find: find.text().into(),
                replace: replace.text().into(),
                regex: regex.is_active(),
                ignore_case: icase.is_active(),
                keep_extension: !ext.is_active(),
                template: Some(template.text().to_string()).filter(|t| !t.is_empty()),
                start: 1,
            };
            match relabel::plan(&files.borrow(), &o) {
                Ok(p) => {
                    preview.set_description(None);
                    for r in &p {
                        let row = adw::ActionRow::builder()
                            .title(glib::markup_escape_text(&file_name(&r.to)))
                            .subtitle(glib::markup_escape_text(&match &r.problem {
                                Some(e) => format!("{} — {e}", file_name(&r.from)),
                                None => file_name(&r.from),
                            }))
                            .build();
                        if r.problem.is_some() {
                            row.add_css_class("error");
                        }
                        list.append(&row);
                    }
                    let ok = p.iter().any(|r| r.changes()) && p.iter().all(|r| r.problem.is_none());
                    apply.set_sensitive(ok);
                    *plan.borrow_mut() = p;
                }
                Err(e) => {
                    preview.set_description(Some(&glib::markup_escape_text(&format!("{e:#}"))));
                    apply.set_sensitive(false);
                    plan.borrow_mut().clear();
                }
            }
        })
    };
    let r = refresh.clone();
    opts.add(&files_row(files.clone(), move || r()));
    for e in [&find, &replace, &template] {
        let r = refresh.clone();
        e.connect_changed(move |_| r());
        opts.add(e);
    }
    for s in [&regex, &icase, &ext] {
        let r = refresh.clone();
        s.connect_active_notify(move |_| r());
        opts.add(s);
    }
    refresh();

    let ctx = ctx.clone();
    apply.connect_clicked(move |_| {
        let p = plan.borrow().clone();
        match relabel::apply(&p) {
            Ok(n) => {
                ctx.toast(&format!("Renamed {n} file(s)"));
                *files.borrow_mut() = p.into_iter().map(|r| r.to).collect();
                refresh();
            }
            Err(e) => ctx.toast(&format!("{e:#}")),
        }
    });
    vec![opts, preview]
}

// ------------------------------------------------------------------ Shrinkray

fn shrinkray_page(ctx: &Rc<Ctx>) -> Groups {
    let files: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
    let g = group("Resize", "Images are scaled to fit, keeping their shape. 0 = no limit.");
    g.add(&files_row(files.clone(), || {}));
    let width = adw::SpinRow::with_range(0.0, 20000.0, 10.0);
    width.set_title("Max width");
    width.set_value(1920.0);
    let height = adw::SpinRow::with_range(0.0, 20000.0, 10.0);
    height.set_title("Max height");
    let formats = ["Keep", "png", "jpg", "webp", "bmp", "tiff"];
    let format = adw::ComboRow::builder().title("Convert to").model(&gtk::StringList::new(&formats)).build();
    let in_place = adw::SwitchRow::builder().title("Replace originals").build();
    for w in [width.upcast_ref::<gtk::Widget>(), height.upcast_ref(), format.upcast_ref(), in_place.upcast_ref()] {
        g.add(w);
    }
    let go = button("Resize", true);
    g.set_header_suffix(Some(&go));

    let ctx = ctx.clone();
    go.connect_clicked(move |b| {
        let list = files.borrow().clone();
        if list.is_empty() {
            ctx.toast("Choose some images first");
            return;
        }
        let nz = |v: f64| Some(v as u32).filter(|v| *v > 0);
        let opts = shrinkray::Options {
            width: nz(width.value()),
            height: nz(height.value()),
            upscale: false,
            format: Some(formats[format.selected() as usize].to_string()).filter(|f| f != "Keep"),
            out_dir: None,
            in_place: in_place.is_active(),
        };
        b.set_sensitive(false);
        let b = b.clone();
        let ctx = ctx.clone();
        util::background(
            move || {
                list.iter()
                    .map(|f| shrinkray::process(f, &opts).map_err(|e| format!("{}: {e:#}", file_name(f))))
                    .collect::<Vec<_>>()
            },
            move |res| {
                b.set_sensitive(true);
                let failed: Vec<_> = res.iter().filter_map(|r| r.as_ref().err()).collect();
                match failed.first() {
                    None => ctx.toast(&format!("Resized {} image(s)", res.len())),
                    Some(e) => ctx.toast(&format!("{} failed — {e}", failed.len())),
                }
            },
        );
    });
    vec![g]
}

// ------------------------------------------------------------------ Hostbook

fn hostbook_page(ctx: &Rc<Ctx>) -> Groups {
    let path = PathBuf::from(hostbook::HOSTS);
    let hosts = match hostbook::Hosts::load(&path) {
        Ok(h) => Rc::new(RefCell::new(h)),
        Err(e) => {
            ctx.toast(&format!("{e:#}"));
            return Vec::new();
        }
    };
    let entries = group("Entries", "Switch entries off instead of deleting them");
    let save = button("Save", true);
    save.set_sensitive(false);
    entries.set_header_suffix(Some(&save));
    let list = boxed_list();
    entries.add(&list);

    let fill: Rc<Box<dyn Fn()>> = {
        let hosts = hosts.clone();
        let save = save.clone();
        let list = list.clone();
        Rc::new_cyclic(|weak: &std::rc::Weak<Box<dyn Fn()>>| {
            let weak = weak.clone();
            Box::new(move || {
                list.remove_all();
                for e in hosts.borrow().entries() {
                    let name = e.names[0].clone();
                    let row = adw::SwitchRow::builder()
                        .title(glib::markup_escape_text(&e.names.join(" ")))
                        .subtitle(glib::markup_escape_text(&match &e.comment {
                            Some(c) => format!("{} · {c}", e.ip),
                            None => e.ip.to_string(),
                        }))
                        .active(e.enabled)
                        .build();
                    let del = icon_button("user-trash-symbolic", "Remove");
                    row.add_suffix(&del);
                    {
                        let (hosts, save, name) = (hosts.clone(), save.clone(), name.clone());
                        row.connect_active_notify(move |r| {
                            hosts.borrow_mut().set_enabled(&name, r.is_active());
                            save.set_sensitive(true);
                        });
                    }
                    {
                        let (hosts, save, weak) = (hosts.clone(), save.clone(), weak.clone());
                        let names = e.names.clone();
                        del.connect_clicked(move |_| {
                            for n in &names {
                                hosts.borrow_mut().remove(n);
                            }
                            save.set_sensitive(true);
                            if let Some(f) = weak.upgrade() {
                                // Rebuild after this handler returns; we're inside a row being removed.
                                glib::idle_add_local_once(move || f());
                            }
                        });
                    }
                    list.append(&row);
                }
            })
        })
    };
    fill();

    let add = group("Add entry", "");
    let ip = adw::EntryRow::builder().title("IP address").build();
    let names = adw::EntryRow::builder().title("Host names (space-separated)").build();
    let add_btn = button("Add", false);
    add.set_header_suffix(Some(&add_btn));
    add.add(&ip);
    add.add(&names);
    {
        let (ctx, hosts, save, fill) = (ctx.clone(), hosts.clone(), save.clone(), fill.clone());
        add_btn.connect_clicked(move |_| {
            let Ok(addr) = ip.text().trim().parse() else {
                ctx.toast("That isn't a valid IP address");
                return;
            };
            let n: Vec<String> = names.text().split_whitespace().map(str::to_string).collect();
            if let Err(e) = hosts.borrow_mut().add(addr, n) {
                ctx.toast(&format!("{e:#}"));
                return;
            }
            ip.set_text("");
            names.set_text("");
            save.set_sensitive(true);
            fill();
        });
    }
    {
        let ctx = ctx.clone();
        save.connect_clicked(move |b| {
            b.set_sensitive(false);
            let h = hosts.borrow().clone();
            let path = path.clone();
            let ctx = ctx.clone();
            let b = b.clone();
            util::background(
                move || h.save(&path).map_err(|e| format!("{e:#}")),
                move |r| match r {
                    Ok(backup) => ctx.toast(&format!("Saved. Backup: {}", backup.display())),
                    Err(e) => {
                        b.set_sensitive(true);
                        ctx.toast(&e);
                    }
                },
            );
        });
    }
    vec![entries, add]
}

// ------------------------------------------------------------------ Lockpick

fn lockpick_page(ctx: &Rc<Ctx>) -> Groups {
    let g = group("Find what's using a file", "Choose or drop a file or folder");
    let target: Rc<RefCell<Option<PathBuf>>> = Rc::default();
    let row = adw::ActionRow::builder().title("Path").subtitle("Nothing chosen").build();
    let file_btn = button("File…", false);
    let dir_btn = button("Folder…", false);
    let refresh_btn = icon_button("view-refresh-symbolic", "Check again");
    row.add_suffix(&file_btn);
    row.add_suffix(&dir_btn);
    row.add_suffix(&refresh_btn);
    g.add(&row);

    let results = group("Processes", "");
    let list = boxed_list();
    results.add(&list);

    let scan: Rc<Box<dyn Fn()>> = Rc::new_cyclic(|weak: &std::rc::Weak<Box<dyn Fn()>>| {
        let (weak, target, list, results, ctx) =
            (weak.clone(), target.clone(), list.clone(), results.clone(), ctx.clone());
        Box::new(move || {
            list.remove_all();
            let Some(p) = target.borrow().clone() else { return };
            match lockpick::holders(&p) {
                Ok(h) if h.is_empty() => results.set_description(Some("Nothing you can see is using it.")),
                Ok(h) => {
                    results.set_description(Some(&format!("{} process(es)", h.len())));
                    for holder in h {
                        let r = adw::ActionRow::builder()
                            .title(glib::markup_escape_text(&format!("{} ({})", holder.name, holder.pid)))
                            .subtitle(glib::markup_escape_text(&format!(
                                "{:?} · {}",
                                holder.hold,
                                holder.path.display()
                            )))
                            .build();
                        let close = button("Close", false);
                        close.add_css_class("destructive-action");
                        let (ctx, weak) = (ctx.clone(), weak.clone());
                        close.connect_clicked(move |_| {
                            match lockpick::close(holder.pid, false) {
                                Ok(()) => ctx.toast(&format!("Asked {} to quit", holder.name)),
                                Err(e) => ctx.toast(&format!("Couldn't close {}: {e}", holder.name)),
                            }
                            if let Some(f) = weak.upgrade() {
                                glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || f());
                            }
                        });
                        r.add_suffix(&close);
                        list.append(&r);
                    }
                }
                Err(e) => results.set_description(Some(&glib::markup_escape_text(&e.to_string()))),
            }
        })
    });
    let set = {
        let (target, row, scan) = (target.clone(), row.clone(), scan.clone());
        Rc::new(move |v: Vec<PathBuf>| {
            if let Some(p) = v.into_iter().next() {
                row.set_subtitle(&glib::markup_escape_text(&p.display().to_string()));
                *target.borrow_mut() = Some(p);
                scan();
            }
        })
    };
    for (btn, folder) in [(&file_btn, false), (&dir_btn, true)] {
        let set = set.clone();
        btn.connect_clicked(move |b| {
            let set = set.clone();
            util::pick_files(b, false, folder, move |v| set(v));
        });
    }
    refresh_btn.connect_clicked(move |_| scan());
    util::on_drop(&row, move |v| set(v));
    vec![g, results]
}

// ------------------------------------------------------------------ Teku

fn teku_page(ctx: &Rc<Ctx>) -> Groups {
    let g = group("Install a package", ".deb, .rpm, .AppImage or .flatpakref — choose or drop it here");
    let file: Rc<RefCell<Option<PathBuf>>> = Rc::default();
    let row = adw::ActionRow::builder().title("Package").subtitle("Nothing chosen").build();
    let choose = button("Choose…", false);
    let install = button("Install", true);
    install.set_sensitive(false);
    row.add_suffix(&choose);
    row.add_suffix(&install);
    g.add(&row);
    let (log, buf) = util::log_view();
    log.set_margin_top(12);
    log.set_visible(false);
    g.add(&log);

    let history = group("History", "");
    let hist_list = boxed_list();
    history.add(&hist_list);
    let fill_history: Rc<Box<dyn Fn()>> = Rc::new_cyclic(|weak: &std::rc::Weak<Box<dyn Fn()>>| {
        let (weak, ctx, hist_list) = (weak.clone(), ctx.clone(), hist_list.clone());
        Box::new(move || {
            hist_list.remove_all();
            for h in teku::history().into_iter().take(30) {
                let r = adw::ActionRow::builder()
                    .title(glib::markup_escape_text(&file_name(&h.file)))
                    .subtitle(format!("{} · {} · {}", h.kind, if h.ok { "installed" } else { "failed" }, h.when))
                    .build();
                if h.ok && h.kind == "appimage" {
                    let rm = icon_button("user-trash-symbolic", "Remove this AppImage");
                    let (ctx, weak, f) = (ctx.clone(), weak.clone(), h.file.clone());
                    rm.connect_clicked(move |_| {
                        let (ctx, weak, f) = (ctx.clone(), weak.clone(), f.clone());
                        util::background(
                            move || teku::uninstall_appimage(&f).map_err(|e| format!("{e:#}")),
                            move |r| {
                                ctx.toast(&match r {
                                    Ok(()) => "AppImage removed".to_string(),
                                    Err(e) => e,
                                });
                                if let Some(f) = weak.upgrade() {
                                    f();
                                }
                            },
                        );
                    });
                    r.add_suffix(&rm);
                }
                hist_list.append(&r);
            }
        })
    });
    fill_history();

    let set = {
        let (file, row, install, ctx) = (file.clone(), row.clone(), install.clone(), ctx.clone());
        Rc::new(move |v: Vec<PathBuf>| {
            let Some(p) = v.into_iter().next() else { return };
            row.set_title(&glib::markup_escape_text(&file_name(&p)));
            row.set_subtitle("Checking…");
            install.set_sensitive(false);
            *file.borrow_mut() = Some(p.clone());
            let (row, install, ctx) = (row.clone(), install.clone(), ctx.clone());
            util::background(
                move || teku::detect(&[p]),
                move |r| match r.map(|mut v| v.pop()) {
                    Ok(Some(d)) => match d.route {
                        teku::Route::Install { how } => {
                            row.set_subtitle(&glib::markup_escape_text(&format!(
                                "{} package · installs via {how}",
                                d.kind
                            )));
                            install.set_sensitive(true);
                        }
                        teku::Route::Error { title, detail } => {
                            row.set_subtitle(&glib::markup_escape_text(&format!("{title}. {detail}")));
                        }
                    },
                    Ok(None) => row.set_subtitle("Teku didn't recognise this file"),
                    Err(e) => ctx.toast(&format!("{e:#}")),
                },
            );
        })
    };
    {
        let set = set.clone();
        choose.connect_clicked(move |b| {
            let set = set.clone();
            util::pick_files(b, false, false, move |v| set(v));
        });
    }
    util::on_drop(&row, move |v| set(v));

    let ctx = ctx.clone();
    install.connect_clicked(move |b| {
        let Some(f) = file.borrow().clone() else { return };
        b.set_sensitive(false);
        log.set_visible(true);
        buf.set_text("");
        let mut cmd = util::cli();
        cmd.args(["teku", "install"]).arg(&f);
        let (buf, ctx, fill_history, b) = (buf.clone(), ctx.clone(), fill_history.clone(), b.clone());
        util::stream(cmd, move |m| match m {
            Out::Line(l) => util::append(&buf, &l),
            Out::Done(r) => {
                b.set_sensitive(true);
                ctx.toast(&match r {
                    Ok(()) => format!("Installed {}", file_name(&f)),
                    Err(_) => format!("Couldn't install {}", file_name(&f)),
                });
                fill_history();
            }
        });
    });
    vec![g, history]
}
