# Forgekit — All-in-One Power Tools for Linux

> One app, many small tools. Inspired by the *idea* of Microsoft PowerToys, with its own names, design and Linux-only extras.
> Works on every major distro. Each tool tells you plainly whether your desktop supports it.

*Planning document · October 2026*

---

## 1. Vision

Linux already has a good standalone tool for almost every PowerToys feature, but they are scattered, configured differently, and many break on Wayland. **Forgekit** puts them in one app with:

- **One settings window** with a toggle for every tool
- **One shortcut system** that works on GNOME, KDE, Hyprland, Sway and X11
- **Honest capability detection**: unsupported tools are greyed out with a one-line reason instead of silently failing
- **Every tool usable from the terminal** (`forgekit eyedrop`, `forgekit shelf`, …)

**Not a copy.** Forgekit does not use the "PowerToys" name, Microsoft icons, logos or code. All tool names are original.

---

## 2. Tool catalog

### Tier 1 — Universal (works on every distro and desktop)

| Tool | What it does | Inspired by |
|---|---|---|
| **Spark** | Launcher and command bar: apps, files, calculator, unit conversion, web search, plugins | Command Palette / Run |
| **Eyedrop** | Pick any color on screen; copy as HEX/RGB/HSL; color history | Color Picker |
| **Nightowl** | Keep the PC awake (indefinitely, for a time, or while a process runs) | Awake |
| **Glyphgrab** | Select an area of the screen and copy its text (OCR with Tesseract) | Text Extractor |
| **Relabel** | Bulk-rename files with find/replace, regex, numbering and live preview | PowerRename |
| **Shrinkray** | Bulk-resize and convert images from the right-click menu | Image Resizer |
| **Glance** | Quick preview of any file with a key press | Peek |
| **Hostbook** | Safe editor for `/etc/hosts` with backups and on/off toggles per entry | Hosts Editor |
| **Envy** | Edit user and system environment variables (`environment.d`, `/etc/environment`) | Environment Variables |
| **Lockpick** | Show which process is holding a file or folder, and close it | File Locksmith |
| **Caliper** | Measure pixel distances and edges on screen | Screen Ruler |
| **Dusk** | Automatic light/dark theme by time or sunset | Light Switch |
| **Clipstack** | Clipboard history for text **and images** (see §3) | *new* |
| **DropShelf** | Hold files while you move between apps (see §4) | *new* |
| **Teku** | Install `.deb`, `.rpm`, `.AppImage`, `.flatpakref` files from a right-click (see §5) | *your app* |

### Tier 2 — Needs the privileged helper (works everywhere after a one-time setup)

| Tool | What it does | Inspired by |
|---|---|---|
| **Keysmith** | Remap keys, shortcuts and key sequences, per app if possible | Keyboard Manager |
| **Umlaut** | Hold a letter to pick an accented version (é, ñ, ü…) | Quick Accent |

### Tier 3 — Per-desktop backends (GNOME + KDE first, then Hyprland/Sway)

| Tool | What it does | Inspired by |
|---|---|---|
| **Mosaic** | Custom window zones and layouts, drag to snap | FancyZones |
| **Pin** | Keep any window always on top | Always on Top |
| **Scenes** | Launch and arrange a set of apps in one click | Workspaces |
| **Cheatsheet** | Overlay that shows keyboard shortcuts | Shortcut Guide |
| **Stage** | Zoom, draw on screen and present | ZoomIt |
| **Beacon** | Find and highlight the mouse cursor, show clicks | Mouse utilities |
| **Roam** | Share one mouse and keyboard across several PCs | Mouse Without Borders |

### Linux-only extras

| Tool | What it does |
|---|---|
| **Sweep** | Clean package caches, unused Flatpak runtimes, old logs and thumbnails |
| **Bootpilot** | Manage startup apps (`~/.config/autostart`, systemd user units) |
| **Servicedeck** | Start/stop/enable systemd services and read their logs |
| **Settings Explorer** | Browse and search dconf/gsettings and KDE config files |

---

## 3. Clipstack — Clipboard manager

**Pitch:** a simple clipboard manager that keeps a history of text and images.

### Features

- History of **text and images** (PNG/JPEG), newest first, with instant search
- **Pin** items so they never expire
- Click an item to put it back on the clipboard, then paste with Ctrl+V
- **Private mode** pauses recording
- Ignores secrets: skips content marked by password managers (for example the `x-kde-passwordManagerHint` type) and lets you exclude apps
- Limits: max items, max size per item (defaults 1 MB text / 5 MB image), auto-expire after N days
- Stored locally in SQLite under `~/.local/share/forgekit/clipstack/`, never synced anywhere

### The hard part: reading the clipboard in the background

On Wayland, an app with no focused window normally can't see the clipboard. A clipboard manager needs a special protocol, and support differs by desktop:

| Desktop | How Clipstack listens |
|---|---|
| X11 (any desktop) | XFixes selection events |
| KDE Plasma 6 | `ext-data-control-v1` (or `wlr-data-control`) |
| Hyprland, Sway, labwc, other wlroots | `wlr-data-control-unstable-v1` |
| GNOME (Mutter) | No data-control protocol. **v0.2 uses XWayland's clipboard bridge** (tested both ways on GNOME Wayland, including 1.9 MB PNGs). A companion Shell extension calling `org.forgekit.Clipstack1.Add` remains the fallback for sessions without XWayland |

**Optional auto-paste:** pressing Enter in Clipstack can also type Ctrl+V for you through the Keysmith helper (uinput). Off by default.

---

## 4. DropShelf — Hold files between apps

**Pitch:** drop files onto a small floating shelf, switch apps, then drag them out where you need them.

### Features

- Drag files, folders, text snippets, links and images **onto** the shelf
- Drag one, several or all items **out** of the shelf into any app
- Shows thumbnails, file names and sizes; stack items into groups
- Actions on shelf items: open, reveal in file manager, copy path, **install with Teku** (for app packages), **resize with Shrinkray** (for images)
- Two modes: keep **references** (default, nothing is copied) or **copy into shelf** (for temporary downloads)
- Summon with a shortcut, the tray/panel icon, or `forgekit shelf`

### The hard part: reliable drag-and-drop on every display system

| Problem | Plan |
|---|---|
| Summoning the shelf. "Shake the mouse" needs global pointer tracking, which Wayland forbids | Shortcut + tray icon everywhere. Shake-to-open and screen-edge reveal only on X11 (and via layer-shell on KDE/wlroots) |
| Staying above other windows. GNOME doesn't let apps keep themselves on top | X11/KDE: "keep above" hint. wlroots/KDE: layer-shell surface. GNOME: optional bit in the companion Shell extension |
| Drag between native Wayland apps and XWayland apps is sometimes flaky | Offer several data types per drag (`text/uri-list`, `x-special/gnome-copied-files`, plain text). Include XWayland apps in the test matrix |
| Sandboxed (Flatpak) apps receive files through the Document portal, not real paths | Use GTK4's portal-aware drag-and-drop so the file is exported correctly. Native install avoids most of this |
| Files moved or deleted after being shelved | Watch shelved paths; show a "missing" badge instead of failing silently |

**Test matrix:** GNOME Wayland, KDE Wayland, Hyprland, Sway, Xfce/X11 × Nautilus, Dolphin, Nemo, Firefox, Chromium, Thunderbird, LibreOffice, one Flatpak app and one XWayland app.

---

## 5. Teku — integrated package installer

[Teku](https://github.com/anuzdhk/teku) (GPL-3.0) installs Linux apps from a right-click. It looks at the file's real type and your distro, then installs it or explains plainly why it can't.

### How Forgekit includes Teku

Teku keeps its own name, repo and release cycle. Forgekit uses it as a dependency, not a fork.

- **Teku page in Forgekit:** drop a package or pick a file. Forgekit runs `teku --detect FILE` to show what it found, then `TEKU_NO_GUI=1 teku -y FILE` and streams the output into Forgekit's own progress view
- **Install history:** reads `~/.local/state/teku/history.log` and lets you remove AppImages with `teku --uninstall-appimage`
- **DropShelf + Spark:** "Install with Teku" action on shelved packages; `install <file>` command in Spark
- Teku's existing right-click menu in Nautilus, Dolphin and Nemo keeps working as before

Teku is deliberately **not** on Flathub or the Snap Store, because their sandboxes block installing host packages and adding file-manager menu entries. Forgekit has the same problem, so it follows Teku's distribution model and reuses its release pipeline: `debian/`, RPM spec, AUR PKGBUILD, AppImage build, COPR and PPA.

---

## 6. Platform constraints (why tools are tiered)

| Area | Constraint | Our approach |
|---|---|---|
| Screenshots (Eyedrop, Glyphgrab, Caliper) | GNOME ≥ 41 and Snaps only allow screenshots through xdg-desktop-portal | Always use the Screenshot / PickColor portal |
| Global shortcuts | GlobalShortcuts portal exists on KDE Plasma 6+, GNOME 48+, Hyprland; behavior differs | Use the portal when present. Otherwise offer to bind `forgekit <tool>` in the desktop's own shortcut settings |
| Key remapping (Keysmith, Umlaut) | Wayland blocks key grabbing; evdev/uinput works everywhere but can log every keystroke | A small, separate, audited helper daemon with strict permissions; opt-in only |
| Moving other windows (Mosaic, Pin, Scenes) | No standard Wayland protocol | Backends: GNOME Shell extension, KWin script, Hyprland IPC, Sway IPC, X11 EWMH |
| Overlays (Stage, Beacon, Cheatsheet) | Need layer-shell, which GNOME lacks | layer-shell on KDE/wlroots; Shell extension on GNOME |
| Shared mouse (Roam) | Needs libei + RemoteDesktop/InputCapture portals; wlroots unsupported upstream | GNOME + KDE first; X11 fallback |
| Clipboard history (Clipstack) | Needs data-control protocol; GNOME lacks it | GNOME companion extension (§3) |

---

## 7. Architecture

```
forgekit (GUI, GTK4/libadwaita)        settings, tool pages, Spark, DropShelf, Clipstack UI
   │  D-Bus (org.forgekit.*)
forgekitd (user daemon)                shortcuts, clipboard history, Nightowl, Dusk, tray icon
   │  polkit-protected D-Bus
forgekit-helper (system, optional)     hosts file, Keysmith/Umlaut (evdev/uinput)
   │
backends/                              gnome-shell-extension/, kwin-script/, hyprland/, sway/, x11/
external: teku, tesseract, imagemagick/vips, xdg-desktop-portal
```

- **Language:** Rust core + GTK4/libadwaita UI. `ashpd` for portals, `zbus` for D-Bus.
- **Capability detection at startup:** `XDG_SESSION_TYPE`, `XDG_CURRENT_DESKTOP`, portals and their versions, Wayland protocols, whether the helper is installed. Each tool reports `supported` / `needs setup` / `unsupported (reason)`.
- **CLI for everything:** `forgekit <tool> [args]`, so any desktop can bind a shortcut to it.
- **Settings** stored in `~/.config/forgekit/` (TOML), one file per tool.
- **License:** GPL-3.0-or-later, the same as Teku.

---

## 8. Packaging & distribution

| Channel | Edition | Notes |
|---|---|---|
| `.deb` (Ubuntu PPA) | Full | Reuse Teku's `debian/` setup |
| `.rpm` (Fedora COPR, openSUSE) | Full | Reuse Teku's RPM spec |
| AUR | Full | Reuse Teku's PKGBUILD |
| AppImage | Full (helper installs on first run with password prompt) | For every other distro |
| Flatpak (later) | **Lite**: Tier 1 tools only, minus Hostbook and Teku | For immutable distros, clearly labeled |
| One-line install script | Full | Same pattern as Teku's `install.sh` |

---

## 9. Roadmap

| Version | Scope |
|---|---|
| **v0.1 — Foundation** | App shell, settings, capability detection, CLI, daemon. Tools: Spark, Eyedrop, Nightowl, Glyphgrab, Relabel, Shrinkray, Hostbook, Lockpick, **Teku integration** |
| **v0.2 — Everyday** | **Clipstack** (X11, KDE, wlroots + GNOME extension), **DropShelf**, Glance, Envy, Dusk |
| **v0.3 — Input** ✓ | Helper daemon, Keysmith, Umlaut (per-app rules and key sequences moved to later) |
| **v0.4 — Windows** ✓ | Mosaic + Pin with GNOME and KDE backends (Hyprland/Sway and X11 later) |
| **v0.5 — Presenting** ✓ | Stage, Beacon, Cheatsheet, Caliper (Stage and Caliper work on a frozen screenshot, so they run on any desktop) |
| **v0.6** ✓ | Scenes (GNOME), Roam (via Deskflow), Sweep, Bootpilot, Servicedeck, Settings Explorer. Still open: Flatpak Lite, KDE Scenes |

---

## 10. Open questions

1. Final app name: **Forgekit**, *Toolbelt* or *Hexbox*? (Check Flathub, GitHub and AUR for name clashes.)
2. Primary test desktop for v0.1 (Ubuntu GNOME, Fedora KDE, Arch Hyprland…)?
3. GTK4/libadwaita or Qt6/QML?
4. Should Teku eventually move under a shared "Forgekit" GitHub organization, or stay separate?
