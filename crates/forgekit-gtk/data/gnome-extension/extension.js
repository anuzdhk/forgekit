// Forgekit companion extension (GNOME Shell 46+).
//
// GNOME doesn't let apps see drags in progress or place their own windows,
// so this does two small things for the Forgekit app:
//   1. When you start dragging files, it opens DropShelf (`forgekit-gtk --shelf --auto`),
//      and closes it again if the drag ends without anything dropped on it.
//   2. It puts the DropShelf window in your chosen screen corner, above other
//      windows and on every workspace.
// Settings come from ~/.config/forgekit/shell.json, written by the app.
// It sends nothing anywhere and reads nothing but drag start/end events.

import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const APP_ID = 'io.github.anuzdhk.Forgekit';
const SHELF_TITLE = 'DropShelf';
const MARGIN = 16;
const FILE_TYPES = ['text/uri-list', 'x-special/gnome-copied-files', 'application/vnd.portal.filetransfer', 'application/vnd.portal.files'];

const DEFAULTS = {app: '', auto_show: true, files_only: true, corner: 'bottom-right', delay_ms: 350};

export default class ForgekitExtension extends Extension {
    enable() {
        this._timeouts = new Set();
        this._dragging = false;
        this._shownByDrag = false;
        this._dragBeginId = Main.xdndHandler.connect('drag-begin', () => this._onDragBegin());
        this._dragEndId = Main.xdndHandler.connect('drag-end', () => this._onDragEnd());
        this._windowId = global.display.connect('window-created', (_display, win) => this._onWindowCreated(win));
    }

    disable() {
        Main.xdndHandler.disconnect(this._dragBeginId);
        Main.xdndHandler.disconnect(this._dragEndId);
        global.display.disconnect(this._windowId);
        for (const id of this._timeouts)
            GLib.source_remove(id);
        this._timeouts = null;
    }

    _config() {
        const path = GLib.build_filenamev([GLib.get_user_config_dir(), 'forgekit', 'shell.json']);
        try {
            const [ok, bytes] = GLib.file_get_contents(path);
            if (ok)
                return {...DEFAULTS, ...JSON.parse(new TextDecoder().decode(bytes))};
        } catch (e) {
            // No config yet: the app hasn't been opened since installing.
        }
        return DEFAULTS;
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
                this._place(win);
        });
    }

    _place(win) {
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
