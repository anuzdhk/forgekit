# Forgekit: Rebuild Report

A complete account of Forgekit as it stands (v0.7, commit `9d380f2`, 27 Oct 2026) for
whoever rebuilds it from scratch. It covers:

- what the product is meant to be, and the decisions the owner has already made;
- every tool, and the tools deliberately dropped;
- the architecture and every interface between the pieces;
- what went wrong and why;
- a recommended plan for doing it properly.

Read it with `docs/PLAN.md` (the original vision). Where this report and the plan
disagree, this report is newer and wins.

---

## 1. What Forgekit is

An all-in-one power-tools suite for Linux, in the spirit of Microsoft PowerToys and macOS
utilities: a launcher, clipboard history, a file shelf, quick look, window layouts, a color
picker, OCR, bulk rename, cleanup and so on. It has one polished app, a CLI for everything,
and honest per-desktop detection: a tool that can't work on your desktop says so instead of
failing silently.

Targets, in order: **GNOME on Wayland** (the owner's desktop, GNOME 50.1 / Mutter 18 /
GTK 4.22 / Nautilus 50.2 on Ubuntu), then KDE Plasma, then wlroots compositors
(Hyprland/Sway), then X11.

### 1.1 Decisions the owner has made (treat as requirements)

These came out of review sessions and should carry over unchanged unless the owner says otherwise.

**Look and feel**
- The design is professional and minimal, modelled on macOS and Windows 11 utilities.
- The app does **not** follow the system theme. It has its own themes:
  - Forge Dark is the default, with accent **green `#b4ff96`** (text on accent `#1a1a1a`).
  - Others: Forge Light (accent `#3a9a2a`), Catppuccin, Dracula, Material, and a
    translucent "glass" theme.
  - The user picks the theme and accent in the app.
- Font: **Mona Sans** (github/mona-sans, SIL OFL). It ships with the app, so nothing is
  installed system-wide. The variable font's family name is `Mona Sans VF`.
- Icons: **Tabler Icons** (MIT).
- Logo: `logo/logo.svg`, a dark circle with two green wedges. A monochrome two-wedge
  version is used as the top-bar icon.

**One Forgekit icon in the top bar.** Not one icon per tool. Clicking it, or pressing
**Super+V**, drops down a menu:
- clipboard history, with a **Bundle** button in the menu header;
- picking a clip copies it and pastes it into the window you were in;
- each clip can be pinned or removed;
- at the bottom: "Clear history" and "Open Forgekit".

**Tools**
- **Spark** is a Spotlight-style search bar, **Super+Space** by default. It floats about
  12% down from the top of the screen, not in the middle. Enter runs the result and Esc
  or clicking elsewhere closes it. Its shadow must fit its window (the earlier version
  was clipped).
- **Bundle** (the file shelf) opens from the top-bar menu, **directly below the logo**. When
  you start dragging files it also pops up in a screen corner and closes again if
  nothing is dropped.
- **Glance** is quick look on **Space in GNOME Files**, for **pictures and documents
  only**. Other file types are ignored. Arrow keys step through the folder; Space or Esc
  closes it.
- **Mosaic** (window zones):
  - hold **Shift** while dragging a window to show its zones;
  - drag to the **top middle** of the screen to drop down a picker with every layout,
    like Windows 11 Snap Layouts.
- **Teku** and **Roam** have **no keyboard shortcut**. Roam is only on/off.

**Names.** User-facing names are final, and the old ids still exist in code only
because of history:

| Display name | Old id / name |
|---|---|
| Clipboard | `clipstack` / Clipstack |
| Bundle | `shelf` / DropShelf |
| Startup Apps | `bootpilot` / Bootpilot |
| Services | `servicedeck` / Servicedeck |

**Removed on purpose** (do not rebuild): Dusk (light/dark scheduler), Stage (screen
zoom and draw), Caliper (on-screen ruler), Beacon (find pointer, click ripples).

**Home screen**, top to bottom:
1. Greeting with the desktop name and how many tools are ready.
2. **Clean PC** card: shows how much can be freed. Its button cleans what is ticked on the
   Sweep page (recommended items by default) after a confirmation.
3. "Shortcuts to remember" card: Spark key, Clipboard key, Space in Files, Shift-drag.
4. Six quick actions: Search, Clipboard, Pick a color, Grab text, Bundle, Keep awake.
5. "Needs attention", listing tools that need a setup step.

There is no grid of every tool on Home; the sidebar already lists them.

**Shortcuts and first run**
- Shortcuts are set from inside the app, never by sending the user to GNOME Settings, and
  conflicts are shown.
- First run sets Super+Space (Spark) and Super+V (Clipboard). To free those keys it removes
  `<Super>space` from GNOME's `switch-input-source` and `<Super>v` from
  `toggle-message-tray`; Super+M still opens notifications. It also starts the
  background service at login.
- There is an update checker against GitHub releases (the repo has no releases yet).

---

## 2. Tool catalog (as built)

Tier: **U** = works on any desktop; **H** = needs the privileged helper; **D** = needs a
per-desktop backend (the GNOME extension or KWin scripts); **L** = Linux system extra.
"Verified" means the owner saw it work on their desktop.

| Tool (id) | Tier | What it does | External deps | Verified live |
|---|---|---|---|---|
| Spark (`spark`) | U | Search bar: apps (.desktop files), calculator, unit conversion, `install <file>` via Teku, web search (DuckDuckGo) | – | Bar shows; placement fix pending re-login |
| Clipboard (`clipstack`) | U | Text and image history in SQLite; pin, search, private mode; top-bar menu; paste on pick | XWayland on GNOME (X11 XFixes), or `ext-data-control`/`wlr-data-control` | Capture: yes. Menu: pending re-login |
| Bundle (`shelf`) | U | Small window holding files, text and links; drag one or all out; pops up while dragging | GNOME extension for the drag pop-up | v0.4 drag pop-up: yes |
| Eyedrop (`eyedrop`) | U | Pick a screen color; HEX/RGB/HSL; history | Screenshot portal | Not confirmed |
| Glyphgrab (`glyphgrab`) | U | OCR an area of the screen | `tesseract-ocr`, screenshot portal | Not confirmed |
| Glance (`glance`) | U | Quick look on Space in Files (pictures, PDF, text) | `poppler-utils` (`pdftoppm`) for PDF | Bus-name fix pending |
| Relabel (`relabel`) | U | Bulk rename: find/replace, regex, numbering, preview | – | Not confirmed |
| Shrinkray (`shrinkray`) | U | Bulk resize and convert images | `image` crate | Not confirmed |
| Lockpick (`lockpick`) | U | Which process holds a file; close it | `/proc` | Not confirmed |
| Teku (`teku`) | U | Install .deb/.rpm/.AppImage/.flatpakref | `teku` binary, dpkg/rpm/flatpak | Not confirmed |
| Nightowl (`nightowl`) | U | Keep awake for a while or while a PID runs | logind/portal inhibit via forgekitd | Not confirmed |
| Hostbook (`hostbook`) | U | Safe `/etc/hosts` editor with backups and toggles | pkexec | Not confirmed |
| Envy (`envy`) | U | User (`~/.config/environment.d`) and system (`/etc/environment`) variables | pkexec for system | Not confirmed |
| Keysmith (`keysmith`) | H | Remap keys and shortcuts, or disable keys (evdev to uinput) | forgekit-helper | **Yes** (CapsLock→Esc) |
| Umlaut (`umlaut`) | H | Hold a letter to pick an accented version | forgekit-helper, IBus (non-GNOME) | Not confirmed |
| Cheatsheet (`cheatsheet`) | U | Every keyboard shortcut the desktop has, searchable | gsettings | Not confirmed |
| Mosaic (`mosaic`) | D | Zones: Shift-drag, top-middle layout picker, keyboard snap | GNOME extension / KWin script | Shift-drag: **yes** (v0.4). Picker: pending |
| Pin (`pin`) | D | Always on top, with a colored border | GNOME extension / KWin script | **Yes** |
| Scenes (`scenes`) | D | Save and restore window arrangements | GNOME extension | Not confirmed |
| Roam (`roam`) | D | Share mouse and keyboard across PCs | Deskflow (one-click install) | Not confirmed |
| Sweep / Clean PC (`sweep`) | L | Package cache, unused Flatpaks, old Snaps, journal, thumbnails, app caches, trash | apt/dnf/pacman/zypper, flatpak, snap, journalctl, pkexec | Scan: yes. Clean: not confirmed |
| Startup Apps (`bootpilot`) | L | XDG autostart entries and systemd user services | systemctl | Not confirmed |
| Services (`servicedeck`) | L | systemd services: status, start/stop/restart, enable/disable, logs | systemctl, journalctl, pkexec | Not confirmed |
| Settings Explorer (`settings-explorer`) | L | Search, edit and reset every GSettings key; shows changed ones | gsettings, dconf | Not confirmed |

**Takeaway:** only 5 of 24 tools have been confirmed on a real desktop. The rest compile
and pass unit tests, but their behavior on a live desktop is unknown.

---

## 3. Architecture as built

A Rust workspace (edition 2024) of about 19,000 lines with six crates:

| Crate | Binary | Role | Notable size |
|---|---|---|---|
| `forgekit-keys` | lib | Pure keymap engine: key names, Keysmith rules, Umlaut tables, remap engine. No I/O. | engine.rs 443 |
| `forgekit-core` | lib | Capability detection, tool catalog, settings, every tool's logic, desktop/extension management, shortcuts, update checker | catalog 467, shortcuts 474, spark 537 |
| `forgekit-cli` | `forgekit` | One subcommand per tool (+ `status`, `enable/disable`, `shortcut`, `helper`, `update`) | main.rs **1,439 in one file** |
| `forgekitd` | `forgekitd` | Session daemon `org.forgekit.Daemon`: Nightowl inhibitor and the clipboard watcher | 364 |
| `forgekit-helper` | `forgekit-helper` | Root system-bus service `org.forgekit.Helper`: evdev to uinput for Keysmith/Umlaut, polkit-checked | 559 |
| `forgekit-gtk` | `forgekit-gtk` | GTK4 + libadwaita app, also hosts the Spark bar, Clipboard pop-up, Bundle, Glance and Files previewer | everyday 812, maintenance 858 |

Outside Rust:
- `data/gnome-extension/extension.js` (874 lines): the GNOME Shell companion.
- `tools/import-tabler-icons.py`: converts stroked Tabler SVGs to GTK's `gpa:stroke` format.

### 3.1 Process model

```
forgekit-gtk ──(single-instance GApplication, id io.github.anuzdhk.Forgekit)
   ├─ main window (sidebar + pages)
   ├─ --spark       Spotlight bar (hidden, not destroyed, for 10 min to reopen fast)
   ├─ --clipboard   Win+V-style pop-up (fallback when the top-bar menu is unavailable)
   ├─ --shelf / --shelf-toggle / --shelf-dismiss   Bundle window
   ├─ --glance FILE Quick look window (one window, reused)
   ├─ --previewer   serves org.gnome.NautilusPreviewer for GNOME Files (D-Bus activated)
   └─ --cheatsheet  shortcut window
forgekit (CLI) ── execs forgekit-gtk for windowed tools; talks to daemon/helper/extension over D-Bus
forgekitd ── session bus; clipboard watcher thread + SQLite store; Nightowl inhibitor
forgekit-helper ── system bus, root, systemd unit; evdev grab + uinput
GNOME extension ── session bus org.forgekit.Shell; top-bar menu, window placement, Mosaic, Pin, Scenes, Bundle drag pop-up
```

### 3.2 D-Bus interfaces

**`org.forgekit.Daemon`**, session bus, path `/org/forgekit/Daemon`, interface `org.forgekit.Daemon1`:
- `Version() → s`
- `Capabilities() → s` (JSON)
- `Tools() → s` (JSON)
- `NightowlStart(...)`, `NightowlStop()`; properties `NightowlActive b`, `NightowlReason s`

Same name, path `/org/forgekit/Clipstack`, interface `org.forgekit.Clipstack1`:
- `Copy(id x)`
- `History(limit u) → s`: JSON array of `{id, mime, preview, size, pinned, created, last_used, thumb?}`
- `SetPinned(id x, pinned b)`, `Delete(id x)`, `Clear() → u`
- `SetText(s)`, `SetClip(mime s, data ay)`, `Add(mime s, data ay)`, `Reload()`
- Properties `Backend s`, `Private b` (read/write). Signal `Changed`.

**`org.forgekit.Shell`**, session bus, owned by the GNOME extension, path `/org/forgekit/Shell`,
interface `org.forgekit.Shell1`:
- `PinToggle() → b`
- `Snap(i) → b`
- `OpenClipboard()`
- `ListWindows() → s` (JSON)
- `Place(s) → s` (JSON)

**`org.forgekit.Helper`**, system bus, root, interface `org.forgekit.Helper1`:
- `Status() → s`
- `Config() → s`
- `SetConfig(json)`: polkit action `org.forgekit.helper.configure`
- `Resume(...)`

**`org.gnome.NautilusPreviewer`**, served by forgekit-gtk for GNOME Files:
- Path `/org/gnome/NautilusPreviewer`, interface **`org.gnome.NautilusPreviewer2`**. Only the
  interface has the "2"; the bus name doesn't.
- `ShowFile(s uri, s window_handle, b close_if_shown)`, `Close()`
- Signal `SelectionEvent(u direction)` (GtkDirectionType)
- Property `Visible b`; emit `PropertiesChanged` when it changes.
- Activation file: `~/.local/share/dbus-1/services/org.gnome.NautilusPreviewer.service`.
- Stay out of the way if GNOME Sushi is installed (`/usr/share/dbus-1/services/org.gnome.NautilusPreviewer.service`).

### 3.3 Files on disk

| What | Where |
|---|---|
| Per-tool settings (TOML, atomic write) | `~/.config/forgekit/{general,appearance,clipstack,shelf,mosaic,pin,sweep}.toml` |
| Tools on/off | `~/.config/forgekit/general.toml` |
| Extension config (mirror of settings, JSON) | `~/.config/forgekit/shell.json`, watched by the extension with a file monitor |
| First-run markers | `~/.config/forgekit/first-run` |
| Clipboard history | `~/.local/share/forgekit/clipstack/history.db` (SQLite, WAL, folder 0700) |
| Clip thumbnails for the top-bar menu | `~/.cache/forgekit/clip-thumbs/` |
| Bundle items, Eyedrop history, update check | `~/.local/share/forgekit/{shelf,eyedrop,updates}/` |
| Embedded font copy | `~/.cache/forgekit/fonts/MonaSans.ttf` |
| GNOME extension | `~/.local/share/gnome-shell/extensions/forgekit@anuzdhk.github.io/{metadata.json,extension.js,forgekit-symbolic.svg}` |
| Daemon autostart | `~/.config/autostart/forgekitd.desktop` |
| Shortcuts | GNOME: `org.gnome.settings-daemon.plugins.media-keys` custom keybindings named `forgekit-<tool>`. Cinnamon: `org.cinnamon.desktop.keybindings` |
| Helper (root) | `/usr/libexec/forgekit-helper`, `/etc/systemd/system/forgekit-helper.service`, `/usr/share/dbus-1/system.d/org.forgekit.Helper.conf`, `/usr/share/polkit-1/actions/org.forgekit.helper.policy`, `/etc/modules-load.d/forgekit-uinput.conf` |

---

## 4. Hard-won technical facts

Each of these cost a debugging round, so they belong in the new design from day one.

### Wayland and GNOME
1. **Apps can't position their own windows on Wayland.** Anything that must appear at a
   specific place (Spark near the top, Bundle under the top-bar icon, Bundle in a corner)
   needs one of:
   - the compositor: the GNOME Shell extension, or a KWin script;
   - the `wlr-layer-shell` protocol on wlroots and KDE (`gtk4-layer-shell`).

   GNOME has no layer-shell.
2. **GTK 4 on Wayland reports its app id only as the window class.**
   `win.get_gtk_application_id()` returns nothing, so match with
   `win.get_wm_class() === APP_ID` and check again when the window first shows. Missing
   this caused "Spark and Bundle open in the middle".
3. **GNOME on Wayland loads extension code only at login.** Every extension change needs a
   full log-out and log-in. Locking the screen or turning the extension off and on reuses the
   old code. This made every iteration slow, and the owner tested old code twice without
   knowing it. **Develop the extension in a nested shell instead**
   (`dbus-run-session gnome-shell --devkit` on GNOME 49+; the owner's GNOME 50 has the flag, but the `mutter-devkit` viewer it opens isn't installed yet, so install Mutter's dev tools package first), and have the app show
   "extension is outdated, log out to update" by comparing installed and bundled versions.
4. After a fresh install, GNOME may put the extension on `disabled-extensions`. Install must
   add it to `enabled-extensions` and remove it from `disabled-extensions`.
5. GNOME 50 Shell APIs in use:
   - `St.ScrollView.set_child` (not `add_actor`);
   - `win.is_maximized()`;
   - `PanelMenu.Button(alignment, name, dontCreateMenu)`;
   - `Main.panel.addToStatusArea`;
   - `Main.xdndHandler` `drag-begin`/`drag-end` for detecting drags;
   - a Clutter virtual keyboard (`create_virtual_device`) to send Ctrl+V for paste-on-pick
     (Ctrl+Shift+V in terminals).
6. **Clipboard on GNOME:** Mutter offers no data-control protocol to clients. History works
   through XWayland's X11 clipboard (XFixes selection events) bridged by Mutter. On
   wlroots/KDE use `ext-data-control-v1` or `wlr-data-control`. When writing through data
   control, skip the next event so the app doesn't record its own selection.
7. **Screenshots:** the portal's non-interactive capture works without a prompt (about
   0.7 s) and returns the whole multi-monitor union. The owner's two monitors: HDMI-1 at
   0,0 and eDP-1 at 1920,221, each 1920×1080, union 3840×1301.
8. **Undecorated GTK windows with a CSS shadow:** the shadow must fit inside the
   transparent margin (`margin ≥ offset + blur + spread`) or it gets cut off in a hard
   line. Current values: margin 26px, `box-shadow: 0 10px 24px -8px`.

### GTK, Rust and system
9. **The user's GTK theme leaks in.** WhiteSur installs into `~/.config/gtk-4.0/gtk.css`
   at USER priority. Load Forgekit's CSS at `STYLE_PROVIDER_PRIORITY_USER + 1` and above.
10. GTK 4.22 floods logs with "GtkImage reported baselines…". It's filtered with
    `glib::log_set_writer_func`, because GTK logs through the structured writer, not the
    default handler.
11. **Stroked Tabler icons render filled** in GTK unless converted to GTK's symbolic stroke
    format (`gpa:stroke="foreground"`): see `tools/import-tabler-icons.py`.
12. An image that expands inside a box makes its parent expand too, which stretched tile
    layouts. Put icon tiles in a CSS-padded `Image` and don't let them expand.
13. **Single-instance GApplication pitfalls:**
    - a second launch (including debug builds with the same app id) is forwarded to the
      running instance, which confused testing;
    - windows that are hidden but not destroyed keep the app alive;
    - `--gapplication-app-id` doesn't apply without the matching flag.
14. **Embedding a font:** use `FcConfigAppFontAddFile` (fontconfig FFI) before GTK lays out
    text. It works on older Pango; `pango_font_map_add_font_file` needs Pango 1.56 or newer.
15. **gio-rs `bus_own_name*` panics** (non-unwinding abort) when the bus disconnects, for
    example at logout. Call `org.freedesktop.DBus.RequestName` directly instead.
16. **pkexec/sudo can't run from sandboxed terminals** (`no_new_privs`, "pkexec must be
    setuid root"). Detect this and explain it plainly; offer a desktop launcher or the
    normal terminal.
17. **Shortcuts via gsettings** spawn a `gsettings` process per call. Run them on a worker
    thread, never in GTK startup or on the UI thread.
18. Keymap engine: the panic chord must count a remapped Ctrl, so check physical **or**
    virtual modifier state. Clipboard store ordering needs strictly increasing millisecond
    timestamps, because same-second ties broke ordering.
19. **Nautilus previewer:** Files 50 finds the previewer with `ListActivatableNames` and
    calls `org.gnome.NautilusPreviewer`. Registering under the interface name
    (`…Previewer2`) means Files never calls you, and nothing logs an error.

### Development environment (owner's machine)
- No GTK or libadwaita dev packages were installed. Builds used pkg-config **shims**: fake
  `.pc` files plus `.so` symlinks in a scratch folder. **For the rebuild, install
  `libgtk-4-dev libadwaita-1-dev` properly**; the shims hid version mismatches and can't
  build anything that needs headers.
- Visual checks used `gtk4-broadwayd` in a browser. Clicks and typing don't pass through,
  and transparency and real compositor behavior don't render. That's useful for layout,
  useless for interaction and placement. Real checks need a nested GNOME Shell or a VM.
- Test runs must never touch the owner's real desktop. Use `dbus-run-session`,
  `GSETTINGS_BACKEND=memory`, scratch `XDG_*_HOME` and no `DISPLAY`/`WAYLAND_DISPLAY`.

---

## 5. What went wrong (why rebuild)

1. **Too wide, too shallow.** Twenty-four tools across six releases in a few days; 5
   confirmed live. Each tool got a page, a CLI command and a backend before any of them
   were proven on the target desktop.
2. **No real feedback loop for desktop integration.** Most of the product's value is
   placement, top bar, global keys, drag pop-ups and paste, and that all lives in the GNOME
   extension. That can only be tested after a log-out, so bugs reached the owner one
   re-login at a time: wrong app-id check, wrong previewer bus name, clipped shadows, old
   extension still running.
3. **The UI process does too much.** The settings app also hosts the Spark bar, the
   clipboard pop-up, Bundle, Glance and the Files previewer. They rely on a "hide for 10
   minutes" trick to feel fast and get tangled with the main window's single-instance
   behavior.
4. **Imperative UI in very large files.** Pages are built by hand with `Rc<RefCell<…>>`
   closures (`everyday.rs` 812 lines, `maintenance.rs` 858). There are no view models, so
   UI logic can't be unit-tested.
5. **The CLI is one 1,439-line `main.rs`.**
6. **Ids drifted from names** after the renames (`clipstack`/Clipboard, `shelf`/Bundle,
   `bootpilot`/Startup Apps, `servicedeck`/Services).
7. **State is spread out:** seven TOML files plus a JSON mirror for the extension, plus
   gsettings, autostart files and D-Bus service files, all kept in sync by hand.
8. **Built without real dev packages**, so there was no way to build tests against real GTK.

---

## 6. Recommended rebuild plan

### 6.1 Principles
- **Depth before breadth.** Ship a small set of tools that are excellent on GNOME Wayland,
  confirmed live, before adding more.
- **Every desktop-facing feature gets a live test path from day one**: a nested GNOME
  Shell, plus a short manual checklist per release.
- **Ids equal names**: `clipboard`, `bundle`, `startup-apps`, `services`, and so on, with a
  one-time settings migration from the old ids.

### 6.2 Proposed architecture

```
forgekit-core        pure logic + models (no GTK, no D-Bus), heavily unit-tested
forgekit-ipc         one versioned D-Bus contract (zbus proxies + interface traits) shared by all
forgekitd            the always-running session service: clipboard, Bundle store,
                     Nightowl, settings owner, emits change signals
forgekit-overlay     a small resident GTK process for the instant surfaces:
                     Spark bar, clipboard pop-up, Bundle, Glance / Files previewer.
                     Uses gtk4-layer-shell where available, otherwise the GNOME extension places it.
forgekit-app         the settings app (main window) only: pages are views over core models
forgekit (CLI)       thin; clap subcommands split per module
forgekit-helper      root input helper (keep the current design; it works)
gnome-extension      thin: top-bar logo + menu, window placement, Mosaic/Pin/Scenes, drag
                     detection. It gets settings and data over D-Bus, not a JSON file.
                     Report its version over D-Bus so the app can say "log out to update".
```

Why:
- Instant surfaces live in a process that's always warm, so there's no startup lag and no
  single-instance tangle with the settings window.
- One owner for settings, the daemon, replaces seven TOML files plus a JSON mirror: one
  `config.toml` with a schema version, changes broadcast over D-Bus.
- Consider **Relm4** or a strict model/view split, with **Blueprint** `.blp` templates for
  layout, to replace the hand-built `Rc<RefCell>` pages.

### 6.3 Suggested phases

| Phase | Scope | Done when |
|---|---|---|
| 0. Foundations | Real dev packages; workspace; core/ipc/daemon skeleton; config + migration; themes, Mona Sans, Tabler icons, logo; nested-shell dev script; CI (fmt, clippy, tests) | App window opens with its theme; extension loads in a nested shell |
| 1. Top bar + Clipboard | Extension logo button + menu; daemon history; paste on pick; Super+V; private mode | Confirmed on the owner's desktop |
| 2. Spark | Overlay bar near the top; apps, calculator, units, web; Super+Space with conflict handling | Confirmed live, opens within 100 ms |
| 3. Bundle | Opens below the logo from the menu; drag pop-up in a corner; drag out | Confirmed live |
| 4. Glance | Files previewer (pictures, PDF, text), arrow keys, Space/Esc | Confirmed live in Files |
| 5. Mosaic + Pin | Shift-drag zones, top-middle layout picker, keyboard snap, pin border | Confirmed live |
| 6. Home + Clean PC | Home as specified in §1.1; Sweep page; cleaning with one password prompt | Clean confirmed live |
| 7. Everything else | Eyedrop, Glyphgrab, Relabel, Shrinkray, Lockpick, Teku, Nightowl, Hostbook, Envy, Keysmith, Umlaut, Cheatsheet, Scenes, Roam, Startup Apps, Services, Settings Explorer | One at a time, each confirmed live |
| 8. Other desktops + packaging | KDE (KWin scripts + layer-shell), wlroots, X11; Flatpak "Lite", deb/rpm/AUR/AppImage | Tested in VMs |

### 6.4 Release checklist (per phase)
- `cargo fmt --check`, `clippy -D warnings`, tests.
- Extension: test in the nested shell first, then on the owner's desktop after a real
  log-out, and check the app shows the extension version as current.
- Walk through the phase's features on the owner's desktop with a short written checklist.
- Screenshots in `docs/screenshots/`.

---

## 7. Reusable as-is

These parts are solid and can be lifted into the rebuild with light changes:

- `forgekit-keys` (keymap engine, rules, Umlaut tables) and `forgekit-helper` (install,
  polkit, evdev/uinput): Keysmith is confirmed working.
- Clipboard backends `tools/clipstack/{x11,wayland}.rs` and `store.rs` (SQLite schema, hash
  dedup, pinning, pruning).
- `spark.rs` query engine (calculator, unit conversion, desktop-file parsing and scoring).
- `sweep.rs` scan/clean (one pkexec for all root steps), `bootpilot.rs`, `servicedeck.rs`,
  `settings_explorer.rs`, `hostbook.rs`, `envy.rs`, `relabel.rs`.
- `capability.rs` detection (session, desktop, portals, Wayland globals, binaries, helper,
  IBus, extension).
- `shortcuts.rs` gsettings custom-keybinding management, conflict detection and default
  setup.
- `theme.rs` palettes and CSS generation, and `tools/import-tabler-icons.py`.
- From `extension.js`: `zoneRect` (mirrors Rust `zone_rect`), the Mosaic overlay and layout
  picker, Pin border, Scenes `ListWindows`/`Place`, the clipboard menu and the paste code.

---

## 8. Open questions for the owner

1. Which tools are must-haves for the first rebuilt release? Phases 1–6 are the suggestion.
2. Should the top-bar menu do more than Clipboard and Bundle, such as Spark, Nightowl or
   Clean PC?
3. Should the logo in the top bar be monochrome (as now, matching GNOME) or in the green
   brand color?
4. How far should support beyond GNOME go for 1.0: KDE only, or wlroots and X11 too?
5. Packaging priority: `.deb` for Ubuntu first, or Flatpak? The full edition needs host
   access, which Flatpak restricts.
