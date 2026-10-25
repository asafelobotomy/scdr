// Reports the focused window's process to the SCDR session agent
// (org.scdr.Session1 on the session bus), which relays it to scdrd.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
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
        const pid = window?.get_pid() ?? -1;
        if (pid <= 0)
            return;
        // A string, matching what the KWin script sends.
        Gio.DBus.session.call(
            'org.scdr.Session1', '/org/scdr/Session1', 'org.scdr.Session1',
            'SetForeground', new GLib.Variant('(s)', [String(pid)]),
            null, Gio.DBusCallFlags.NO_AUTO_START, -1, null, null);
    }
}
