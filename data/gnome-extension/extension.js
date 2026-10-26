// Forgekit companion extension (GNOME Shell 46+).
//
// GNOME doesn't let apps see drags, move other windows or keep them on top,
// so this extension does those few things for the Forgekit app:
//   DropShelf  When you start dragging files, open the shelf (`forgekit-gtk --shelf --auto`)
//              in your chosen corner, and close it if nothing gets dropped on it.
//   Mosaic     Hold Shift while dragging a window to show zones; drop it into one.
//              `Snap(n)` over D-Bus moves the focused window into zone n (keyboard shortcuts).
//   Pin        `PinToggle()` keeps the focused window above others, with a colored border.
//   Beacon     `FindCursor()` (or pressing Ctrl twice) rings the pointer; optional click ripples.
// Settings come from ~/.config/forgekit/shell.json, written by the app. It talks to
// nothing but the session bus, and only reads drag/window-move events and the pointer's
// position and button/modifier state (never which keys are typed).

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const APP_ID = 'io.github.anuzdhk.Forgekit';
const SHELF_TITLE = 'DropShelf';
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
    mosaic: {zones: [[0, 0, 0.5, 1], [0.5, 0, 0.5, 1]], gap: 8, shift_drag: true},
    pin: {border: true, color: '#3b82f6'},
    beacon: {clicks: false, double_ctrl: true},
};

const IFACE = `<node>
  <interface name="org.forgekit.Shell1">
    <method name="PinToggle"><arg type="b" direction="out"/></method>
    <method name="Snap"><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
    <method name="FindCursor"/>
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
        this._beaconId = 0;
        this._cfg = this._readConfig();
        // Re-read the config whenever the app saves it.
        const file = Gio.File.new_for_path(this._configPath());
        this._monitor = file.monitor_file(Gio.FileMonitorFlags.NONE, null);
        this._monitorId = this._monitor.connect('changed', () => {
            this._cfg = this._readConfig();
            this._syncBeacon();
        });
        this._syncBeacon();

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
        if (this._beaconId)
            GLib.source_remove(this._beaconId);
        this._beaconId = 0;
        this._stopPolling();
        this._hideOverlay();
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
                    beacon: {...DEFAULTS.beacon, ...c.beacon},
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
        const hit = this._overlay?.hit;
        const monitor = this._overlay?.monitor;
        this._hideOverlay();
        this._grabbed = null;
        if (hit !== undefined && hit !== null) {
            const zones = this._config().mosaic.zones;
            // Let Mutter finish the move before resizing.
            GLib.idle_add(GLib.PRIORITY_DEFAULT, () => {
                this._snapTo(win, zones[hit], monitor);
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
            return;
        }
        const monitor = global.display.get_current_monitor();
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

    // ---------------------------------------------------------------- Beacon

    FindCursor() {
        const [x, y] = global.get_pointer();
        const color = this._config().pin.color;
        // Two rings shrinking onto the pointer, the second a moment later.
        for (const [size, delay] of [[320, 0], [200, 120]]) {
            const ring = new St.Widget({
                style: `border: 5px solid ${color}; border-radius: ${size / 2}px; background-color: ${rgba(color, 0.08)};`,
                width: size, height: size, x: x - size / 2, y: y - size / 2,
                reactive: false, opacity: 0,
            });
            ring.set_pivot_point(0.5, 0.5);
            Main.layoutManager.uiGroup.add_child(ring);
            ring.ease({
                opacity: 255, duration: 120, delay,
                onComplete: () => ring.ease({
                    scale_x: 0.15, scale_y: 0.15, opacity: 0,
                    duration: 650, mode: Clutter.AnimationMode.EASE_OUT_QUAD,
                    onComplete: () => ring.destroy(),
                }),
            });
        }
    }

    _ripple(x, y, color) {
        const size = 44;
        const dot = new St.Widget({
            style: `border: 3px solid ${color}; border-radius: ${size / 2}px; background-color: ${rgba(color, 0.25)};`,
            width: size, height: size, x: x - size / 2, y: y - size / 2, reactive: false,
        });
        dot.set_pivot_point(0.5, 0.5);
        Main.layoutManager.uiGroup.add_child(dot);
        dot.ease({
            scale_x: 2.2, scale_y: 2.2, opacity: 0, duration: 420,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
            onComplete: () => dot.destroy(),
        });
    }

    /// Watch the pointer's buttons and modifier state only while a Beacon feature is on.
    _syncBeacon() {
        const b = this._config().beacon;
        const want = b.clicks || b.double_ctrl;
        if (want && !this._beaconId) {
            let prev = 0;
            let lastCtrl = 0;
            this._beaconId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 25, () => {
                const [x, y, mods] = global.get_pointer();
                const cfg = this._config().beacon;
                const down = m => (mods & m) && !(prev & m);
                if (cfg.clicks) {
                    if (down(Clutter.ModifierType.BUTTON1_MASK))
                        this._ripple(x, y, this._config().pin.color);
                    if (down(Clutter.ModifierType.BUTTON3_MASK))
                        this._ripple(x, y, '#f97316');
                }
                if (cfg.double_ctrl && down(Clutter.ModifierType.CONTROL_MASK)) {
                    const now = GLib.get_monotonic_time() / 1000;
                    if (now - lastCtrl < 400) {
                        lastCtrl = 0;
                        this.FindCursor();
                    } else {
                        lastCtrl = now;
                    }
                }
                prev = mods;
                return GLib.SOURCE_CONTINUE;
            });
        } else if (!want && this._beaconId) {
            GLib.source_remove(this._beaconId);
            this._beaconId = 0;
        }
    }

    // ---------------------------------------------------------------- DropShelf

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
        if (win.get_gtk_application_id?.() !== APP_ID)
            return;
        const actor = win.get_compositor_private();
        if (!actor)
            return;
        const id = actor.connect('first-frame', () => {
            actor.disconnect(id);
            if (win.get_title() === SHELF_TITLE)
                this._placeShelf(win);
        });
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
