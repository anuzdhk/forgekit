//! Running work off the UI thread and talking to the `forgekit` CLI.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use gtk::{gdk, gio, glib, prelude::*};

/// Run `work` on a thread, then `done` with its result on the UI thread.
pub fn background<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static, done: impl FnOnce(T) + 'static) {
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(work());
    });
    glib::spawn_future_local(async move {
        if let Ok(v) = rx.recv().await {
            done(v);
        }
    });
}

pub enum Out {
    Line(String),
    Done(Result<(), String>),
}

/// Run a command, streaming stdout+stderr lines to `on` on the UI thread.
pub fn stream(mut cmd: Command, on: impl Fn(Out) + 'static) {
    let (tx, rx) = async_channel::unbounded();
    std::thread::spawn(move || {
        let child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send_blocking(Out::Done(Err(e.to_string())));
                return;
            }
        };
        let err = child.stderr.take().unwrap();
        let etx = tx.clone();
        let t = std::thread::spawn(move || {
            for l in BufReader::new(err).lines().map_while(Result::ok) {
                let _ = etx.send_blocking(Out::Line(l));
            }
        });
        for l in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
            let _ = tx.send_blocking(Out::Line(l));
        }
        let _ = t.join();
        let res = match child.wait() {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => Err(format!("exited with {s}")),
            Err(e) => Err(e.to_string()),
        };
        let _ = tx.send_blocking(Out::Done(res));
    });
    glib::spawn_future_local(async move {
        while let Ok(m) = rx.recv().await {
            on(m);
        }
    });
}

/// The `forgekit` CLI next to this binary, or on `$PATH`.
pub fn cli() -> Command {
    let sibling = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("forgekit")));
    Command::new(sibling.filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("forgekit")))
}

/// Run the CLI and collect stdout, failing with its stderr.
pub fn cli_output(args: &[&str]) -> Result<String, String> {
    let out = cli().args(args).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().trim_start_matches("forgekit: ").to_string())
    }
}

pub fn copy(text: &str) {
    if let Some(d) = gdk::Display::default() {
        d.clipboard().set_text(text);
    }
}

/// Open a file dialog and hand the chosen paths to `done`.
pub fn pick_files(parent: &impl IsA<gtk::Widget>, multiple: bool, folder: bool, done: impl Fn(Vec<PathBuf>) + 'static) {
    let dialog = gtk::FileDialog::new();
    let win = parent.root().and_downcast::<gtk::Window>();
    let paths = |files: gio::ListModel| -> Vec<PathBuf> {
        (0..files.n_items()).filter_map(|i| files.item(i).and_downcast::<gio::File>().and_then(|f| f.path())).collect()
    };
    match (multiple, folder) {
        (_, true) => dialog.select_folder(win.as_ref(), gio::Cancellable::NONE, move |r| {
            if let Some(p) = r.ok().and_then(|f| f.path()) {
                done(vec![p]);
            }
        }),
        (true, false) => dialog.open_multiple(win.as_ref(), gio::Cancellable::NONE, move |r| {
            if let Ok(files) = r {
                done(paths(files));
            }
        }),
        (false, false) => dialog.open(win.as_ref(), gio::Cancellable::NONE, move |r| {
            if let Some(p) = r.ok().and_then(|f| f.path()) {
                done(vec![p]);
            }
        }),
    }
}

/// Accept files dropped onto `widget`.
pub fn on_drop(widget: &impl IsA<gtk::Widget>, done: impl Fn(Vec<PathBuf>) + 'static) {
    let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    target.connect_drop(move |_, value, _, _| {
        let Ok(list) = value.get::<gdk::FileList>() else { return false };
        let paths: Vec<PathBuf> = list.files().iter().filter_map(|f| f.path()).collect();
        if paths.is_empty() {
            return false;
        }
        done(paths);
        true
    });
    widget.add_controller(target);
}

/// A read-only monospace log view.
pub fn log_view() -> (gtk::ScrolledWindow, gtk::TextBuffer) {
    let view = gtk::TextView::builder()
        .editable(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .build();
    let buf = view.buffer();
    let sw = gtk::ScrolledWindow::builder().child(&view).min_content_height(160).css_classes(["card"]).build();
    (sw, buf)
}

pub fn append(buf: &gtk::TextBuffer, line: &str) {
    let mut end = buf.end_iter();
    buf.insert(&mut end, line);
    buf.insert(&mut end, "\n");
}
