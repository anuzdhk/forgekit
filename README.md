# Forgekit

All-in-one power tools for Linux. One app with a toggle per tool, every tool also on the command line, and honest detection: tools your desktop can't support are greyed out with a one-line reason.

Status: **v0.1 (Foundation)**. See [docs/PLAN.md](docs/PLAN.md) for the full catalog and roadmap.

## What works in v0.1

| Tool | CLI | What it does |
|---|---|---|
| Spark | `forgekit spark 2+2`, `forgekit spark "10 km to mi"`, `forgekit spark firefox --run 1` | Apps, calculator, units, `install <file>`, web search |
| Eyedrop | `forgekit eyedrop [-f hex\|rgb\|hsl]`, `--history` | Pick a screen color via the Screenshot portal |
| Nightowl | `forgekit nightowl [--for 1h30m] [--while PID]` | Blocks sleep (logind) and idle/blanking (Inhibit portal) |
| Glyphgrab | `forgekit glyphgrab [-l eng]`, `--image FILE` | Select an area, OCR it with Tesseract |
| Relabel | `forgekit relabel *.jpg -f IMG_ -r trip_ [-x] [-t "{n:3}_{name}"] [--apply]` | Bulk rename, preview by default, handles swaps and collisions |
| Shrinkray | `forgekit shrinkray *.png -w 1280 -f webp` | Resize to fit and convert |
| Hostbook | `forgekit hostbook [list\|add IP NAME…\|on\|off\|remove NAME\|backups]` | Edits `/etc/hosts`, keeps untouched lines byte-for-byte, backs up before saving, writes through `pkexec` |
| Lockpick | `forgekit lockpick PATH [--close PID [--force]]` | Which processes hold a file or folder (fds, cwd, mmaps) |
| Teku | `forgekit teku detect\|install\|history\|uninstall` | Drives [Teku](https://github.com/anuzdhk/teku) |

`forgekit status` shows what this desktop supports. `forgekit enable|disable <tool>` toggles tools. Running `forgekit` with no arguments opens the app.

## Layout

```
crates/forgekit-core   capability detection, catalog, settings, tool logic (no UI)
crates/forgekit-cli    the `forgekit` binary
crates/forgekitd       user daemon on the session bus as org.forgekit.Daemon (Nightowl today)
crates/forgekit-gtk    GTK4/libadwaita app (`forgekit-gtk [tool]`)
data/                  desktop entry, systemd user unit, D-Bus activation file
```

## Building

```bash
cargo build --release
```

That builds the core, CLI and daemon. The app needs the GTK development packages:

```bash
sudo apt install libgtk-4-dev libadwaita-1-dev
```

```bash
cargo build --release -p forgekit-gtk
```

Optional runtime tools: `tesseract-ocr` (Glyphgrab), `wl-clipboard` or `xclip` (copying from the CLI), `teku`.

## Daemon API

```bash
busctl --user call org.forgekit.Daemon /org/forgekit/Daemon org.forgekit.Daemon1 NightowlStart su "Rendering" 3600
```

Methods: `Capabilities() → s` (JSON), `Tools() → s` (JSON), `NightowlStart(reason s, seconds u)` (0 = until stopped), `NightowlStop()`. Properties: `Version`, `NightowlActive`, `NightowlReason`.

## License

GPL-3.0-or-later.
