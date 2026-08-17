// Keep Yeet's windows above everything else and pinned to the screen edge.
//
// Yoink's whole premise is that the shelf stays reachable while you drag from
// another application. Wayland gives a client no say over stacking or position,
// and Mutter implements no layer-shell protocol, so on GNOME that guarantee can
// only come from the compositor side — which is this extension. Yeet detects
// whether it is enabled and, when it is not, restarts itself on XWayland where
// `_NET_WM_STATE_ABOVE` and dock windows achieve the same thing.

import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const APP_ID = 'io.github.hjosugi.Yeet';
const EDGE_TITLE = 'Yeet edge';
// Matches the strip width Yeet asks for; the shelf keeps a small gap so it
// reads as floating rather than glued to the bezel.
const SHELF_MARGIN = 8;

export default class YeetShelfExtension extends Extension {
    enable() {
        this._handlers = [];
        this._pending = new Map();

        this._connect(global.display, 'window-created', (_display, window) =>
            this._track(window));
        // A shelf can already exist when the extension is enabled mid-session.
        for (const actor of global.get_window_actors())
            this._track(actor.meta_window);
    }

    disable() {
        for (const [object, id] of this._handlers)
            object.disconnect(id);
        this._handlers = [];
        for (const [window, id] of this._pending)
            window.disconnect(id);
        this._pending.clear();
    }

    _connect(object, signal, callback) {
        this._handlers.push([object, object.connect(signal, callback)]);
    }

    _isYeet(window) {
        if (!window)
            return false;
        if (window.get_gtk_application_id() === APP_ID)
            return true;
        const wmClass = window.get_wm_class() ?? '';
        return wmClass.toLowerCase().startsWith('io.github.hjosugi.yeet');
    }

    // Placement needs the window's real frame, which does not exist until it
    // has been laid out. `shown` fires once that is true.
    _track(window) {
        if (!this._isYeet(window) || this._pending.has(window))
            return;
        if (window.get_frame_rect().width > 0) {
            this._apply(window);
            return;
        }
        const id = window.connect('shown', () => {
            window.disconnect(id);
            this._pending.delete(window);
            this._apply(window);
        });
        this._pending.set(window, id);
    }

    _apply(window) {
        window.make_above();
        window.stick();

        const monitor = window.get_monitor();
        // The work area excludes the top bar and dock, so the shelf lands
        // beside them rather than underneath.
        const area = window.get_work_area_for_monitor(monitor);
        const frame = window.get_frame_rect();
        const onRight = this._preferredEdge() !== 'left';

        if (window.get_title() === EDGE_TITLE) {
            const width = Math.max(frame.width, 1);
            const x = onRight ? area.x + area.width - width : area.x;
            window.move_resize_frame(false, x, area.y, width, area.height);
            return;
        }
        const x = onRight
            ? area.x + area.width - frame.width - SHELF_MARGIN
            : area.x + SHELF_MARGIN;
        const y = area.y + Math.max(0, Math.floor((area.height - frame.height) / 2));
        window.move_frame(false, x, y);
    }

    // Read the edge from Yeet's own settings so the two agree. Any problem
    // reading it falls back to the application's default rather than failing.
    _preferredEdge() {
        const path = GLib.build_filenamev([
            GLib.get_user_config_dir(), 'Yeet', 'settings.json',
        ]);
        try {
            const [ok, contents] = Gio.File.new_for_path(path).load_contents(null);
            if (!ok)
                return 'right';
            const settings = JSON.parse(new TextDecoder().decode(contents));
            return settings.edge === 'left' ? 'left' : 'right';
        } catch {
            return 'right';
        }
    }
}
