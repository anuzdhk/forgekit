//! `forgekit` — run any Forgekit tool from the terminal, or open the app.

use std::net::IpAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use forgekit_core::settings::General;
use forgekit_core::tools::{
    self, clipstack, dusk, envy, eyedrop, glyphgrab, hostbook, lockpick, nightowl, relabel, shrinkray, spark, teku,
};
use forgekit_core::{CATALOG, Capabilities, Status, catalog};

#[derive(Parser)]
#[command(name = "forgekit", version, about = "All-in-one power tools for Linux", disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show what this desktop supports, tool by tool
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Turn a tool on
    Enable { tool: String },
    /// Turn a tool off
    Disable { tool: String },
    /// Launcher queries: apps, calculator, units, `install <file>`, web search
    Spark {
        query: Vec<String>,
        /// Run the Nth result (1 = first)
        #[arg(long, short)]
        run: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Pick a color on screen
    Eyedrop {
        #[arg(long, short, default_value = "hex")]
        format: eyedrop::Format,
        #[arg(long)]
        no_copy: bool,
        /// Print recently picked colors instead
        #[arg(long)]
        history: bool,
        /// Report the result as a desktop notification (for keyboard shortcuts)
        #[arg(long)]
        notify: bool,
    },
    /// Keep the PC awake (until Ctrl+C, for a time, or while a process runs)
    Nightowl {
        /// e.g. 30m, 2h, 1h30m (a bare number is minutes)
        #[arg(long = "for", value_name = "DURATION")]
        duration: Option<String>,
        /// Stay awake while this process is running
        #[arg(long = "while", value_name = "PID")]
        pid: Option<u32>,
        /// Turn keep-awake on or off in the background service
        #[arg(long, conflicts_with_all = ["duration", "pid"])]
        toggle: bool,
    },
    /// Select an area of the screen and copy its text
    Glyphgrab {
        /// Tesseract language(s), e.g. eng or eng+deu
        #[arg(long, short, default_value = "eng")]
        lang: String,
        /// OCR this image instead of taking a screenshot
        #[arg(long)]
        image: Option<PathBuf>,
        #[arg(long)]
        no_copy: bool,
        /// Report the result as a desktop notification (for keyboard shortcuts)
        #[arg(long)]
        notify: bool,
    },
    /// Bulk-rename files (preview by default; --apply to rename)
    Relabel {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        #[arg(long, short, default_value = "")]
        find: String,
        #[arg(long, short, default_value = "")]
        replace: String,
        /// Treat --find as a regular expression ($1 etc. in --replace)
        #[arg(long, short = 'x')]
        regex: bool,
        #[arg(long, short)]
        ignore_case: bool,
        /// Also rename the extension
        #[arg(long)]
        include_ext: bool,
        /// Template: {name}, {ext}, {n}, {n:3}
        #[arg(long, short)]
        template: Option<String>,
        #[arg(long, default_value_t = 1)]
        start: usize,
        #[arg(long)]
        apply: bool,
    },
    /// Bulk-resize and convert images
    Shrinkray {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        #[arg(long, short)]
        width: Option<u32>,
        #[arg(long = "height", short = 'H')]
        height: Option<u32>,
        #[arg(long)]
        upscale: bool,
        /// png, jpg, webp, …
        #[arg(long, short)]
        format: Option<String>,
        #[arg(long, short)]
        out: Option<PathBuf>,
        #[arg(long)]
        in_place: bool,
    },
    /// View and edit /etc/hosts safely
    Hostbook {
        #[command(subcommand)]
        action: Option<HostCmd>,
        /// Edit another file instead of /etc/hosts
        #[arg(long, global = true, default_value = hostbook::HOSTS)]
        file: PathBuf,
    },
    /// Show which processes hold a file or folder
    Lockpick {
        path: PathBuf,
        /// Ask this process to quit
        #[arg(long)]
        close: Option<u32>,
        /// With --close: kill instead of asking
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
    },
    /// Install app packages with Teku
    Teku {
        #[command(subcommand)]
        action: TekuCmd,
    },
    /// Clipboard history
    Clipstack {
        #[command(subcommand)]
        action: Option<ClipCmd>,
    },
    /// Hold files while you move between apps (opens the shelf)
    Shelf {
        /// Files to put on the shelf
        files: Vec<PathBuf>,
    },
    /// Preview a file
    Glance { file: PathBuf },
    /// Light/dark theme: switch now or set a schedule
    Dusk {
        #[command(subcommand)]
        action: Option<DuskCmd>,
    },
    /// Environment variables for your session (or the whole system with --system)
    Envy {
        #[command(subcommand)]
        action: Option<EnvCmd>,
        /// Edit /etc/environment instead of your user file
        #[arg(long, global = true)]
        system: bool,
    },
    /// Window zones: pick a layout or move the focused window into a zone
    Mosaic {
        #[command(subcommand)]
        action: Option<MosaicCmd>,
    },
    /// Keep the focused window on top of others (toggle)
    Pin,
    /// Zoom into and draw on your screen (opens fullscreen)
    Stage,
    /// Measure things on screen (opens fullscreen)
    Caliper,
    /// Find the mouse pointer, or turn click ripples on/off
    Beacon {
        #[command(subcommand)]
        action: Option<BeaconCmd>,
    },
    /// List your desktop's keyboard shortcuts (`--open` for the window)
    Cheatsheet {
        #[arg(long)]
        open: bool,
        /// Only shortcuts matching this text
        search: Option<String>,
    },
    /// Check GitHub for a newer Forgekit release
    Update,
    /// Global keyboard shortcuts for tools (saved in your desktop's settings)
    Shortcut {
        #[command(subcommand)]
        action: Option<ShortcutCmd>,
    },
    /// The privileged input helper that Keysmith and Umlaut need
    Helper {
        #[command(subcommand)]
        action: Option<HelperCmd>,
    },
    /// Remap keys and shortcuts (needs the helper)
    Keysmith {
        #[command(subcommand)]
        action: Option<KeyCmd>,
    },
    /// Hold a letter, tap Space or arrows to pick an accent (needs the helper)
    Umlaut {
        #[command(subcommand)]
        action: Option<UmlautCmd>,
    },
    /// Tools from later releases
    #[command(external_subcommand)]
    Other(Vec<String>),
}

#[derive(Subcommand)]
enum HostCmd {
    List,
    Add { ip: IpAddr, names: Vec<String> },
    On { name: String },
    Off { name: String },
    Remove { name: String },
    Backups,
}

#[derive(Subcommand)]
enum ClipCmd {
    /// Recent items (pinned first)
    List {
        #[arg(default_value = "")]
        search: String,
        #[arg(long, short, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Put an item back on the clipboard
    Copy {
        id: i64,
    },
    /// Print an item's text (or write an image to stdout)
    Show {
        id: i64,
    },
    Pin {
        id: i64,
    },
    Unpin {
        id: i64,
    },
    Delete {
        id: i64,
    },
    /// Remove everything except pinned items
    Clear,
    /// Pause or resume recording
    Private {
        state: OnOff,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum OnOff {
    On,
    Off,
}

#[derive(Subcommand)]
enum DuskCmd {
    /// Current mode and schedule
    Status,
    Light,
    Dark,
    Toggle,
    /// Switch at fixed times, e.g. `dusk times 07:00 19:30`
    Times {
        light: String,
        dark: String,
    },
    /// Switch at sunrise and sunset for a location
    Sun {
        #[arg(allow_hyphen_values = true)]
        latitude: f64,
        #[arg(allow_hyphen_values = true)]
        longitude: f64,
    },
    /// Stop switching automatically
    Manual,
}

#[derive(Subcommand)]
enum EnvCmd {
    /// Every variable and the file it comes from
    List,
    Set {
        key: String,
        value: String,
    },
    Unset {
        key: String,
    },
}

#[derive(Subcommand)]
enum BeaconCmd {
    /// Highlight the pointer now
    Find,
    /// Ripples where you click
    Clicks { state: OnOff },
}

#[derive(Subcommand)]
enum MosaicCmd {
    /// The current layout and its zones
    Status,
    /// Available layouts
    Layouts,
    /// Use a preset layout, e.g. `layout thirds`
    Layout { id: String },
    /// Use a custom grid, e.g. `grid 3 2`
    Grid { columns: u8, rows: u8 },
    /// Pixels between zones
    Gap { pixels: u32 },
    /// Move the focused window into zone N (1 = first)
    Snap { zone: usize },
}

#[derive(Subcommand)]
enum ShortcutCmd {
    List,
    /// e.g. `set eyedrop super+shift+c`
    Set {
        tool: String,
        keys: String,
    },
    Remove {
        tool: String,
    },
}

#[derive(Subcommand)]
enum HelperCmd {
    Status,
    /// Install system-wide (asks for your password)
    Install,
    Uninstall,
    /// Start remapping again after Ctrl+Alt+Shift+Esc
    Resume,
}

#[derive(Subcommand)]
enum KeyCmd {
    List,
    /// e.g. `add capslock ctrl`, `add ctrl+h backspace`, `add insert none`
    Add {
        from: String,
        to: String,
    },
    /// Remove a rule by its number in `list`
    Remove {
        number: usize,
    },
    On,
    Off,
}

#[derive(Subcommand)]
enum UmlautCmd {
    Status,
    On,
    Off,
    /// How long to hold a letter before Space/arrows pick accents
    Delay {
        ms: u32,
    },
    /// Comma-separated: french,german,… or `all`
    Languages {
        list: String,
    },
}

#[derive(Subcommand)]
enum TekuCmd {
    /// What Teku sees, without installing
    Detect {
        files: Vec<PathBuf>,
    },
    Install {
        file: PathBuf,
    },
    History,
    /// Remove an AppImage Teku installed
    Uninstall {
        file: PathBuf,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("forgekit: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Refuse to run tools that are off or can't work here.
fn gate(id: &str, caps: &Capabilities) -> anyhow::Result<()> {
    let tool = catalog::find(id).with_context(|| format!("unknown tool '{id}'"))?;
    if !General::load()?.is_enabled(tool.id) {
        bail!("{} is turned off (forgekit enable {})", tool.name, tool.id);
    }
    match tool.status(caps) {
        Status::Supported => Ok(()),
        Status::NeedsSetup(r) => {
            eprintln!("{}: {r}", tool.name);
            Ok(())
        }
        Status::Unsupported(r) => bail!("{} isn't available here: {r}", tool.name),
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    let caps = Capabilities::detect();
    let Some(cmd) = cli.cmd else {
        return open_app(&caps);
    };
    match cmd {
        Cmd::Status { json } => status(&caps, json),
        Cmd::Enable { tool } | Cmd::Disable { tool } if catalog::find(&tool).is_none() => {
            bail!("unknown tool '{tool}'")
        }
        Cmd::Enable { tool } => toggle(&tool, true),
        Cmd::Disable { tool } => toggle(&tool, false),
        Cmd::Spark { query, run, json } => {
            gate("spark", &caps)?;
            let items = spark::query(&query.join(" "), &spark::load_apps());
            if let Some(n) = run {
                let item = items.get(n.wrapping_sub(1)).with_context(|| format!("no result #{n}"))?;
                return spark::run(&item.action);
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&items)?);
            } else {
                for (i, it) in items.iter().enumerate() {
                    println!("{:>2}. {}  — {}", i + 1, it.title, it.subtitle);
                }
            }
            Ok(())
        }
        Cmd::Eyedrop { format, no_copy, history, notify } => {
            if history {
                for c in eyedrop::history() {
                    println!("{}", c.format(format));
                }
                return Ok(());
            }
            notifying(notify, "Eyedrop", || {
                gate("eyedrop", &caps)?;
                let c = block_on(eyedrop::pick())?;
                eyedrop::remember(c)?;
                let text = c.format(format);
                println!("{text}");
                copy(&caps, &text, no_copy)?;
                Ok(format!("Copied {text}"))
            })
        }
        Cmd::Nightowl { toggle: true, .. } => {
            gate("nightowl", &caps)?;
            let on = nightowl_toggle()?;
            let msg = if on { "Keeping your PC awake" } else { "Normal sleep settings again" };
            println!("{msg}.");
            tools::notify("Nightowl", msg);
            Ok(())
        }
        Cmd::Nightowl { duration, pid, .. } => {
            gate("nightowl", &caps)?;
            nightowl_cmd(duration, pid)
        }
        Cmd::Glyphgrab { lang, image, no_copy, notify } => notifying(notify, "Glyphgrab", || {
            gate("glyphgrab", &caps)?;
            let path = match image {
                Some(p) => p,
                None => block_on(glyphgrab::capture())?,
            };
            let text = glyphgrab::ocr(&path, &lang)?;
            if text.is_empty() {
                bail!("No text found");
            }
            println!("{text}");
            copy(&caps, &text, no_copy)?;
            let preview: String = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect();
            Ok(format!("Copied: {preview}"))
        }),
        Cmd::Relabel { files, find, replace, regex, ignore_case, include_ext, template, start, apply } => {
            gate("relabel", &caps)?;
            let opts =
                relabel::Options { find, replace, regex, ignore_case, keep_extension: !include_ext, template, start };
            let plan = relabel::plan(&files, &opts)?;
            for r in &plan {
                let from = r.from.file_name().unwrap_or_default().to_string_lossy();
                let to = r.to.file_name().unwrap_or_default().to_string_lossy();
                match &r.problem {
                    Some(p) => println!("  ✗ {from}  →  {to}   ({p})"),
                    None if r.changes() => println!("    {from}  →  {to}"),
                    None => println!("    {from}  (unchanged)"),
                }
            }
            if apply {
                println!("Renamed {} file(s).", relabel::apply(&plan)?);
            } else {
                println!("Preview only. Add --apply to rename.");
            }
            Ok(())
        }
        Cmd::Shrinkray { files, width, height, upscale, format, out, in_place } => {
            gate("shrinkray", &caps)?;
            let opts = shrinkray::Options { width, height, upscale, format, out_dir: out, in_place };
            let mut failed = 0;
            for f in &files {
                match shrinkray::process(f, &opts) {
                    Ok(o) => println!("{} → {}", f.display(), o.display()),
                    Err(e) => {
                        failed += 1;
                        eprintln!("{}: {e:#}", f.display());
                    }
                }
            }
            if failed > 0 {
                bail!("{failed} of {} image(s) failed", files.len());
            }
            Ok(())
        }
        Cmd::Hostbook { action, file } => {
            gate("hostbook", &caps)?;
            hostbook_cmd(action.unwrap_or(HostCmd::List), &file)
        }
        Cmd::Lockpick { path, close, force, json } => {
            gate("lockpick", &caps)?;
            if let Some(pid) = close {
                lockpick::close(pid, force).with_context(|| format!("closing process {pid}"))?;
                println!("Sent {} to {pid}.", if force { "SIGKILL" } else { "SIGTERM" });
                return Ok(());
            }
            let found = lockpick::holders(&path).with_context(|| format!("reading {}", path.display()))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&found)?);
            } else if found.is_empty() {
                println!("Nothing (that you can see) is using {}.", path.display());
            } else {
                for h in &found {
                    println!(
                        "{:>7}  {:<16} {:<6} {}",
                        h.pid,
                        h.name,
                        format!("{:?}", h.hold).to_lowercase(),
                        h.path.display()
                    );
                }
                println!("Close one with: forgekit lockpick {} --close PID", path.display());
            }
            Ok(())
        }
        Cmd::Teku { action } => {
            gate("teku", &caps)?;
            teku_cmd(action)
        }
        Cmd::Clipstack { action } => {
            gate("clipstack", &caps)?;
            clipstack_cmd(action.unwrap_or(ClipCmd::List { search: String::new(), limit: 20, json: false }))
        }
        Cmd::Shelf { files } => {
            gate("shelf", &caps)?;
            let mut args = vec!["--shelf".into()];
            args.extend(files.iter().map(|f| f.canonicalize().unwrap_or_else(|_| f.clone()).into_os_string()));
            exec_app(args)
        }
        Cmd::Glance { file } => {
            gate("glance", &caps)?;
            anyhow::ensure!(file.exists(), "{} doesn't exist", file.display());
            exec_app(vec!["--glance".into(), file.canonicalize()?.into_os_string()])
        }
        Cmd::Dusk { action } => {
            gate("dusk", &caps)?;
            dusk_cmd(action.unwrap_or(DuskCmd::Status), &caps)
        }
        Cmd::Envy { action, system } => {
            gate("envy", &caps)?;
            envy_cmd(action.unwrap_or(EnvCmd::List), if system { envy::Scope::System } else { envy::Scope::User })
        }
        Cmd::Mosaic { action } => {
            gate("mosaic", &caps)?;
            mosaic_cmd(action.unwrap_or(MosaicCmd::Status), &caps)
        }
        Cmd::Pin => {
            gate("pin", &caps)?;
            let b = forgekit_core::desktop::backend(&caps).context("no window backend for this desktop")?;
            match forgekit_core::desktop::pin_toggle(b)? {
                Some(true) => println!("Pinned on top."),
                Some(false) => println!("Unpinned."),
                // KWin scripts can't report back; say what happened anyway.
                None => tools::notify("Pin", "Toggled always-on-top for the focused window"),
            }
            Ok(())
        }
        Cmd::Stage => {
            gate("stage", &caps)?;
            exec_app(vec!["--stage".into()])
        }
        Cmd::Caliper => {
            gate("caliper", &caps)?;
            exec_app(vec!["--caliper".into()])
        }
        Cmd::Beacon { action } => {
            use forgekit_core::tools::beacon;
            gate("beacon", &caps)?;
            let b = forgekit_core::desktop::backend(&caps).context("no backend for this desktop")?;
            match action.unwrap_or(BeaconCmd::Find) {
                BeaconCmd::Find => beacon::find(b),
                BeaconCmd::Clicks { state } => {
                    let mut s = beacon::Settings::load()?;
                    s.clicks = matches!(state, OnOff::On);
                    s.save()?;
                    beacon::apply(b, &s)?;
                    println!("Click ripples {}.", if s.clicks { "on" } else { "off" });
                    Ok(())
                }
            }
        }
        Cmd::Cheatsheet { open, search } => {
            gate("cheatsheet", &caps)?;
            if open {
                return exec_app(vec!["--cheatsheet".into()]);
            }
            let q = search.unwrap_or_default().to_lowercase();
            let mut group = String::new();
            for s in forgekit_core::tools::cheatsheet::collect(caps.desktop) {
                if !q.is_empty() && !s.action.to_lowercase().contains(&q) && !s.keys.to_lowercase().contains(&q) {
                    continue;
                }
                if s.group != group {
                    println!("\n{}", s.group);
                    group = s.group.clone();
                }
                println!("  {:<28} {}", s.keys, s.action);
            }
            Ok(())
        }
        Cmd::Update => {
            use forgekit_core::update::{self, Outcome};
            match update::check_now(env!("CARGO_PKG_VERSION"))? {
                Outcome::UpToDate => println!("Forgekit {} is the latest version.", env!("CARGO_PKG_VERSION")),
                Outcome::NoReleases => println!(
                    "No releases published yet; you're on a development build ({}).",
                    env!("CARGO_PKG_VERSION")
                ),
                Outcome::Available(r) => {
                    println!("Forgekit {} is available (you have {}): {}", r.version, env!("CARGO_PKG_VERSION"), r.url)
                }
            }
            Ok(())
        }
        Cmd::Shortcut { action } => shortcut_cmd(action.unwrap_or(ShortcutCmd::List), &caps),
        Cmd::Helper { action } => helper_cmd(action.unwrap_or(HelperCmd::Status)),
        Cmd::Keysmith { action } => {
            gate("keysmith", &caps)?;
            keysmith_cmd(action.unwrap_or(KeyCmd::List))
        }
        Cmd::Umlaut { action } => {
            gate("umlaut", &caps)?;
            umlaut_cmd(action.unwrap_or(UmlautCmd::Status))
        }
        Cmd::Other(args) => {
            let id = &args[0];
            match catalog::find(id) {
                Some(t) => {
                    bail!("{} isn't available yet: {}", t.name, t.status(&caps).reason().unwrap_or("not in this build"))
                }
                None => bail!("unknown command '{id}' (try forgekit --help)"),
            }
        }
    }
}

fn block_on<T>(f: impl std::future::Future<Output = anyhow::Result<T>>) -> anyhow::Result<T> {
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(f)
}

fn copy(caps: &Capabilities, text: &str, skip: bool) -> anyhow::Result<()> {
    if !skip && !tools::copy_to_clipboard(caps, text)? {
        bail!("not copied: start Forgekit's background service, or install wl-clipboard or xclip");
    }
    Ok(())
}

/// Run an action; with `notify`, report the outcome (or error) as a desktop notification.
fn notifying(notify: bool, title: &str, f: impl FnOnce() -> anyhow::Result<String>) -> anyhow::Result<()> {
    match f() {
        Ok(msg) => {
            if notify {
                tools::notify(title, &msg);
            }
            Ok(())
        }
        Err(e) => {
            if notify {
                tools::notify(title, &format!("{e:#}"));
            }
            Err(e)
        }
    }
}

/// Flip the daemon's keep-awake state. Returns whether it's now on.
fn nightowl_toggle() -> anyhow::Result<bool> {
    let conn = zbus::blocking::Connection::session()?;
    let proxy =
        zbus::blocking::Proxy::new(&conn, "org.forgekit.Daemon", "/org/forgekit/Daemon", "org.forgekit.Daemon1")?;
    let on: bool =
        proxy.get_property("NightowlActive").context("forgekitd isn't running (start it from Forgekit's Settings)")?;
    if on {
        proxy.call_method("NightowlStop", &())?;
    } else {
        proxy.call_method("NightowlStart", &("Forgekit Nightowl", 0u32))?;
    }
    Ok(!on)
}

fn app_path() -> Option<PathBuf> {
    let sibling = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("forgekit-gtk")));
    sibling.filter(|p| p.exists()).or_else(|| forgekit_core::paths::which("forgekit-gtk"))
}

fn open_app(caps: &Capabilities) -> anyhow::Result<()> {
    if app_path().is_some() {
        return exec_app(Vec::new());
    }
    eprintln!("The Forgekit app isn't installed; showing tool status instead.\n");
    status(caps, false)
}

/// Replace this process with the GTK app.
fn exec_app(args: Vec<std::ffi::OsString>) -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;
    let app = app_path().context("this needs the Forgekit app (forgekit-gtk), which isn't installed")?;
    Err(std::process::Command::new(app).args(args).exec().into())
}

fn clipstack_cmd(action: ClipCmd) -> anyhow::Result<()> {
    use clipstack::store::Store;
    let store = Store::open_default()?;
    match action {
        ClipCmd::List { search, limit, json } => {
            let items = store.list(&search, limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&items)?);
                return Ok(());
            }
            if items.is_empty() {
                println!("Clipboard history is empty.");
            }
            for i in items {
                let line: String =
                    i.preview.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect();
                println!("{:>5} {} {line}", i.id, if i.pinned { "📌" } else { "  " });
            }
            Ok(())
        }
        ClipCmd::Copy { id } => {
            let conn = zbus::blocking::Connection::session()?;
            conn.call_method(
                Some("org.forgekit.Daemon"),
                "/org/forgekit/Clipstack",
                Some("org.forgekit.Clipstack1"),
                "Copy",
                &id,
            )
            .context("asking forgekitd to set the clipboard (is it running?)")?;
            println!("Copied item {id}.");
            Ok(())
        }
        ClipCmd::Show { id } => {
            use std::io::Write;
            let clip = store.get(id)?.with_context(|| format!("no item {id}"))?;
            std::io::stdout().write_all(&clip.data)?;
            Ok(())
        }
        ClipCmd::Pin { id } => {
            anyhow::ensure!(store.set_pinned(id, true)?, "no item {id}");
            Ok(())
        }
        ClipCmd::Unpin { id } => {
            anyhow::ensure!(store.set_pinned(id, false)?, "no item {id}");
            Ok(())
        }
        ClipCmd::Delete { id } => {
            anyhow::ensure!(store.delete(id)?, "no item {id}");
            Ok(())
        }
        ClipCmd::Clear => {
            println!("Removed {} item(s); pinned items kept.", store.clear()?);
            Ok(())
        }
        ClipCmd::Private { state } => {
            let mut s = clipstack::Settings::load()?;
            s.private = matches!(state, OnOff::On);
            s.save()?;
            println!("Private mode {}.", if s.private { "on: nothing is recorded" } else { "off" });
            Ok(())
        }
    }
}

fn dusk_cmd(action: DuskCmd, caps: &Capabilities) -> anyhow::Result<()> {
    let mut settings = dusk::Settings::load()?;
    let set_mode = |m: dusk::Mode| -> anyhow::Result<()> {
        dusk::apply(m, caps.desktop, &settings)?;
        println!("Switched to {m:?}.");
        Ok(())
    };
    match action {
        DuskCmd::Status => {
            let now = chrono::Local::now();
            println!("Now: {:?}", dusk::current(caps.desktop)?);
            match &settings.schedule {
                dusk::Schedule::Manual => println!("Schedule: manual"),
                dusk::Schedule::Times { light, dark } => println!("Schedule: light at {light}, dark at {dark}"),
                dusk::Schedule::Sun { latitude, longitude } => {
                    println!("Schedule: sunrise/sunset at {latitude:.3}, {longitude:.3}");
                    if let Some((rise, set)) = dusk::sun_times(now, *latitude, *longitude) {
                        println!("Today: sunrise {}, sunset {}", rise.format("%H:%M"), set.format("%H:%M"));
                    }
                }
            }
            if let Some(next) = dusk::next_change(&settings.schedule, now) {
                println!("Next switch: {}", next.format("%a %H:%M"));
            }
            Ok(())
        }
        DuskCmd::Light => set_mode(dusk::Mode::Light),
        DuskCmd::Dark => set_mode(dusk::Mode::Dark),
        DuskCmd::Toggle => set_mode(dusk::current(caps.desktop)?.flip()),
        DuskCmd::Times { light, dark } => {
            dusk::parse_time(&light)?;
            dusk::parse_time(&dark)?;
            settings.schedule = dusk::Schedule::Times { light, dark };
            settings.save()?;
            println!("Saved. forgekitd switches automatically while it runs.");
            Ok(())
        }
        DuskCmd::Sun { latitude, longitude } => {
            anyhow::ensure!(
                (-90.0..=90.0).contains(&latitude) && (-180.0..=180.0).contains(&longitude),
                "latitude is -90..90 and longitude -180..180"
            );
            settings.schedule = dusk::Schedule::Sun { latitude, longitude };
            settings.save()?;
            println!("Saved. forgekitd switches automatically while it runs.");
            Ok(())
        }
        DuskCmd::Manual => {
            settings.schedule = dusk::Schedule::Manual;
            settings.save()?;
            println!("Automatic switching is off.");
            Ok(())
        }
    }
}

fn envy_cmd(action: EnvCmd, scope: envy::Scope) -> anyhow::Result<()> {
    let path = scope.path();
    let mut file = envy::EnvFile::load(&path)?;
    match action {
        EnvCmd::List => {
            for src in envy::sources() {
                if src.vars.is_empty() {
                    continue;
                }
                let tag = match src.editable {
                    Some(envy::Scope::User) => " (yours)",
                    Some(envy::Scope::System) => " (system)",
                    None => "",
                };
                println!("{}{tag}", src.path.display());
                for v in src.vars {
                    println!("  {}={}", v.key, v.value);
                }
            }
            return Ok(());
        }
        EnvCmd::Set { key, value } => file.set(&key, &value)?,
        EnvCmd::Unset { key } => anyhow::ensure!(file.remove(&key), "{key} isn't set in {}", path.display()),
    }
    let backup = file.save(scope)?;
    println!("Saved {}. It applies the next time you log in.", path.display());
    if let Some(b) = backup {
        println!("Backup: {}", b.display());
    }
    Ok(())
}

fn status(caps: &Capabilities, json: bool) -> anyhow::Result<()> {
    let general = General::load()?;
    if json {
        let tools: Vec<_> = CATALOG
            .iter()
            .map(|t| serde_json::json!({ "tool": t, "enabled": general.is_enabled(t.id), "status": t.status(caps) }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "system": caps, "tools": tools }))?);
        return Ok(());
    }
    println!(
        "Session: {:?} · Desktop: {} · Helper: {}",
        caps.session,
        if caps.desktop_raw.is_empty() { "unknown" } else { &caps.desktop_raw },
        if caps.helper_installed { "installed" } else { "not installed" }
    );
    let mut tier = None;
    for t in CATALOG {
        if tier != Some(t.tier) {
            tier = Some(t.tier);
            println!("\n{}", t.tier.title());
        }
        let s = t.status(caps);
        let mark = match (&s, general.is_enabled(t.id)) {
            (_, false) => "off",
            (Status::Supported, _) => " ✓ ",
            (Status::NeedsSetup(_), _) => " ! ",
            (Status::Unsupported(_), _) => " – ",
        };
        let reason = s.reason().map(|r| format!("  ({r})")).unwrap_or_default();
        println!("  [{mark}] {:<18} {}{reason}", t.name, t.summary);
    }
    Ok(())
}

fn toggle(id: &str, on: bool) -> anyhow::Result<()> {
    let tool = catalog::find(id).unwrap();
    let mut g = General::load()?;
    g.set_enabled(tool.id, on);
    g.save()?;
    println!("{} is now {}.", tool.name, if on { "on" } else { "off" });
    Ok(())
}

fn nightowl_cmd(duration: Option<String>, pid: Option<u32>) -> anyhow::Result<()> {
    let until = duration.as_deref().map(nightowl::parse_duration).transpose()?.map(|d| Instant::now() + d);
    if let Some(pid) = pid {
        anyhow::ensure!(nightowl::process_alive(pid), "no process {pid}");
    }
    let reason = match (pid, &duration) {
        (Some(p), _) => format!("Waiting for process {p}"),
        (None, Some(d)) => format!("Keeping awake for {d}"),
        (None, None) => "Keeping awake".to_string(),
    };
    let inhibitor = nightowl::Inhibitor::acquire(&reason)?;
    println!("Nightowl: {} — {reason}. Ctrl+C to stop.", inhibitor.describe());
    loop {
        if until.is_some_and(|t| Instant::now() >= t) {
            println!("Time's up.");
            break;
        }
        if pid.is_some_and(|p| !nightowl::process_alive(p)) {
            println!("Process exited.");
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}

fn hostbook_cmd(action: HostCmd, file: &std::path::Path) -> anyhow::Result<()> {
    let mut hosts = hostbook::Hosts::load(file)?;
    let changed = match action {
        HostCmd::List => {
            for e in hosts.entries() {
                let c = e.comment.as_deref().map(|c| format!("  # {c}")).unwrap_or_default();
                println!("[{}] {:<24} {}{c}", if e.enabled { "on " } else { "off" }, e.ip, e.names.join(" "));
            }
            return Ok(());
        }
        HostCmd::Backups => {
            for b in hostbook::backups() {
                println!("{}", b.display());
            }
            return Ok(());
        }
        HostCmd::Add { ip, names } => {
            hosts.add(ip, names)?;
            1
        }
        HostCmd::On { name } => hosts.set_enabled(&name, true),
        HostCmd::Off { name } => hosts.set_enabled(&name, false),
        HostCmd::Remove { name } => hosts.remove(&name),
    };
    if changed == 0 {
        println!("Nothing to change.");
        return Ok(());
    }
    let backup = hosts.save(file)?;
    match backup {
        Some(b) => println!("Saved {} (backup: {}).", file.display(), b.display()),
        None => println!("Saved {}.", file.display()),
    }
    Ok(())
}

fn teku_cmd(action: TekuCmd) -> anyhow::Result<()> {
    match action {
        TekuCmd::Detect { files } => {
            for d in teku::detect(&files)? {
                match d.route {
                    teku::Route::Install { how } => println!("{}: {} — installs via {how}", d.file.display(), d.kind),
                    teku::Route::Error { title, detail } => println!("{}: {title}. {detail}", d.file.display()),
                }
            }
            Ok(())
        }
        TekuCmd::Install { file } => teku::install(&file, |l| println!("{l}")),
        TekuCmd::History => {
            for h in teku::history() {
                println!("{}  {:<6} {:<9} {}", h.when, if h.ok { "ok" } else { "failed" }, h.kind, h.file.display());
            }
            Ok(())
        }
        TekuCmd::Uninstall { file } => teku::uninstall_appimage(&file),
    }
}

fn helper_cmd(action: HelperCmd) -> anyhow::Result<()> {
    use forgekit_core::helper;
    match action {
        HelperCmd::Status => match helper::status() {
            Some(s) => {
                println!("forgekit-helper {} is running.", s.version);
                if s.paused {
                    println!("Paused by Ctrl+Alt+Shift+Esc. Resume with: forgekit helper resume");
                } else if s.active {
                    println!("Remapping: {}", s.keyboards.join(", "));
                } else {
                    println!("Idle: no rules or Umlaut switched on, so no keyboard is grabbed.");
                }
                if let Some(e) = s.error {
                    println!("Problem: {e}");
                }
            }
            None => println!("The helper isn't running. Install it with: forgekit helper install"),
        },
        HelperCmd::Install => {
            helper::install(false)?;
            println!("Installed.");
            let saved = helper::load();
            if forgekit_keys::engine::Engine::is_active(&saved) {
                helper::push(&saved)?;
                println!("Applied your saved Keysmith/Umlaut settings.");
            }
        }
        HelperCmd::Uninstall => {
            helper::install(true)?;
            println!("Removed.");
        }
        HelperCmd::Resume => {
            helper::resume()?;
            println!("Remapping resumed.");
        }
    }
    Ok(())
}

/// Save settings and apply them if the helper is running.
fn apply_input(cfg: &forgekit_core::helper::HelperConfig) -> anyhow::Result<()> {
    use forgekit_core::helper;
    if helper::status().is_some() {
        helper::push(cfg)?;
        println!("Applied.");
    } else {
        helper::save(cfg)?;
        println!("Saved. It applies once the helper is installed (forgekit helper install).");
    }
    Ok(())
}

fn keysmith_cmd(action: KeyCmd) -> anyhow::Result<()> {
    use forgekit_core::helper;
    use forgekit_keys::rules::Rule;
    let mut cfg = helper::load();
    match action {
        KeyCmd::List => {
            println!("Keysmith is {}.", if cfg.keysmith_enabled { "on" } else { "off" });
            if cfg.rules.is_empty() {
                println!("No rules yet. Example: forgekit keysmith add capslock ctrl");
            }
            for (i, r) in cfg.rules.iter().enumerate() {
                println!("{:>3}. {r}", i + 1);
            }
            return Ok(());
        }
        KeyCmd::Add { from, to } => {
            let r = Rule::parse(&from, &to).map_err(anyhow::Error::msg)?;
            cfg.rules.retain(|x| x.from != r.from);
            println!("{r}");
            cfg.rules.push(r);
            cfg.keysmith_enabled = true;
        }
        KeyCmd::Remove { number } => {
            anyhow::ensure!((1..=cfg.rules.len()).contains(&number), "no rule {number}");
            println!("Removed {}", cfg.rules.remove(number - 1));
        }
        KeyCmd::On => cfg.keysmith_enabled = true,
        KeyCmd::Off => cfg.keysmith_enabled = false,
    }
    apply_input(&cfg)
}

fn umlaut_cmd(action: UmlautCmd) -> anyhow::Result<()> {
    use forgekit_core::helper;
    use forgekit_keys::umlaut::LANGUAGES;
    let mut cfg = helper::load();
    match action {
        UmlautCmd::Status => {
            let u = &cfg.umlaut;
            println!(
                "Umlaut is {}. Hold a letter {} ms, then tap Space or →/← to cycle accents.",
                if u.enabled { "on" } else { "off" },
                u.delay_ms
            );
            let langs = if u.languages.is_empty() { "all".to_string() } else { u.languages.join(", ") };
            println!("Languages: {langs}");
            for (letter, v) in u.variants() {
                println!("  {letter}: {}", v.iter().collect::<String>());
            }
            return Ok(());
        }
        UmlautCmd::On => cfg.umlaut.enabled = true,
        UmlautCmd::Off => cfg.umlaut.enabled = false,
        UmlautCmd::Delay { ms } => cfg.umlaut.delay_ms = ms,
        UmlautCmd::Languages { list } => {
            let ids: Vec<String> = list.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect();
            if ids.iter().any(|i| i == "all") {
                cfg.umlaut.languages.clear();
            } else {
                for i in &ids {
                    anyhow::ensure!(
                        LANGUAGES.iter().any(|(id, _, _)| id == i),
                        "unknown language '{i}' (choose from: {})",
                        LANGUAGES.iter().map(|(id, _, _)| *id).collect::<Vec<_>>().join(", ")
                    );
                }
                cfg.umlaut.languages = ids;
            }
        }
    }
    apply_input(&cfg)
}

fn shortcut_cmd(action: ShortcutCmd, caps: &Capabilities) -> anyhow::Result<()> {
    use forgekit_core::shortcuts;
    let backend = shortcuts::backend(caps.desktop).context(
        "this desktop isn't supported yet; bind the command from `forgekit status` in its keyboard settings",
    )?;
    match action {
        ShortcutCmd::List => {
            let mut any = false;
            for t in CATALOG.iter().filter(|t| !t.is_planned()) {
                if let Some(a) = shortcuts::get(backend, t.id) {
                    println!("{:<12} {:<22} {}", t.name, a, shortcuts::action(t).label);
                    any = true;
                }
            }
            if !any {
                println!("No Forgekit shortcuts yet. Example: forgekit shortcut set eyedrop super+shift+c");
            }
        }
        ShortcutCmd::Set { tool, keys } => {
            let t = catalog::find(&tool).with_context(|| format!("unknown tool '{tool}'"))?;
            let accel = shortcuts::parse_accel(&keys)?;
            let clashes = shortcuts::conflicts(backend, t.id, &accel);
            if !clashes.is_empty() {
                eprintln!("Note: also used by {}", clashes.join(", "));
            }
            let a = shortcuts::action(t);
            shortcuts::set(backend, t.id, &a.label, &a.command, &accel)?;
            println!("{accel} → {}", a.label);
        }
        ShortcutCmd::Remove { tool } => {
            let t = catalog::find(&tool).with_context(|| format!("unknown tool '{tool}'"))?;
            shortcuts::remove(backend, t.id)?;
            println!("Removed.");
        }
    }
    Ok(())
}

fn mosaic_cmd(action: MosaicCmd, caps: &Capabilities) -> anyhow::Result<()> {
    use forgekit_core::desktop;
    use forgekit_core::tools::mosaic::{PRESETS, Settings};
    let mut s = Settings::load()?;
    match action {
        MosaicCmd::Status => {
            let name = PRESETS.iter().find(|(id, _, _)| *id == s.layout).map(|(_, n, _)| n.to_string());
            let name = name.unwrap_or_else(|| format!("{} × {} grid", s.columns, s.rows));
            println!("Layout: {name}, {} px gaps{}", s.gap, if s.shift_drag { ", Shift-drag on" } else { "" });
            for (i, z) in s.zones().iter().enumerate() {
                println!(
                    "  zone {}: {:.0}% {:.0}% · {:.0}% × {:.0}%",
                    i + 1,
                    z.x * 100.0,
                    z.y * 100.0,
                    z.w * 100.0,
                    z.h * 100.0
                );
            }
            return Ok(());
        }
        MosaicCmd::Layouts => {
            for (id, name, zones) in PRESETS {
                println!("{id:<10} {name} ({} zones)", zones.len());
            }
            println!("{:<10} Custom columns × rows (forgekit mosaic grid 3 2)", "grid");
            return Ok(());
        }
        MosaicCmd::Snap { zone } => {
            anyhow::ensure!(zone >= 1, "zones start at 1");
            let b = desktop::backend(caps).context("no window backend for this desktop")?;
            return desktop::snap(b, zone - 1);
        }
        MosaicCmd::Layout { id } => {
            anyhow::ensure!(
                id == "grid" || PRESETS.iter().any(|(p, _, _)| *p == id),
                "unknown layout '{id}' (see forgekit mosaic layouts)"
            );
            s.layout = id;
        }
        MosaicCmd::Grid { columns, rows } => {
            anyhow::ensure!((1..=6).contains(&columns) && (1..=4).contains(&rows), "1–6 columns and 1–4 rows");
            s.layout = "grid".into();
            s.columns = columns;
            s.rows = rows;
        }
        MosaicCmd::Gap { pixels } => {
            anyhow::ensure!(pixels <= 64, "gaps up to 64 px");
            s.gap = pixels;
        }
    }
    s.save()?;
    desktop::write_config(None);
    println!("Saved. {} zones.", s.zones().len());
    Ok(())
}
