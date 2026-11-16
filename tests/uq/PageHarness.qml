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
    readonly property var writeMethods: ["Resume", "SetRoleEnabled", "SetSettings", "TryScheduler", "EndSchedulerTrial", "Reload"]

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

    // `scdrctl config show --json` as the page reads it; `over` replaces rows by key.
    function configDoc(over, layer) {
        const row = (key, type, group, label, extra) => Object.assign({ key: key, type: type, group: group, label: label, help: "", value: null, source: "default", default: null, nullable: false }, extra || {});
        const rows = [
            row("system.iocost", "bool", "System", "Disk weights on every disk", { value: false, source: "files", default: false }),
            row("system.autogroup", "bool", "System", "Kernel autogroup", { nullable: true }),
            row("system.rescan_secs", "uint", "System", "Full rescan every", { value: 60, source: "files", default: 60, min: 0, max: 86400, unit: "s" }),
            row("system.exec_recheck_ms", "uint", "System", "Look again after a start", { default: 1500, min: 0, max: 60000, unit: "ms" }),
            row("split.apps", "list", "Agent tools", "Apps whose tools get their own group", { value: ["claude"], source: "you" }),
            row("split.memory_high", "text", "Agent tools", "Memory soft limit"),
            row("exceptions.name", "list", "Left alone", "Never touch these", { value: [], inherited: ["bwrap"] })
        ].map(r => Object.assign(r, over && over[r.key] ? over[r.key] : {}));
        return { ok: true, layer_file: "/var/lib/scdr/settings.toml", settings: rows, layer: layer || { rules: 3, profiles: 0, empty: false } };
    }

    function plan(over) {
        return Object.assign({ summary: "1 change", changes: [{ group: "Settings", label: "x", detail: "a → b", reversible: true }], would_refuse: false, reason: "" }, over || {});
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
            "installer-git": { code: 0, output: null },
            "pause": { code: 0, output: null },
            "config": { code: 0, output: configDoc() },
            "config-set": { code: 0, output: { ok: true } },
            "config-unset": { code: 0, output: { ok: true } },
            "config-reset": { code: 0, output: { ok: true } },
            "check": { code: 0, output: { ok: true, profiles: 13, rules: 11, files: ["/usr/share/scdr/scdr.toml"], warnings: ["rule 3 shadows rule 5"] } },
            "list": { code: 0, output: { ok: true, processes: [{ pid: 2101, name: "firefox", profile: "desktop", rule: "scdr.toml #3", excepted: false }, { pid: 2230, name: "<b>x</b>", profile: "game", rule: "", excepted: false }] } },
            "list-all": { code: 0, output: { ok: true, processes: [{ pid: 1, name: "systemd", profile: "-", rule: "", excepted: false }] } },
            "bench-check": { code: 0, output: { ok: true, can_run: true, reason: "", seconds: 40 } },
            "bench": { code: 0, output: null },
            "preview-import-ananicy": { code: 0, output: plan({ summary: "1 profile, 1 rule", changes: [{ group: "Added", label: "1 rule", detail: "1 name pattern", reversible: true }, { group: "Left out", label: "bar: unknown type" }] }) },
            "preview-import-system76": { code: 0, output: plan() }
        }, o.commands || {});
        fake.events = Object.assign({
            "bench": [{ event: "scenario", name: "baseline", round: 1, rounds: 1, percent: 0 }, { event: "scenario", name: "probe slice 2000us (foreground)", round: 1, rounds: 1, percent: 50 }, { event: "done", percent: 100, results: [{ scenario: { name: "baseline" }, p50_us: 80.4, p99_us: 900, max_us: 5000 }] }]
        }, o.events || {});
        fake.dbus = Object.assign({
            "Settings": { value: JSON.stringify(settingsDoc()) },
            "Status": { value: JSON.stringify({ version: "0.4.2", dry_run: false, uptime_secs: 7200, power_profile: "balanced", tracked: 300, managed: 12, excepted: 4, governors: ["powersave"], cpufreq_driver: ["amd-pstate-epp", "active"], memory_pressure: 1.2, io_pressure: 0.3, disks: [{ name: "nvme0n1", dev: "259:0", scheduler: "none", iocost: false }], notes: [] }) },
            "Explain": { value: JSON.stringify({ pid: 2101, name: "firefox", state: "managed", reason: null, profile: "desktop", profile_chain: ["desktop", "base"], rule: { source: "/usr/share/scdr/scdr.toml", index: 3, conditions: "name = firefox" }, notes: ["a note"], errors: [] }) },
            "Reload": { value: "13 profiles, 11 rules" },
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

    // Opens a tab of the page, as the person would: by its button.
    function tab(title) {
        const button = findNamed("tab-" + title);
        if (button === null)
            throw new Error("no tab " + title);
        button.clicked();
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
        confirmed: ["agent-off", "installer-install", "installer-update", "installer-git", "pause", "config-reset", "bench"]
    }

    Loader {
        id: loader
        anchors.fill: parent
    }
}
