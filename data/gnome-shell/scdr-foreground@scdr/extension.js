// Reports the focused window's process to the SCDR session agent
// (org.scdr.Session1 on the session bus), which relays it to scdrd.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

export default class ScdrForeground extends Extension {
    enable() {
        this._focusId = global.display.connect('notify::focus-window', () => this._report());
        this._report();
    }

    disable() {
        if (this._focusId) {
            global.display.disconnect(this._focusId);
            this._focusId = null;
        }
    }

    _report() {
        const window = global.display.focus_window;
        if (!window)
            return;
        // Only real application windows; not menus, tooltips, docks, the
        // desktop, or the shell's own windows.
        const type = window.get_window_type();
        if (type !== Meta.WindowType.NORMAL && type !== Meta.WindowType.DIALOG &&
            type !== Meta.WindowType.MODAL_DIALOG)
            return;
        const klass = (window.get_wm_class() ?? '').toLowerCase();
        const appId = window.get_gtk_application_id() ?? '';
        if (klass === 'gnome-shell' || appId === 'org.gnome.Shell')
            return;
        const pid = window.get_pid();
        if (pid <= 0)
            return;
        // A string, matching what the KWin script sends.
        Gio.DBus.session.call(
            'org.scdr.Session1', '/org/scdr/Session1', 'org.scdr.Session1',
            'SetForeground', new GLib.Variant('(s)', [String(pid)]),
            null, Gio.DBusCallFlags.NO_AUTO_START, -1, null, null);
    }
}
