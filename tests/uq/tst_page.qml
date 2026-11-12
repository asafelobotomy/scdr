// The Scheduling page offscreen, with the stub org.uq.settings module and a fake ctx:
//   QT_QPA_PLATFORM=offscreen qmltestrunner -import tests/uq/stub -input tests/uq
import QtQuick
import QtTest

Item {
    id: root
    width: 600
    height: 400

    readonly property url pageUrl: Qt.resolvedUrl("../../uq/pages/scdr/Page.qml")

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

    function settingsDoc(rules) {
        return {
            layer: {
                sched_ext: { enable: true, default: "bpfland", power: { performance: "lavd" } },
                rule: rules || [{ profile: "desktop", name: "firefox" }, { profile: "game", name: ["steam*"] }, { profile: "background", exe: "/x/*", name: "x" }]
            },
            base: { profile: { desktop: {}, game: {}, background: {} } },
            schedulers: [
                { name: "bpfland", modes: ["auto", "gaming"], installed: true },
                { name: "lavd", modes: ["auto"], installed: true }
            ],
            scx_loader_error: null,
            current_scheduler: "scx_bpfland:auto"
        };
    }

    function stateResult(over) {
        return { code: 0, output: { type: "s", data: JSON.stringify(stateDoc(over)) } };
    }

    FakeContext {
        id: fake
        confirmed: ["agent-off"]
    }

    Loader {
        id: loader
        anchors.fill: parent
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

    // The D-Bus calls that change something.
    function writes() {
        return fake.calls.filter(c => c[0] === "call" && ["Pause", "Resume", "SetRoleEnabled", "SetSettings"].indexOf(c[1]) >= 0);
    }
    function flip(row, on) {
        row.toggle.checked = on;
        row.toggle.toggled();
    }

    function arrange(over) {
        const o = over || {};
        fake.confirms = true;
        fake.commands = Object.assign({
            "state": stateResult(),
            "agent-state": { code: 0, output: "enabled\n" },
            "agent-on": { code: 0, output: null },
            "agent-off": { code: 0, output: null },
            "start": { code: 0, output: null }
        }, o.commands || {});
        fake.dbus = Object.assign({
            "Settings": { value: JSON.stringify(settingsDoc()) },
            "Pause": { value: null },
            "Resume": { value: null },
            "SetRoleEnabled": { value: null },
            "SetSettings": { value: "3 profiles, 4 rules" }
        }, o.dbus || {});
    }

    TestCase {
        name: "SchedulingPage"
        when: windowShown

        function load(over) {
            fake.calls = [];
            arrange(over);
            loader.source = "";
            loader.source = root.pageUrl;
            verify(loader.status === Loader.Ready, "the page loads");
            loader.item.ctx = fake.ctx;
        }

        function loaded() {
            tryVerify(() => findLabel("scdr is running") !== null);
            tryVerify(() => findNamed("rule-0") !== null);
        }

        function test_initial_reads() {
            load();
            loaded();
            verify(fake.calls.some(c => c[0] === "watch" && c[1] === "state"));
            verify(fake.calls.some(c => c[0] === "watch" && c[1] === "agent-state"));
            verify(fake.calls.some(c => c[0] === "call" && c[1] === "Settings"));
            compare(writes().length, 0);
        }

        function test_shows_state() {
            load();
            loaded();
            verify(findLabel("firefox — desktop") !== null);
            verify(findLabel("<b>x</b> — realtime") !== null);
            verify(findLabel("CPU busy: 34 % · 12 apps managed") !== null);
            verify(findLabel("Running now: bpfland") !== null);
            verify(findLabel("steam* → game") !== null);
        }

        function test_a_change_reads_again() {
            load();
            loaded();
            fake.commands = Object.assign({}, fake.commands, { "state": stateResult({ paused: true }) });
            fake.change("state");
            tryVerify(() => findLabel("scdr is paused") !== null);
            verify(findButton("Resume") !== null);
        }

        function test_role_switch_calls_dbus() {
            load();
            loaded();
            flip(findRow("Games"), false);
            tryVerify(() => writes().length === 1);
            compare(writes()[0].slice(0, 3), ["call", "SetRoleEnabled", ["game", false]]);
        }

        function test_failed_write_restores() {
            load({ dbus: { "SetRoleEnabled": { error: "org.freedesktop.DBus.Error.AccessDenied: not authorized" } } });
            loaded();
            flip(findRow("Games"), false);
            tryVerify(() => findLabel("You need to enter your password to change this.") !== null);
            // The roles are rebuilt by the re-read, so look the row up again.
            tryVerify(() => findRow("Games") !== null && findRow("Games").value === true);
            compare(findRow("Games").toggle.checked, true);
        }

        function test_cancel_is_silent() {
            load({ commands: { "agent-off": { error: "Cancelled", cancelled: true } } });
            loaded();
            const row = findRow("Follow the focused window");
            tryVerify(() => row.value === true);
            flip(row, false);
            tryVerify(() => fake.calls.some(c => c[0] === "run" && c[1] === "agent-off"));
            wait(100);
            verify(findLabel("Cancelled") === null);
            verify(findLabel("Something went wrong.") === null);
            compare(row.toggle.checked, true);
        }

        function test_agent_switch_runs_commands() {
            load({ commands: { "agent-state": { code: 1, error: "exit 1", output: "disabled\n" } } });
            loaded();
            const row = findRow("Follow the focused window");
            tryVerify(() => row.enabled);
            compare(row.value, false);
            flip(row, true);
            tryVerify(() => fake.calls.some(c => c[0] === "run" && c[1] === "agent-on"));
        }

        function test_not_running() {
            const down = "exit 1: Failed to get property State: The name org.scdr.Daemon1 was not provided by any .service files";
            load({ commands: { "state": { code: 1, error: down } },
                   dbus: { "Settings": { error: "org.freedesktop.DBus.Error.ServiceUnknown: The name is not known" } } });
            tryVerify(() => findLabel("scdr isn't running") !== null);
            const start = findButton("Start scdr");
            verify(start !== null);
            verify(findLabel("Boosted now") !== null && !findLabel("Boosted now").visible);
            start.clicked();
            tryVerify(() => fake.calls.some(c => c[0] === "run" && c[1] === "start"));
            compare(writes().length, 0);
        }

        function test_starts_while_open() {
            const down = "exit 1: The name org.scdr.Daemon1 was not provided by any .service files";
            load({ commands: { "state": { code: 1, error: down } },
                   dbus: { "Settings": { error: "org.freedesktop.DBus.Error.ServiceUnknown: not known" } } });
            tryVerify(() => findLabel("scdr isn't running") !== null);
            fake.commands = Object.assign({}, fake.commands, { "state": stateResult() });
            fake.dbus = Object.assign({}, fake.dbus, { "Settings": { value: JSON.stringify(settingsDoc()) } });
            fake.change("state");
            tryVerify(() => findLabel("scdr is running") !== null);
            tryVerify(() => findNamed("rule-0") !== null);
        }

        function test_state_that_cannot_be_read() {
            load({ commands: { "state": { code: 0, output: { type: "s", data: "{" } } } });
            tryVerify(() => findLabel("scdr's state couldn't be read.") !== null);
        }

        function test_pause_asks_first() {
            load();
            loaded();
            const dlg = loader.item.confirmDialog;
            findButton("Pause").clicked();
            compare(dlg.message, "Pause scdr? Every app's priority goes back to normal until you resume.");
            wait(50);
            compare(writes().length, 0);
            dlg.reject();
            wait(50);
            compare(writes().length, 0);
            findButton("Pause").clicked();
            dlg.accept();
            tryVerify(() => writes().length === 1);
            compare(writes()[0].slice(0, 3), ["call", "Pause", []]);
        }

        function test_resume_is_direct() {
            load({ commands: { "state": stateResult({ paused: true }) } });
            tryVerify(() => findLabel("scdr is paused") !== null);
            findButton("Resume").clicked();
            tryVerify(() => writes().length === 1);
            compare(writes()[0].slice(0, 3), ["call", "Resume", []]);
        }

        function test_rule_add_validates() {
            load();
            loaded();
            const field = findNamed("rule-name");
            field.text = "-rf";
            tryVerify(() => findLabel("Use letters, numbers, spaces and . _ + * ? @ - only, starting with a letter or number. 64 characters at most.") !== null);
            loader.item.addRule();
            wait(50);
            compare(writes().length, 0);
            field.text = "kate";
            const profile = findNamed("rule-profile");
            tryVerify(() => profile.model.indexOf("desktop") >= 0);
            profile.currentIndex = profile.model.indexOf("desktop");
            findNamed("rule-add").clicked();
            tryVerify(() => writes().length === 1);
            compare(writes()[0][1], "SetSettings");
            const doc = JSON.parse(writes()[0][2][0]);
            compare(doc.rule.length, 4);
            compare(doc.rule[3], { profile: "desktop", name: "kate" });
            compare(doc.sched_ext.power.performance, "lavd");
        }

        function test_rule_remove_asks_first() {
            load();
            loaded();
            const dlg = loader.item.confirmDialog;
            verify(findNamed("remove-2") !== null && !findNamed("remove-2").visible, "a rule with other conditions has no Remove");
            findNamed("remove-1").clicked();
            compare(dlg.message, "Remove your rule for steam*?");
            wait(50);
            compare(writes().length, 0);
            dlg.reject();
            wait(50);
            compare(writes().length, 0);
            findNamed("remove-1").clicked();
            dlg.accept();
            tryVerify(() => writes().length === 1);
            const doc = JSON.parse(writes()[0][2][0]);
            compare(doc.rule.map(r => r.profile), ["desktop", "background"]);
        }

        function test_scheduler_change() {
            load();
            loaded();
            const combo = findNamed("scheduler-choice");
            tryVerify(() => combo.model.length === 4);
            compare(combo.model[0], "Kernel default");
            compare(combo.currentIndex, 1);
            combo.currentIndex = 3;
            combo.activated(3);
            tryVerify(() => writes().length === 1);
            const doc = JSON.parse(writes()[0][2][0]);
            compare(doc.sched_ext.default, "lavd");
            compare(doc.sched_ext.enable, true);
            compare(doc.sched_ext.power.performance, "lavd");
        }

        function test_scheduler_failure_restores() {
            load({ dbus: { "SetSettings": { error: "org.freedesktop.DBus.Error.AccessDenied: not authorized" } } });
            loaded();
            const combo = findNamed("scheduler-choice");
            tryVerify(() => combo.model.length === 4);
            combo.currentIndex = 0;
            combo.activated(0);
            tryVerify(() => findLabel("You need to enter your password to change this.") !== null);
            tryVerify(() => combo.currentIndex === 1);
        }

        function test_a_settings_document_too_big_makes_no_call() {
            const rules = [];
            for (let i = 0; i < 480; ++i)
                rules.push({ profile: "desktop", name: "app" + i });
            load({ dbus: { "Settings": { value: JSON.stringify(settingsDoc(rules)) } } });
            loaded();
            findNamed("rule-name").text = "kate";
            const profile = findNamed("rule-profile");
            tryVerify(() => profile.model.indexOf("desktop") >= 0);
            findNamed("rule-add").clicked();
            tryVerify(() => findLabel("Too many settings to change here; use scdr's own settings.") !== null);
            compare(writes().length, 0);
        }

        function test_untrusted_text_plain() {
            load();
            loaded();
            const label = findLabel("<b>x</b> — realtime");
            verify(label !== null);
            compare(label.textFormat, Text.PlainText);
        }

        function test_highlight_scrolls() {
            load();
            loaded();
            const page = loader.item;
            compare(page.flick.contentY, 0);
            page.highlight = "rules";
            tryVerify(() => page.flick.contentY > 0);
            const y = page.sections.rules.mapToItem(page.flick, 0, 0).y;
            verify(y >= -1 && y < page.flick.height, "the rules section is in view: " + y);
        }
    }
}
