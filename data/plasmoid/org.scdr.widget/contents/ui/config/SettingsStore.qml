// The widget's settings, as scdrd keeps them: `base` is every config file
// merged (read-only here), `layer` the settings this widget owns, saved
// through SetSettings to /var/lib/scdr/settings.toml. Values are addressed
// by path, e.g. ["pressure", "high"]; a value missing from the layer falls
// back to the base. Bind to `revision` to follow edits.

import QtQuick
import org.kde.plasma.workspace.dbus as DBus

QtObject {
    id: store

    readonly property string busName: "org.scdr.Daemon1"
    readonly property string objectPath: "/org/scdr/Daemon1"

    property var base: ({})
    property var layer: ({})
    property var info: ({})
    property bool loaded: false
    property bool dirty: false
    property bool busy: false
    property string error: ""
    property int revision: 0

    signal saved(string summary)

    function call(member, args, done) {
        const msg = { service: busName, path: objectPath, iface: busName, member: member, arguments: args || [] };
        const reply = DBus.SystemBus.asyncCall(msg);
        reply.finished.connect(() => {
            if (reply.isError) {
                store.error = reply.error.message || i18n("scdrd didn't answer");
                store.busy = false;
                return;
            }
            if (done) {
                done(reply.value);
            }
        });
    }

    function load() {
        busy = true;
        call("Settings", [], value => {
            let s;
            try {
                s = JSON.parse(value);
            } catch (e) {
                error = i18n("Couldn't read scdrd's settings: %1", e.message);
                busy = false;
                return;
            }
            info = s;
            base = s.base || {};
            layer = s.layer || {};
            loaded = true;
            dirty = false;
            busy = false;
            error = "";
            revision++;
        });
    }

    function save() {
        busy = true;
        error = "";
        call("SetSettings", [JSON.stringify(layer)], summary => {
            dirty = false;
            busy = false;
            saved(summary);
            load();
        });
    }

    function lookup(obj, path) {
        let v = obj;
        for (const key of path) {
            if (v === null || v === undefined || typeof v !== "object" || !(key in v)) {
                return undefined;
            }
            v = v[key];
        }
        return v;
    }

    function baseValue(path) {
        return lookup(base, path);
    }

    function layerValue(path) {
        return lookup(layer, path);
    }

    function isSet(path) {
        return layerValue(path) !== undefined;
    }

    function effective(path) {
        const v = layerValue(path);
        return v !== undefined ? v : baseValue(path);
    }

    // Sets a layer value; undefined removes it (falling back to the base)
    // along with any tables it leaves empty.
    function set(path, value) {
        const copy = JSON.parse(JSON.stringify(layer));
        let obj = copy;
        const parents = [];
        for (const key of path.slice(0, -1)) {
            if (typeof obj[key] !== "object" || obj[key] === null || Array.isArray(obj[key])) {
                obj[key] = {};
            }
            parents.push([obj, key]);
            obj = obj[key];
        }
        const last = path[path.length - 1];
        if (value === undefined) {
            delete obj[last];
            for (let i = parents.length - 1; i >= 0; i--) {
                const [parent, key] = parents[i];
                if (Object.keys(parent[key]).length === 0) {
                    delete parent[key];
                }
            }
        } else {
            obj[last] = value;
        }
        layer = copy;
        dirty = true;
        revision++;
    }

    // Names of every profile, from the base and the layer.
    function profileNames() {
        const names = new Set(Object.keys(base.profile || {}).concat(Object.keys(layer.profile || {})));
        return Array.from(names).sort();
    }

    Component.onCompleted: load()
}
