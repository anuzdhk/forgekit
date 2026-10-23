//! Relabel, Shrinkray, Lockpick and Teku pages.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use forgekit_core::tools::{lockpick, relabel, shrinkray, teku};
use gtk::glib;

use super::{boxed_list, files_row};
use crate::util::{self, Out};
use crate::widgets as w;
use crate::window::App;

type Cards = Vec<gtk::Widget>;

// ------------------------------------------------------------------ Relabel

pub fn relabel(app: &Rc<App>) -> Cards {
    let files: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
    let (opts, opts_body) =
        w::card("Rename rules", "Template fields: {name} {ext} {n} {n:3}. Nothing changes until you press Rename.");
    let rows = boxed_list();
    let find = adw::EntryRow::builder().title("Find").build();
    let replace = adw::EntryRow::builder().title("Replace with").build();
    let template = adw::EntryRow::builder().title("Template (optional), e.g. {n:3} {name}").build();
    let regex = adw::SwitchRow::builder().title("Regular expression").subtitle("Use $1, $2 in “Replace with”").build();
    let icase = adw::SwitchRow::builder().title("Ignore case").build();
    let ext = adw::SwitchRow::builder().title("Include extension").build();

    let apply = w::button("Rename", Some("check"), "suggested-action");
    apply.set_sensitive(false);
    let table = Rc::new(w::Table::new(&["Before", "After", ""], &[-1, -1, 24]));
    let (preview, preview_body) = w::card_with_action("Preview", "", &apply);
    preview_body.append(&table.root);
    let empty = w::empty_state("forms", "Add some files", "Choose or drop files above to see how they'll be renamed.");
    preview_body.append(&empty);

    let plan: Rc<RefCell<Vec<relabel::Rename>>> = Rc::default();
    let refresh: Rc<dyn Fn()> = {
        let (files, find, replace, template, regex, icase, ext, plan, apply, table, empty, preview_body) = (
            files.clone(),
            find.clone(),
            replace.clone(),
            template.clone(),
            regex.clone(),
            icase.clone(),
            ext.clone(),
            plan.clone(),
            apply.clone(),
            table.clone(),
            empty.clone(),
            preview_body.clone(),
        );
        let error = w::label("", &["error"]);
        preview_body.prepend(&error);
        Rc::new(move || {
            table.clear();
            let o = relabel::Options {
                find: find.text().into(),
                replace: replace.text().into(),
                regex: regex.is_active(),
                ignore_case: icase.is_active(),
                keep_extension: !ext.is_active(),
                template: Some(template.text().to_string()).filter(|t| !t.is_empty()),
                start: 1,
            };
            let has_files = !files.borrow().is_empty();
            empty.set_visible(!has_files);
            table.root.set_visible(has_files);
            match relabel::plan(&files.borrow(), &o) {
                Ok(p) => {
                    error.set_visible(false);
                    for r in &p {
                        let mark: gtk::Widget = match &r.problem {
                            Some(e) => {
                                let i = w::icon("alert-triangle", 16);
                                i.add_css_class("warning");
                                i.set_tooltip_text(Some(e));
                                i.upcast()
                            }
                            None if r.changes() => {
                                let i = w::icon("check", 16);
                                i.add_css_class("success");
                                i.upcast()
                            }
                            None => w::icon("circle-dashed", 16).upcast(),
                        };
                        table.add(vec![
                            w::cell(&util::file_name(&r.from), &["dim"]),
                            w::cell(&util::file_name(&r.to), if r.problem.is_some() { &["warning"] } else { &[] }),
                            mark,
                        ]);
                    }
                    let ok = p.iter().any(|r| r.changes()) && p.iter().all(|r| r.problem.is_none());
                    apply.set_sensitive(ok);
                    *plan.borrow_mut() = p;
                }
                Err(e) => {
                    error.set_label(&format!("{e:#}"));
                    error.set_visible(true);
                    apply.set_sensitive(false);
                    plan.borrow_mut().clear();
                }
            }
        })
    };
    let r = refresh.clone();
    rows.append(&files_row(files.clone(), "Files", move || r()));
    for e in [&find, &replace, &template] {
        let r = refresh.clone();
        e.connect_changed(move |_| r());
        rows.append(e);
    }
    for s in [&regex, &icase, &ext] {
        let r = refresh.clone();
        s.connect_active_notify(move |_| r());
        rows.append(s);
    }
    opts_body.append(&rows);
    refresh();

    let a = app.clone();
    apply.connect_clicked(move |_| {
        let p = plan.borrow().clone();
        match relabel::apply(&p) {
            Ok(n) => {
                a.toast(&format!("Renamed {n} file(s)"));
                *files.borrow_mut() = p.into_iter().map(|r| r.to).collect();
                refresh();
            }
            Err(e) => a.toast(&format!("{e:#}")),
        }
    });
    vec![opts.upcast(), preview.upcast()]
}

// ------------------------------------------------------------------ Shrinkray

pub fn shrinkray(app: &Rc<App>) -> Cards {
    let files: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
    let go = w::button("Resize", Some("photo-scan"), "suggested-action");
    let (card, body) = w::card_with_action(
        "Resize images",
        "Images are scaled to fit inside the box, keeping their shape. 0 = no limit.",
        &go,
    );
    let rows = boxed_list();
    rows.append(&files_row(files.clone(), "Images", || {}));
    let width = adw::SpinRow::with_range(0.0, 20000.0, 10.0);
    width.set_title("Max width (px)");
    width.set_value(1920.0);
    let height = adw::SpinRow::with_range(0.0, 20000.0, 10.0);
    height.set_title("Max height (px)");
    let formats = ["Keep format", "png", "jpg", "webp", "bmp", "tiff"];
    let format = adw::ComboRow::builder().title("Convert to").model(&gtk::StringList::new(&formats)).build();
    let in_place = adw::SwitchRow::builder()
        .title("Replace originals")
        .subtitle("Otherwise saves “name (WxH).ext” next to them")
        .build();
    rows.append(&width);
    rows.append(&height);
    rows.append(&format);
    rows.append(&in_place);
    body.append(&rows);
    let progress = gtk::ProgressBar::builder().visible(false).show_text(true).build();
    body.append(&progress);

    let a = app.clone();
    go.connect_clicked(move |b| {
        let list = files.borrow().clone();
        if list.is_empty() {
            a.toast("Choose some images first");
            return;
        }
        let nz = |v: f64| Some(v as u32).filter(|v| *v > 0);
        let opts = shrinkray::Options {
            width: nz(width.value()),
            height: nz(height.value()),
            upscale: false,
            format: Some(formats[format.selected() as usize].to_string()).filter(|f| !f.starts_with("Keep")),
            out_dir: None,
            in_place: in_place.is_active(),
        };
        b.set_sensitive(false);
        progress.set_visible(true);
        progress.pulse();
        progress.set_text(Some(&format!("Resizing {} image(s)…", list.len())));
        let (b, a, progress) = (b.clone(), a.clone(), progress.clone());
        util::background(
            move || {
                list.iter()
                    .map(|f| shrinkray::process(f, &opts).map_err(|e| format!("{}: {e:#}", util::file_name(f))))
                    .collect::<Vec<_>>()
            },
            move |res| {
                b.set_sensitive(true);
                progress.set_visible(false);
                let failed: Vec<_> = res.iter().filter_map(|r| r.as_ref().err()).collect();
                match failed.first() {
                    None => a.toast(&format!("Resized {} image(s)", res.len())),
                    Some(e) => a.toast(&format!("{} failed: {e}", failed.len())),
                }
            },
        );
    });
    vec![card.upcast()]
}

// ------------------------------------------------------------------ Lockpick

pub fn lockpick(app: &Rc<App>) -> Cards {
    let target: Rc<RefCell<Option<PathBuf>>> = Rc::default();
    let (card, body) =
        w::card("What's using this?", "Choose or drop a file or folder you can't delete, unmount or eject");
    let rows = boxed_list();
    let row = adw::ActionRow::builder().title("Path").subtitle("Nothing chosen").build();
    row.add_prefix(&w::icon_tile("lock-open", false, ""));
    let file_btn = w::button("File…", Some("file"), "");
    let dir_btn = w::button("Folder…", Some("folder"), "");
    let refresh_btn = w::icon_button("refresh", "Check again");
    row.add_suffix(&file_btn);
    row.add_suffix(&dir_btn);
    row.add_suffix(&refresh_btn);
    rows.append(&row);
    body.append(&rows);

    let table = Rc::new(w::Table::new(&["Process", "PID", "How", "Path", ""], &[150, 70, 70, -1, 80]));
    let (results, results_body) = w::card("Processes", "");
    results_body.append(&table.root);
    results.set_visible(false);

    let scan: Rc<dyn Fn()> = Rc::new({
        let (target, table, results, a) = (target.clone(), table.clone(), results.clone(), app.clone());
        let me: super::SelfRef = Rc::default();
        let me2 = me.clone();
        let f: Rc<dyn Fn()> = Rc::new(move || {
            table.clear();
            let Some(p) = target.borrow().clone() else { return };
            results.set_visible(true);
            match lockpick::holders(&p) {
                Ok(h) if h.is_empty() => {
                    table.root.set_visible(false);
                    a.toast("Nothing you can see is using it");
                }
                Ok(h) => {
                    table.root.set_visible(true);
                    for holder in h {
                        let close = w::button("Close", None, "destructive-action");
                        close.add_css_class("flat");
                        let (a, me) = (a.clone(), me2.clone());
                        let (pid, name) = (holder.pid, holder.name.clone());
                        close.connect_clicked(move |_| {
                            match lockpick::close(pid, false) {
                                Ok(()) => a.toast(&format!("Asked {name} to quit")),
                                Err(e) => a.toast(&format!("Couldn't close {name}: {e}")),
                            }
                            if let Some(f) = me.borrow().clone() {
                                glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || f());
                            }
                        });
                        table.add(vec![
                            w::cell(&holder.name, &["fk-tile-title"]),
                            w::cell(&holder.pid.to_string(), &["dim", "mono"]),
                            w::cell(&format!("{:?}", holder.hold).to_lowercase(), &["dim"]),
                            w::cell(&holder.path.display().to_string(), &["dim"]),
                            close.upcast(),
                        ]);
                    }
                }
                Err(e) => a.toast(&e.to_string()),
            }
        });
        *me.borrow_mut() = Some(f.clone());
        move || f()
    });
    let set = {
        let (target, row, scan) = (target.clone(), row.clone(), scan.clone());
        Rc::new(move |v: Vec<PathBuf>| {
            if let Some(p) = v.into_iter().next() {
                row.set_subtitle(&w::escape(&p.display().to_string()));
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
    vec![card.upcast(), results.upcast()]
}

// ------------------------------------------------------------------ Teku

pub fn teku(app: &Rc<App>) -> Cards {
    let file: Rc<RefCell<Option<PathBuf>>> = Rc::default();
    let install = w::button("Install", Some("download"), "suggested-action");
    install.set_sensitive(false);
    let (card, body) = w::card_with_action(
        "Install a package",
        ".deb, .rpm, .AppImage or .flatpakref. Choose one or drop it here.",
        &install,
    );
    let rows = boxed_list();
    let row = adw::ActionRow::builder().title("Package").subtitle("Nothing chosen").build();
    row.add_prefix(&w::icon_tile("package", false, ""));
    let choose = w::button("Choose…", Some("folder-open"), "");
    row.add_suffix(&choose);
    rows.append(&row);
    body.append(&rows);
    let (log, buf) = util::log_view();
    log.set_visible(false);
    body.append(&log);

    let table = Rc::new(w::Table::new(&["Package", "Type", "Result", "When", ""], &[-1, 90, 90, 150, 40]));
    let (history, history_body) = w::card("Install history", "From Teku's log. AppImages can be removed here.");
    history_body.append(&table.root);
    let empty = w::empty_state("history", "No installs yet", "");
    history_body.append(&empty);

    let fill: Rc<dyn Fn()> = {
        let (a, table, empty) = (app.clone(), table.clone(), empty.clone());
        let me: super::SelfRef = Rc::default();
        let me2 = me.clone();
        let f: Rc<dyn Fn()> = Rc::new(move || {
            table.clear();
            let h = teku::history();
            empty.set_visible(h.is_empty());
            table.root.set_visible(!h.is_empty());
            for e in h.into_iter().take(40) {
                let action: gtk::Widget = if e.ok && e.kind == "appimage" {
                    let rm = w::icon_button("trash", "Remove this AppImage");
                    let (a, me, f) = (a.clone(), me2.clone(), e.file.clone());
                    rm.connect_clicked(move |_| {
                        let (a, me, f) = (a.clone(), me.clone(), f.clone());
                        util::background(
                            move || teku::uninstall_appimage(&f).map_err(|e| format!("{e:#}")),
                            move |r| {
                                a.toast(&match r {
                                    Ok(()) => "AppImage removed".to_string(),
                                    Err(e) => e,
                                });
                                if let Some(f) = me.borrow().clone() {
                                    f();
                                }
                            },
                        );
                    });
                    rm.upcast()
                } else {
                    gtk::Box::new(gtk::Orientation::Horizontal, 0).upcast()
                };
                let when = e.when.replace('T', " ").chars().take(16).collect::<String>();
                table.add(vec![
                    w::cell(&util::file_name(&e.file), &["fk-tile-title"]),
                    w::cell(&e.kind, &["dim"]),
                    w::pill(if e.ok { "Installed" } else { "Failed" }, if e.ok { "ok" } else { "warn" }).upcast(),
                    w::cell(&when, &["dim"]),
                    action,
                ]);
            }
        });
        *me.borrow_mut() = Some(f.clone());
        f
    };
    fill();

    let set = {
        let (file, row, install, a) = (file.clone(), row.clone(), install.clone(), app.clone());
        Rc::new(move |v: Vec<PathBuf>| {
            let Some(p) = v.into_iter().next() else { return };
            row.set_title(&w::escape(&util::file_name(&p)));
            row.set_subtitle("Checking…");
            install.set_sensitive(false);
            *file.borrow_mut() = Some(p.clone());
            let (row, install, a) = (row.clone(), install.clone(), a.clone());
            util::background(
                move || teku::detect(&[p]),
                move |r| match r.map(|mut v| v.pop()) {
                    Ok(Some(d)) => match d.route {
                        teku::Route::Install { how } => {
                            row.set_subtitle(&w::escape(&format!("{} package · installs with {how}", d.kind)));
                            install.set_sensitive(true);
                        }
                        teku::Route::Error { title, detail } => {
                            row.set_subtitle(&w::escape(&format!("{title}. {detail}")))
                        }
                    },
                    Ok(None) => row.set_subtitle("Teku didn't recognise this file"),
                    Err(e) => a.toast(&format!("{e:#}")),
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

    let a = app.clone();
    install.connect_clicked(move |b| {
        let Some(f) = file.borrow().clone() else { return };
        b.set_sensitive(false);
        log.set_visible(true);
        buf.set_text("");
        let mut cmd = util::cli();
        cmd.args(["teku", "install"]).arg(&f);
        let (buf, a, fill, b) = (buf.clone(), a.clone(), fill.clone(), b.clone());
        util::stream(cmd, move |m| match m {
            Out::Line(l) => util::append(&buf, &l),
            Out::Done(r) => {
                b.set_sensitive(true);
                a.toast(&match r {
                    Ok(()) => format!("Installed {}", util::file_name(&f)),
                    Err(_) => format!("Couldn't install {}", util::file_name(&f)),
                });
                fill();
            }
        });
    });
    vec![card.upcast(), history.upcast()]
}
