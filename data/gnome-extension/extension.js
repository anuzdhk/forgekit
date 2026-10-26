// Forgekit companion extension (GNOME Shell 46+).
//
// GNOME doesn't let apps see drags, move other windows or keep them on top,
// so this extension does those few things for the Forgekit app:
//   Top bar    The Forgekit logo. Its menu lists clipboard history (from forgekitd);
//              `OpenClipboard()` drops it down from a keyboard shortcut. Picking an item
//              copies it and pastes it into the window you were in. A Bundle button in
//              the menu drops the Bundle window down below the logo.
//   Bundle     When you start dragging files, open Bundle (`forgekit-gtk --shelf --auto`) in your
//              chosen corner, and close it if nothing gets dropped on it.
//   Mosaic     Hold Shift while dragging a window to show zones; drop it into one. Drag it
//              to the top middle of the screen for a picker with every layout instead.
//              `Snap(n)` over D-Bus moves the focused window into zone n (keyboard shortcuts).
//   Pin        `PinToggle()` keeps the focused window above others, with a colored border.
//   Spark      Puts the search bar in the top middle of the screen.
//   Scenes     `ListWindows()` reports open app windows; `Place()` arranges windows of
//              those apps, including ones that open in the next 30 seconds.
// Settings come from ~/.config/forgekit/shell.json, written by the app. It talks to
// nothing but the session bus, and only reads drag/window-move events and the pointer's
// position and Shift state while you move a window (never which keys are typed).

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const APP_ID = 'io.github.anuzdhk.Forgekit';
const SHELF_TITLE = 'Bundle';
const SPARK_TITLE = 'Spark';
const CLIP_TITLE = 'Clipboard';
const DAEMON = 'org.forgekit.Daemon';
const CLIP_PATH = '/org/forgekit/Clipstack';
const CLIP_IFACE = 'org.forgekit.Clipstack1';
const CLIP_ITEMS = 30;
// Layout picker tiles, in pixels.
const TILE_W = 104;
const TILE_H = 64;
const TILE_GAP = 10;
const FLY_PAD = 14;
const FLY_LABEL = 26;
const BUS_NAME = 'org.forgekit.Shell';
const MARGIN = 16;
const BORDER = 3;
const FILE_TYPES = ['text/uri-list', 'x-special/gnome-copied-files', 'application/vnd.portal.filetransfer', 'application/vnd.portal.files'];

const DEFAULTS = {
    app: '',
    auto_show: true,
    files_only: true,
    corner: 'bottom-right',
    delay_ms: 350,
    mosaic: {zones: [[0, 0, 0.5, 1], [0.5, 0, 0.5, 1]], layouts: [], gap: 8, shift_drag: true},
    pin: {border: true, color: '#b4ff96'},
    clipboard: {panel: false, paste: true},
    bundle: {panel: false},
};

const IFACE = `<node>
  <interface name="org.forgekit.Shell1">
    <method name="PinToggle"><arg type="b" direction="out"/></method>
    <method name="Snap"><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
    <method name="OpenClipboard"/>
    <method name="ListWindows"><arg type="s" direction="out"/></method>
    <method name="Place"><arg type="s" direction="in"/><arg type="s" direction="out"/></method>
  </interface>
</node>`;

/// A zone (fractions) in pixels inside a work area. Mirrors Forgekit's Rust `zone_rect`:
/// a full gap at screen edges, half a gap on each side between zones.
export function zoneRect([zx, zy, zw, zh], area, gap) {
    const edge = f => (f <= 0.0001 || f >= 0.9999) ? gap : gap / 2;
    const left = area.x + zx * area.width + edge(zx);
    const top = area.y + zy * area.height + edge(zy);
    const right = area.x + (zx + zw) * area.width - edge(zx + zw);
    const bottom = area.y + (zy + zh) * area.height - edge(zy + zh);
    return {x: Math.round(left), y: Math.round(top), width: Math.round(right - left), height: Math.round(bottom - top)};
}

/// `#rrggbb` with an alpha, as CSS `rgba()`.
export function rgba(hex, alpha) {
    const n = parseInt(hex.replace('#', ''), 16) || 0x5b8cff;
    return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

export default class ForgekitExtension extends Extension {
    enable() {
        this._timeouts = new Set();
        this._dragging = false;
        this._shownByDrag = false;
        this._borders = new Map();
        this._overlay = null;
        this._pollId = 0;
        this._flyout = null;
        this._button = null;
        this._pending = [];
        this._cfg = this._readConfig();
        // Re-read the config whenever the app saves it.
        const file = Gio.File.new_for_path(this._configPath());
        this._monitor = file.monitor_file(Gio.FileMonitorFlags.NONE, null);
        this._monitorId = this._monitor.connect('changed', () => {
            this._cfg = this._readConfig();
            // A switch changed: rebuild the button so its menu matches.
            this._button?.destroy();
            this._button = null;
            this._syncButton();
        });
        this._syncButton();

        this._signals = [
            [Main.xdndHandler, Main.xdndHandler.connect('drag-begin', () => this._onDragBegin())],
            [Main.xdndHandler, Main.xdndHandler.connect('drag-end', () => this._onDragEnd())],
            [global.display, global.display.connect('window-created', (_d, win) => this._onWindowCreated(win))],
            [global.display, global.display.connect('grab-op-begin', (_d, win, op) => this._onGrabBegin(win, op))],
            [global.display, global.display.connect('grab-op-end', (_d, win) => this._onGrabEnd(win))],
        ];

        this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
        this._dbus.export(Gio.DBus.session, '/org/forgekit/Shell');
        this._nameId = Gio.bus_own_name_on_connection(Gio.DBus.session, BUS_NAME, Gio.BusNameOwnerFlags.NONE, null, null);
    }

    disable() {
        for (const [obj, id] of this._signals)
            obj.disconnect(id);
        this._signals = [];
        this._dbus.unexport();
        this._dbus = null;
        Gio.bus_unown_name(this._nameId);
        this._monitor.disconnect(this._monitorId);
        this._monitor.cancel();
        this._monitor = null;
        this._button?.destroy();
        this._button = null;
        this._vkbd?.run_dispose();
        this._vkbd = null;
        this._stopPolling();
        this._hideOverlay();
        this._hideFlyout();
        for (const win of [...this._borders.keys()])
            this._removeBorder(win);
        for (const id of this._timeouts)
            GLib.source_remove(id);
        this._timeouts = null;
    }

    // ---------------------------------------------------------------- helpers

    _configPath() {
        return GLib.build_filenamev([GLib.get_user_config_dir(), 'forgekit', 'shell.json']);
    }

    _readConfig() {
        try {
            const [ok, bytes] = GLib.file_get_contents(this._configPath());
            if (ok) {
                const c = JSON.parse(new TextDecoder().decode(bytes));
                return {
                    ...DEFAULTS, ...c,
                    mosaic: {...DEFAULTS.mosaic, ...c.mosaic},
                    pin: {...DEFAULTS.pin, ...c.pin},
                    clipboard: {...DEFAULTS.clipboard, ...c.clipboard},
                    bundle: {...DEFAULTS.bundle, ...c.bundle},
                };
            }
        } catch (e) {
            // No config yet: the app hasn't been opened since installing.
        }
        return DEFAULTS;
    }

    _config() {
        return this._cfg ?? DEFAULTS;
    }

    _later(ms, fn) {
        const id = GLib.timeout_add(GLib.PRIORITY_DEFAULT, ms, () => {
            this._timeouts?.delete(id);
            fn();
            return GLib.SOURCE_REMOVE;
        });
        this._timeouts.add(id);
    }

    _spawn(argv) {
        try {
            GLib.spawn_async(null, argv, null, GLib.SpawnFlags.SEARCH_PATH, null);
        } catch (e) {
            console.error(`Forgekit: couldn't run ${argv[0]}: ${e.message}`);
        }
    }

    _osd(icon, label) {
        Main.osdWindowManager.showAll(Gio.ThemedIcon.new(icon), label, null, null);
    }

    _movable(win) {
        return win && win.get_window_type() === Meta.WindowType.NORMAL && win.allows_move() && win.allows_resize();
    }

    _snapTo(win, zone, monitor) {
        const cfg = this._config();
        const area = Main.layoutManager.getWorkAreaForMonitor(monitor);
        const r = zoneRect(zone, area, cfg.mosaic.gap);
        if (win.is_maximized())
            win.unmaximize();
        win.move_resize_frame(true, r.x, r.y, r.width, r.height);
    }

    // ---------------------------------------------------------------- D-Bus

    PinToggle() {
        const win = global.display.focus_window;
        if (!win)
            return false;
        if (win.is_above()) {
            win.unmake_above();
            this._removeBorder(win);
            this._osd('view-pin-symbolic', 'Unpinned');
            return false;
        }
        win.make_above();
        if (this._config().pin.border)
            this._addBorder(win);
        this._osd('view-pin-symbolic', 'Pinned on top');
        return true;
    }

    Snap(index) {
        const win = global.display.focus_window;
        const zones = this._config().mosaic.zones;
        if (!this._movable(win) || index < 0 || index >= zones.length)
            return false;
        this._snapTo(win, zones[index], win.get_monitor());
        return true;
    }

    // ---------------------------------------------------------------- Pin border

    _addBorder(win) {
        if (this._borders.has(win))
            return;
        const actor = win.get_compositor_private();
        if (!actor)
            return;
        const color = this._config().pin.color;
        const border = new St.Widget({
            style: `border: ${BORDER}px solid ${color}; border-radius: 12px;`,
            reactive: false,
        });
        // A child of the window's actor, so it follows moves, workspaces and minimizing.
        actor.add_child(border);
        const sync = () => {
            const frame = win.get_frame_rect();
            const buffer = win.get_buffer_rect();
            border.set_position(frame.x - buffer.x - BORDER, frame.y - buffer.y - BORDER);
            border.set_size(frame.width + 2 * BORDER, frame.height + 2 * BORDER);
            border.visible = !win.is_fullscreen();
        };
        const ids = [win.connect('size-changed', sync), win.connect('unmanaged', () => this._removeBorder(win))];
        this._borders.set(win, {border, ids});
        sync();
    }

    _removeBorder(win) {
        const entry = this._borders.get(win);
        if (!entry)
            return;
        for (const id of entry.ids)
            win.disconnect(id);
        entry.border.destroy();
        this._borders.delete(win);
    }

    // ---------------------------------------------------------------- Mosaic Shift-drag

    _onGrabBegin(win, op) {
        const moving = op === Meta.GrabOp.MOVING || op === Meta.GrabOp.MOVING_UNCONSTRAINED;
        if (!moving || !this._movable(win) || !this._config().mosaic.shift_drag)
            return;
        this._grabbed = win;
        this._pollId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 40, () => {
            this._updateOverlay();
            return GLib.SOURCE_CONTINUE;
        });
    }

    _onGrabEnd(win) {
        if (win !== this._grabbed)
            return;
        this._stopPolling();
        // A zone picked in the layout picker wins over the zone overlay.
        const zone = this._flyout?.hit?.zone ?? (this._overlay?.hit != null ? this._config().mosaic.zones[this._overlay.hit] : null);
        const monitor = this._flyout?.monitor ?? this._overlay?.monitor;
        this._hideOverlay();
        this._hideFlyout();
        this._grabbed = null;
        if (zone) {
            // Let Mutter finish the move (and any edge tiling) before resizing.
            GLib.idle_add(GLib.PRIORITY_DEFAULT, () => {
                this._snapTo(win, zone, monitor);
                return GLib.SOURCE_REMOVE;
            });
        }
    }

    _stopPolling() {
        if (this._pollId) {
            GLib.source_remove(this._pollId);
            this._pollId = 0;
        }
    }

    _updateOverlay() {
        const [x, y, mods] = global.get_pointer();
        if (!(mods & Clutter.ModifierType.SHIFT_MASK)) {
            this._hideOverlay();
            this._hideFlyout();
            return;
        }
        const monitor = global.display.get_current_monitor();
        if (this._wantFlyout(x, y, monitor)) {
            this._hideOverlay();
            if (!this._flyout || this._flyout.monitor !== monitor)
                this._showFlyout(monitor);
            this._updateFlyout(x, y);
            return;
        }
        this._hideFlyout();
        if (!this._overlay || this._overlay.monitor !== monitor)
            this._showOverlay(monitor);
        const o = this._overlay;
        o.hit = null;
        o.rects.forEach((r, i) => {
            const inside = x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height;
            o.tiles[i].style = this._tileStyle(inside);
            if (inside)
                o.hit = i;
        });
    }

    _tileStyle(active) {
        const c = this._config().pin.color;
        return active
            ? `background-color: ${rgba(c, 0.4)}; border: 2px solid ${c}; border-radius: 14px;`
            : `background-color: ${rgba(c, 0.12)}; border: 2px solid ${rgba(c, 0.55)}; border-radius: 14px;`;
    }

    _showOverlay(monitor) {
        this._hideOverlay();
        const cfg = this._config();
        const area = Main.layoutManager.getWorkAreaForMonitor(monitor);
        const group = new St.Widget({reactive: false});
        const rects = cfg.mosaic.zones.map(z => zoneRect(z, area, cfg.mosaic.gap));
        const tiles = rects.map((r, i) => {
            const tile = new St.Widget({
                reactive: false,
                x: r.x, y: r.y, width: r.width, height: r.height,
                style: this._tileStyle(false),
                layout_manager: new Clutter.BinLayout(),
            });
            tile.add_child(new St.Label({
                text: `${i + 1}`,
                style: 'font-size: 42px; font-weight: 800; color: white;',
                x_align: Clutter.ActorAlign.CENTER,
                y_align: Clutter.ActorAlign.CENTER,
            }));
            group.add_child(tile);
            return tile;
        });
        Main.layoutManager.uiGroup.add_child(group);
        this._overlay = {group, rects, tiles, monitor, hit: null};
    }

    _hideOverlay() {
        this._overlay?.group.destroy();
        this._overlay = null;
    }

    // ---------------------------------------------------------------- Mosaic layout picker

    _layouts() {
        const l = this._config().mosaic.layouts;
        return l && l.length ? l : [{id: 'current', name: 'Current layout', zones: this._config().mosaic.zones}];
    }

    /// Near the top middle of the screen, or still over the open picker.
    _wantFlyout(x, y, monitor) {
        const f = this._flyout;
        if (f && f.monitor === monitor) {
            const m = 28;
            return x >= f.x - m && x < f.x + f.width + m && y < f.y + f.height + m;
        }
        const geo = global.display.get_monitor_geometry(monitor);
        const area = Main.layoutManager.getWorkAreaForMonitor(monitor);
        return y <= area.y + 6 && Math.abs(x - (geo.x + geo.width / 2)) < geo.width * 0.18;
    }

    _showFlyout(monitor) {
        this._hideFlyout();
        const color = this._config().pin.color;
        const area = Main.layoutManager.getWorkAreaForMonitor(monitor);
        const layouts = this._layouts();
        const width = layouts.length * TILE_W + (layouts.length - 1) * TILE_GAP + 2 * FLY_PAD;
        const height = TILE_H + 2 * FLY_PAD + FLY_LABEL;
        const x = area.x + Math.round((area.width - width) / 2);
        const y = area.y + 8;
        const box = new St.Widget({
            reactive: false, x, y, width, height,
            style: 'background-color: rgba(28, 28, 32, 0.94); border-radius: 18px; border: 1px solid rgba(255, 255, 255, 0.12);',
        });
        const label = new St.Label({
            text: 'Drop on a zone',
            x: 0, y: FLY_PAD + TILE_H + 6, width,
            style: 'color: rgba(255, 255, 255, 0.75); font-size: 12px; font-weight: 600; text-align: center;',
        });
        box.add_child(label);
        const cells = [];
        layouts.forEach((layout, li) => {
            const tx = FLY_PAD + li * (TILE_W + TILE_GAP);
            const tile = new St.Widget({
                x: tx, y: FLY_PAD, width: TILE_W, height: TILE_H,
                style: 'background-color: rgba(255, 255, 255, 0.06); border-radius: 10px;',
            });
            box.add_child(tile);
            for (const zone of layout.zones) {
                const [zx, zy, zw, zh] = zone;
                const inner = {x: 4, y: 4, w: TILE_W - 8, h: TILE_H - 8};
                const r = {
                    x: Math.round(inner.x + zx * inner.w + 1.5), y: Math.round(inner.y + zy * inner.h + 1.5),
                    width: Math.round(zw * inner.w - 3), height: Math.round(zh * inner.h - 3),
                };
                const cell = new St.Widget({...r, style: this._cellStyle(color, false)});
                tile.add_child(cell);
                cells.push({
                    actor: cell, layout, zone,
                    x: x + tx + r.x, y: y + FLY_PAD + r.y, width: r.width, height: r.height,
                });
            }
        });
        // A full-size preview of the zone under the pointer.
        const preview = new St.Widget({reactive: false, visible: false, style: this._tileStyle(true)});
        Main.layoutManager.uiGroup.add_child(preview);
        Main.layoutManager.uiGroup.add_child(box);
        box.opacity = 0;
        box.translation_y = -10;
        box.ease({opacity: 255, translation_y: 0, duration: 140, mode: Clutter.AnimationMode.EASE_OUT_QUAD});
        this._flyout = {box, label, cells, preview, monitor, x, y, width, height, hit: null, color};
    }

    _cellStyle(color, active) {
        return active
            ? `background-color: ${color}; border-radius: 4px;`
            : 'background-color: rgba(255, 255, 255, 0.28); border-radius: 4px;';
    }

    _updateFlyout(x, y) {
        const f = this._flyout;
        const hit = f.cells.find(c => x >= c.x && x < c.x + c.width && y >= c.y && y < c.y + c.height) ?? null;
        if (hit === f.hit)
            return;
        f.hit?.actor.set_style(this._cellStyle(f.color, false));
        f.hit = hit;
        if (!hit) {
            f.label.text = 'Drop on a zone';
            f.preview.visible = false;
            return;
        }
        hit.actor.set_style(this._cellStyle(f.color, true));
        f.label.text = hit.layout.name;
        const area = Main.layoutManager.getWorkAreaForMonitor(f.monitor);
        const r = zoneRect(hit.zone, area, this._config().mosaic.gap);
        f.preview.set_position(r.x, r.y);
        f.preview.set_size(r.width, r.height);
        f.preview.visible = true;
    }

    _hideFlyout() {
        this._flyout?.box.destroy();
        this._flyout?.preview.destroy();
        this._flyout = null;
    }

    // ---------------------------------------------------------------- Top-bar button

    /// One Forgekit logo in the top bar. Its menu is the clipboard history, with
    /// a Bundle button at the top that drops the Bundle window down below the logo.
    _syncButton() {
        const cfg = this._config();
        const want = cfg.clipboard.panel || cfg.bundle.panel;
        if (want && !this._button) {
            this._button = new PanelMenu.Button(0.5, 'Forgekit');
            const icon = Gio.FileIcon.new(Gio.File.new_for_path(GLib.build_filenamev([this.path, 'forgekit-symbolic.svg'])));
            this._button.add_child(new St.Icon({gicon: icon, style_class: 'system-status-icon'}));
            this._button.menu.connect('open-state-changed', (_m, open) => {
                if (open)
                    this._loadClips();
            });
            Main.panel.addToStatusArea('forgekit', this._button);
            this._renderMenu([]);
        } else if (!want && this._button) {
            this._button.destroy();
            this._button = null;
        }
    }

    _toggleBundle() {
        const app = this._config().app;
        if (!app)
            return;
        this._button?.menu.close();
        this._spawn([app, '--shelf-toggle']);
    }

    OpenClipboard() {
        if (this._button && this._config().clipboard.panel) {
            this._button.menu.open();
            return;
        }
        // No top-bar menu: use the app's pop-up window instead.
        const app = this._config().app;
        if (app)
            this._spawn([app, '--clipboard']);
    }

    _daemon(method, params, done) {
        Gio.DBus.session.call(DAEMON, CLIP_PATH, CLIP_IFACE, method, params, null,
            Gio.DBusCallFlags.NONE, 3000, null, (conn, res) => {
                let out = null;
                try {
                    out = conn.call_finish(res).deep_unpack();
                } catch (e) {
                    console.warn(`Forgekit: clipboard ${method}: ${e.message}`);
                }
                done?.(out);
            });
    }

    _loadClips() {
        if (!this._config().clipboard.panel) {
            this._renderMenu(null);
            return;
        }
        this._daemon('History', new GLib.Variant('(u)', [CLIP_ITEMS]), out => {
            let items = null;
            try {
                items = out ? JSON.parse(out[0]) : null;
            } catch (e) {
                items = null;
            }
            if (this._button)
                this._renderMenu(items);
        });
    }

    /// `items` null means there's no history to show (switched off, or the
    /// background service isn't answering).
    _renderMenu(items) {
        const cfg = this._config();
        const menu = this._button.menu;
        menu.removeAll();

        // Header: "Clipboard", and the Bundle button on the right.
        const header = new PopupMenu.PopupBaseMenuItem({reactive: false, can_focus: false});
        header.add_child(new St.Label({
            text: 'Clipboard', x_expand: true, y_align: Clutter.ActorAlign.CENTER, style: 'font-weight: bold;',
        }));
        if (cfg.bundle.panel) {
            const box = new St.BoxLayout({style: 'spacing: 6px;'});
            box.add_child(new St.Icon({icon_name: 'emblem-documents-symbolic', icon_size: 16}));
            box.add_child(new St.Label({text: 'Bundle', y_align: Clutter.ActorAlign.CENTER}));
            const bundle = new St.Button({
                style_class: 'button', child: box, can_focus: true, accessible_name: 'Open Bundle',
                style: 'padding: 4px 12px; border-radius: 99px; min-height: 0;',
            });
            bundle.connect('clicked', () => this._toggleBundle());
            header.add_child(bundle);
        }
        menu.addMenuItem(header);

        if (!cfg.clipboard.panel) {
            menu.addAction('Open Forgekit', () => this._openApp(''));
            return;
        }
        if (items === null) {
            menu.addMenuItem(new PopupMenu.PopupMenuItem('Clipboard history is off', {reactive: false}));
            menu.addAction('Turn it on in Forgekit', () => this._openApp('clipstack'));
            return;
        }
        if (items.length === 0) {
            const empty = new PopupMenu.PopupMenuItem('Copy text or a picture and it shows up here', {reactive: false});
            empty.label.style = 'color: rgba(255,255,255,0.6);';
            menu.addMenuItem(empty);
        } else {
            const list = new PopupMenu.PopupMenuSection();
            const scroll = new St.ScrollView({
                style: 'max-height: 440px;', overlay_scrollbars: true,
                hscrollbar_policy: St.PolicyType.NEVER,
            });
            scroll.set_child(list.actor);
            const holder = new PopupMenu.PopupMenuSection();
            holder.actor.add_child(scroll);
            menu.addMenuItem(holder);
            for (const it of items)
                list.addMenuItem(this._clipItem(it));
        }

        menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        if (items.length)
            menu.addAction('Clear history', () => this._daemon('Clear', null));
        menu.addAction('Open Forgekit', () => this._openApp(''));
    }

    _clipItem(it) {
        const item = new PopupMenu.PopupBaseMenuItem({style_class: 'forgekit-clip-item'});
        item.style = 'padding: 8px 10px; spacing: 8px;';
        let body;
        if (it.thumb) {
            const uri = GLib.filename_to_uri(it.thumb, null);
            body = new St.Widget({
                x_expand: true, width: 300, height: 90,
                style: `background-image: url("${uri}"); background-size: cover; background-position: center; border-radius: 8px;`,
            });
        } else {
            const text = it.preview.split(/\s+/).join(' ').trim();
            body = new St.Label({text: text.length > 160 ? `${text.slice(0, 160)}…` : text, x_expand: true, y_align: Clutter.ActorAlign.CENTER});
            body.style = 'max-width: 340px;';
            body.clutter_text.line_wrap = true;
            body.clutter_text.ellipsize = 3; // Pango.EllipsizeMode.END
        }
        item.add_child(body);

        const button = (icon, accessible, active, fn) => {
            const b = new St.Button({
                style_class: 'button', can_focus: true, accessible_name: accessible,
                child: new St.Icon({icon_name: icon, icon_size: 14}),
                style: `padding: 4px; border-radius: 99px; min-width: 0; min-height: 0;${active ? ` color: ${this._config().pin.color};` : ''}`,
                y_align: Clutter.ActorAlign.START,
            });
            b.connect('clicked', () => {
                fn();
                return Clutter.EVENT_STOP;
            });
            return b;
        };
        item.add_child(button('view-pin-symbolic', it.pinned ? 'Unpin' : 'Pin', it.pinned,
            () => this._daemon('SetPinned', new GLib.Variant('(xb)', [it.id, !it.pinned]), () => this._loadClips())));
        item.add_child(button('window-close-symbolic', 'Remove', false,
            () => this._daemon('Delete', new GLib.Variant('(x)', [it.id]), () => this._loadClips())));

        item.connect('activate', () => {
            this._button.menu.close();
            this._daemon('Copy', new GLib.Variant('(x)', [it.id]), out => {
                if (out && this._config().clipboard.paste)
                    this._later(220, () => this._paste());
            });
        });
        return item;
    }

    /// Open the app, on a tool's page when `page` is given.
    _openApp(page) {
        const app = this._config().app;
        if (app)
            this._spawn(page ? [app, page] : [app]);
    }

    /// Press Ctrl+V (Ctrl+Shift+V in terminals) in the focused window.
    _paste() {
        if (!this._vkbd)
            this._vkbd = Clutter.get_default_backend().get_default_seat().create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);
        const cls = global.display.focus_window?.get_wm_class() ?? '';
        const terminal = /term|kitty|alacritty|wezterm|konsole|ptyxis|foot|tilix|ghostty|console/i.test(cls);
        const keys = terminal ? [Clutter.KEY_Control_L, Clutter.KEY_Shift_L, Clutter.KEY_v] : [Clutter.KEY_Control_L, Clutter.KEY_v];
        const t = GLib.get_monotonic_time();
        for (const k of keys)
            this._vkbd.notify_keyval(t, k, Clutter.KeyState.PRESSED);
        for (const k of [...keys].reverse())
            this._vkbd.notify_keyval(t, k, Clutter.KeyState.RELEASED);
    }

    // ---------------------------------------------------------------- Bundle

    _dragHasFiles() {
        try {
            const types = global.display.get_selection().get_mimetypes(Meta.SelectionType.SELECTION_DND);
            // Unknown contents: assume it could be files rather than miss it.
            if (!types || types.length === 0)
                return true;
            return types.some(t => FILE_TYPES.some(f => t.startsWith(f)));
        } catch (e) {
            return true;
        }
    }

    _onDragBegin() {
        const cfg = this._config();
        if (!cfg.auto_show || !cfg.app)
            return;
        this._dragging = true;
        // Quick drags (reordering tabs, selecting text) shouldn't pop the shelf up.
        this._later(cfg.delay_ms, () => {
            if (!this._dragging || (cfg.files_only && !this._dragHasFiles()))
                return;
            this._shownByDrag = true;
            this._spawn([cfg.app, '--shelf', '--auto']);
        });
    }

    _onDragEnd() {
        this._dragging = false;
        if (!this._shownByDrag)
            return;
        this._shownByDrag = false;
        const cfg = this._config();
        // The app closes the shelf only if nothing was dropped on it.
        this._later(1500, () => this._spawn([cfg.app, '--shelf-dismiss']));
    }

    _onWindowCreated(win) {
        const ours = win.get_gtk_application_id?.() === APP_ID;
        this._pending = this._pending.filter(r => r.expires > Date.now());
        if (!ours && this._pending.length === 0)
            return;
        const actor = win.get_compositor_private();
        if (!actor)
            return;
        const id = actor.connect('first-frame', () => {
            actor.disconnect(id);
            const title = ours ? win.get_title() : null;
            if (title === SHELF_TITLE) {
                // Opened from the top-bar icon: drop down below it.
                // From the top bar (or a shortcut): below the logo. From a drag: the corner.
                if (this._button && !this._shownByDrag)
                    this._placeBelow(win, this._button);
                else
                    this._placeShelf(win);
                return;
            }
            if (title === SPARK_TITLE || title === CLIP_TITLE) {
                this._placeCentered(win, title === SPARK_TITLE ? 0.12 : 0.08);
                return;
            }
            const appId = Shell.WindowTracker.get_default().get_window_app(win)?.get_id() ?? '';
            const i = this._pending.findIndex(r => r.app_id === appId);
            if (i >= 0)
                this._placeWin(win, this._pending.splice(i, 1)[0]);
        });
    }

    // ---------------------------------------------------------------- Scenes

    _appWindows() {
        return global.get_window_actors()
            .map(a => a.get_meta_window())
            .filter(w => w && !w.skip_taskbar && w.get_window_type() === Meta.WindowType.NORMAL);
    }

    ListWindows() {
        const tracker = Shell.WindowTracker.get_default();
        const out = this._appWindows().map(win => {
            const app = tracker.get_window_app(win);
            const monitor = win.get_monitor();
            const area = Main.layoutManager.getWorkAreaForMonitor(monitor);
            const r = win.get_frame_rect();
            return {
                app_id: app?.get_id() ?? '',
                name: app?.get_name() ?? win.get_title() ?? '',
                monitor,
                x: (r.x - area.x) / area.width,
                y: (r.y - area.y) / area.height,
                w: r.width / area.width,
                h: r.height / area.height,
                maximized: win.is_maximized(),
            };
        });
        return JSON.stringify(out);
    }

    /// Arrange open windows of the scene's apps now; remember the rest for
    /// 30 seconds so windows of apps that are starting get placed too.
    /// Returns the app ids placed right away.
    Place(json) {
        const tracker = Shell.WindowTracker.get_default();
        const used = new Set();
        const placedNow = [];
        const pending = [];
        for (const rule of JSON.parse(json)) {
            const win = this._appWindows().find(w => !used.has(w) && tracker.get_window_app(w)?.get_id() === rule.app_id);
            if (win) {
                used.add(win);
                this._placeWin(win, rule);
                placedNow.push(rule.app_id);
            } else {
                pending.push({...rule, expires: Date.now() + 30000});
            }
        }
        this._pending = pending;
        return JSON.stringify(placedNow);
    }

    _placeWin(win, rule) {
        const monitor = rule.monitor < global.display.get_n_monitors() ? rule.monitor : Main.layoutManager.primaryIndex;
        const area = Main.layoutManager.getWorkAreaForMonitor(monitor);
        if (win.is_maximized())
            win.unmaximize();
        const clamp = (v, lo, hi) => Math.min(Math.max(v, lo), hi);
        const w = Math.round(clamp(rule.w, 0.1, 1) * area.width);
        const h = Math.round(clamp(rule.h, 0.1, 1) * area.height);
        const x = Math.round(area.x + clamp(rule.x, 0, 1) * area.width);
        const y = Math.round(area.y + clamp(rule.y, 0, 1) * area.height);
        win.move_resize_frame(true, x, y, w, h);
        if (rule.maximized)
            win.maximize();
    }

    /// Horizontally centered on the monitor with the pointer, `top` of the way down.
    _placeCentered(win, top) {
        const area = Main.layoutManager.getWorkAreaForMonitor(global.display.get_current_monitor());
        const rect = win.get_frame_rect();
        const x = area.x + Math.round((area.width - rect.width) / 2);
        const y = area.y + Math.round(area.height * top);
        win.move_frame(false, x, y);
        win.make_above();
        win.activate(global.get_current_time());
    }

    /// Just below a top-bar button, like its menu would be.
    _placeBelow(win, button) {
        const [bx] = button.get_transformed_position();
        const monitor = Main.layoutManager.primaryIndex;
        const area = Main.layoutManager.getWorkAreaForMonitor(monitor);
        const rect = win.get_frame_rect();
        const center = bx + button.width / 2;
        const x = Math.round(Math.min(Math.max(center - rect.width / 2, area.x + 8), area.x + area.width - rect.width - 8));
        win.move_frame(false, x, area.y + 6);
        win.make_above();
        win.activate(global.get_current_time());
    }

    _placeShelf(win) {
        const corner = this._config().corner;
        const monitor = global.display.get_current_monitor();
        const area = Main.layoutManager.getWorkAreaForMonitor(monitor);
        const rect = win.get_frame_rect();
        const right = corner.endsWith('right');
        const bottom = corner.startsWith('bottom');
        const x = right ? area.x + area.width - rect.width - MARGIN : area.x + MARGIN;
        const y = bottom ? area.y + area.height - rect.height - MARGIN : area.y + MARGIN;
        win.move_resize_frame(false, x, y, rect.width, rect.height);
        win.make_above();
        win.stick();
    }
}
