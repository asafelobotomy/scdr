// The Scheduling page in a Loader with a FakeContext, and helpers to find what it shows and
// what it asked for. Shared by tst_page.qml and tst_editors.qml.
import QtQuick

Item {
    id: harness
    anchors.fill: parent

    readonly property url pageUrl: Qt.resolvedUrl("../../uq/pages/scdr/Page.qml")
    readonly property alias fake: fake
    readonly property alias loader: loader
    readonly property var page: loader.item
    // The D-Bus methods that change something.
    readonly property var writeMethods: ["Pause", "Resume", "SetRoleEnabled", "SetSettings", "TryScheduler", "EndSchedulerTrial"]

    function stateDoc(over) {
        return Object.assign({
            paused: false,
            managed: 12,
            power_profile: "balanced",
            cpu_pressure: 34.2,
            pressure_active: false,
            scheduler: ["bpfland", "auto"],
            roles: [["foreground", "desktop", true], ["audio", "realtime", true], ["game", "game", true], ["background", "background", false]],
            agents: 1,
            boosted: [
                { pid: 2101, name: "firefox", profile: "desktop", role: "foreground", why: "x", app: null },
                { pid: 2230, name: "<b>x</b>", profile: "realtime", role: "audio", why: "y", app: null }
            ]
        }, over || {});
    }

    function settingsDoc(rules, over) {
        return Object.assign({
            layer: {
                sched_ext: { enable: true, default: "bpfland", power: { performance: "lavd" } },
                rule: rules || [{ profile: "desktop", name: "firefox" }, { profile: "game", name: ["steam*"] }, { profile: "background", exe: "/x/*", name: "x" }]
            },
            base: {
                profile: { desktop: { nice: -2, _source: "/usr/share/scdr/scdr.toml" }, game: {}, background: {} },
                roles: { foreground: "desktop" },
                rule: [{ profile: "background", name: "baloo*", _source: "/usr/share/scdr/scdr.toml" }]
            },
            schedulers: [
                { name: "bpfland", modes: ["auto", "gaming"], installed: true, description: "General purpose" },
                { name: "lavd", modes: ["auto"], installed: true, description: "Low latency" },
                { name: "devbox", modes: ["auto"], installed: true, description: "For development" },
                { name: "rusty", modes: ["auto"], installed: false, description: "" }
            ],
            scx_loader_error: null,
            current_scheduler: "scx_bpfland:auto",
            trial: null,
            power_profiles: ["power-saver", "balanced", "performance"]
        }, over || {});
    }

    function stateResult(over) {
        return { code: 0, output: { type: "s", data: JSON.stringify(stateDoc(over)) } };
    }

    function arrange(over) {
        const o = over || {};
        fake.confirms = true;
        fake.commands = Object.assign({
            "state": stateResult(),
            "agent-state": { code: 0, output: "enabled\n" },
            "agent-on": { code: 0, output: null },
            "agent-off": { code: 0, output: null },
            "start": { code: 0, output: null },
            "installer-which": { code: 0, output: "pamac\n" },
            "installer-install": { code: 0, output: null },
            "installer-update": { code: 0, output: null },
            "installer-git": { code: 0, output: null }
        }, o.commands || {});
        fake.dbus = Object.assign({
            "Settings": { value: JSON.stringify(settingsDoc()) },
            "Pause": { value: null },
            "Resume": { value: null },
            "SetRoleEnabled": { value: null },
            "SetSettings": { value: "3 profiles, 4 rules" },
            "TryScheduler": { value: null },
            "EndSchedulerTrial": { value: null }
        }, o.dbus || {});
    }

    function walk(item, test) {
        if (test(item))
            return item;
        for (let i = 0; i < item.children.length; ++i) {
            const hit = walk(item.children[i], test);
            if (hit)
                return hit;
        }
        return null;
    }
    function findLabel(text) {
        return walk(loader.item, it => it.text === text && it.textFormat !== undefined);
    }
    function findButton(text) {
        return walk(loader.item, it => it.text === text && it.clicked !== undefined);
    }
    function findNamed(name) {
        return walk(loader.item, it => it.objectName === name);
    }
    function findRow(label) {
        return walk(loader.item, it => it.label === label && it.toggle !== undefined);
    }

    function writes() {
        return fake.calls.filter(c => c[0] === "call" && harness.writeMethods.indexOf(c[1]) >= 0);
    }
    // The layer sent by the last SetSettings.
    function sentLayer() {
        const w = writes().filter(c => c[1] === "SetSettings");
        return w.length ? JSON.parse(w[w.length - 1][2][0]) : null;
    }
    function flip(row, on) {
        row.toggle.checked = on;
        row.toggle.toggled();
    }
    // Picks `text` in a ChoiceBox as the person would.
    function pick(box, text) {
        const i = box.model.indexOf(text);
        if (i < 0)
            throw new Error("no choice " + text + " in " + JSON.stringify(box.model));
        box.currentIndex = i;
        box.activated(i);
    }

    FakeContext {
        id: fake
        confirmed: ["agent-off", "installer-install", "installer-update", "installer-git"]
    }

    Loader {
        id: loader
        anchors.fill: parent
    }
}
