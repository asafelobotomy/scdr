// The Scheduling page offscreen, with the stub org.uq.settings module and a fake ctx:
//   QT_QPA_PLATFORM=offscreen qmltestrunner -import tests/uq/stub -input tests/uq
import QtQuick
import QtTest

Item {
    id: root
    width: 600
    height: 400

    PageHarness {
        id: h
    }

    TestCase {
        name: "SchedulingPage"
        when: windowShown

        function load(over) {
            h.fake.calls = [];
            h.arrange(over);
            h.loader.source = "";
            h.loader.source = h.pageUrl;
            verify(h.loader.status === Loader.Ready, "the page loads");
            h.loader.item.ctx = h.fake.ctx;
        }

        function loaded() {
            tryVerify(() => h.findLabel("scdr is running") !== null);
            tryVerify(() => h.findNamed("rule-0") !== null);
        }

        function test_initial_reads() {
            load();
            loaded();
            verify(h.fake.calls.some(c => c[0] === "watch" && c[1] === "state"));
            verify(h.fake.calls.some(c => c[0] === "watch" && c[1] === "agent-state"));
            verify(h.fake.calls.some(c => c[0] === "call" && c[1] === "Settings"));
            compare(h.writes().length, 0);
        }

        function test_shows_state() {
            load();
            loaded();
            verify(h.findLabel("firefox — desktop") !== null);
            verify(h.findLabel("<b>x</b> — realtime") !== null);
            verify(h.findLabel("CPU busy: 34 % · 12 apps managed") !== null);
            verify(h.findLabel("Running now: bpfland") !== null);
            verify(h.findLabel("steam* → game") !== null);
        }

        function test_a_change_reads_again() {
            load();
            loaded();
            h.fake.commands = Object.assign({}, h.fake.commands, { "state": h.stateResult({ paused: true }) });
            h.fake.change("state");
            tryVerify(() => h.findLabel("scdr is paused") !== null);
            verify(h.findButton("Resume") !== null);
        }

        function test_role_switch_calls_dbus() {
            load();
            loaded();
            h.flip(h.findRow("Games"), false);
            tryVerify(() => h.writes().length === 1);
            compare(h.writes()[0].slice(0, 3), ["call", "SetRoleEnabled", ["game", false]]);
        }

        function test_failed_write_restores() {
            load({ dbus: { "SetRoleEnabled": { error: "org.freedesktop.DBus.Error.AccessDenied: not authorized" } } });
            loaded();
            h.flip(h.findRow("Games"), false);
            tryVerify(() => h.findLabel("You need to enter your password to change this.") !== null);
            // The roles are rebuilt by the re-read, so look the row up again.
            tryVerify(() => h.findRow("Games") !== null && h.findRow("Games").value === true);
            compare(h.findRow("Games").toggle.checked, true);
        }

        function test_cancel_is_silent() {
            load({ commands: { "agent-off": { error: "Cancelled", cancelled: true } } });
            loaded();
            const row = h.findRow("Follow the focused window");
            tryVerify(() => row.value === true);
            h.flip(row, false);
            tryVerify(() => h.fake.calls.some(c => c[0] === "run" && c[1] === "agent-off"));
            wait(100);
            verify(h.findLabel("Cancelled") === null);
            verify(h.findLabel("Something went wrong.") === null);
            compare(row.toggle.checked, true);
        }

        function test_agent_switch_runs_commands() {
            load({ commands: { "agent-state": { code: 1, error: "exit 1", output: "disabled\n" } } });
            loaded();
            const row = h.findRow("Follow the focused window");
            tryVerify(() => row.enabled);
            compare(row.value, false);
            h.flip(row, true);
            tryVerify(() => h.fake.calls.some(c => c[0] === "run" && c[1] === "agent-on"));
        }

        function test_not_running() {
            const down = "exit 1: Failed to get property State: The name org.scdr.Daemon1 was not provided by any .service files";
            load({ commands: { "state": { code: 1, error: down } },
                   dbus: { "Settings": { error: "org.freedesktop.DBus.Error.ServiceUnknown: The name is not known" } } });
            tryVerify(() => h.findLabel("scdr isn't running") !== null);
            const start = h.findButton("Start scdr");
            verify(start !== null);
            verify(h.findLabel("Boosted now") !== null && !h.findLabel("Boosted now").visible);
            start.clicked();
            tryVerify(() => h.fake.calls.some(c => c[0] === "run" && c[1] === "start"));
            compare(h.writes().length, 0);
        }

        function test_starts_while_open() {
            const down = "exit 1: The name org.scdr.Daemon1 was not provided by any .service files";
            load({ commands: { "state": { code: 1, error: down } },
                   dbus: { "Settings": { error: "org.freedesktop.DBus.Error.ServiceUnknown: not known" } } });
            tryVerify(() => h.findLabel("scdr isn't running") !== null);
            h.fake.commands = Object.assign({}, h.fake.commands, { "state": h.stateResult() });
            h.fake.dbus = Object.assign({}, h.fake.dbus, { "Settings": { value: JSON.stringify(h.settingsDoc()) } });
            h.fake.change("state");
            tryVerify(() => h.findLabel("scdr is running") !== null);
            tryVerify(() => h.findNamed("rule-0") !== null);
        }

        function test_state_that_cannot_be_read() {
            load({ commands: { "state": { code: 0, output: { type: "s", data: "{" } } } });
            tryVerify(() => h.findLabel("scdr's state couldn't be read.") !== null);
        }

        function test_pause_asks_first() {
            load();
            loaded();
            const dlg = h.loader.item.confirmDialog;
            h.findButton("Pause").clicked();
            compare(dlg.message, "Pause scdr? Every app's priority goes back to normal until you resume.");
            wait(50);
            compare(h.writes().length, 0);
            dlg.reject();
            wait(50);
            compare(h.writes().length, 0);
            h.findButton("Pause").clicked();
            dlg.accept();
            tryVerify(() => h.writes().length === 1);
            compare(h.writes()[0].slice(0, 3), ["call", "Pause", []]);
        }

        function test_resume_is_direct() {
            load({ commands: { "state": h.stateResult({ paused: true }) } });
            tryVerify(() => h.findLabel("scdr is paused") !== null);
            h.findButton("Resume").clicked();
            tryVerify(() => h.writes().length === 1);
            compare(h.writes()[0].slice(0, 3), ["call", "Resume", []]);
        }

        function test_rule_add_validates() {
            load();
            loaded();
            const field = h.findNamed("rule-name");
            field.text = "-rf";
            tryVerify(() => h.findLabel("Use letters, numbers, spaces and . _ + * ? @ - only, starting with a letter or number. 64 characters at most.") !== null);
            h.loader.item.addRule();
            wait(50);
            compare(h.writes().length, 0);
            field.text = "kate";
            const profile = h.findNamed("rule-profile");
            tryVerify(() => profile.model.indexOf("desktop") >= 0);
            profile.currentIndex = profile.model.indexOf("desktop");
            h.findNamed("rule-add").clicked();
            tryVerify(() => h.writes().length === 1);
            compare(h.writes()[0][1], "SetSettings");
            const doc = JSON.parse(h.writes()[0][2][0]);
            compare(doc.rule.length, 4);
            compare(doc.rule[3], { profile: "desktop", name: "kate" });
            compare(doc.sched_ext.power.performance, "lavd");
        }

        function test_rule_remove_asks_first() {
            load();
            loaded();
            const dlg = h.loader.item.confirmDialog;
            verify(h.findNamed("remove-2") !== null && h.findNamed("remove-2").visible, "a rule with other conditions can be removed too");
            h.findNamed("remove-1").clicked();
            compare(dlg.message, "Remove your rule for steam*?");
            wait(50);
            compare(h.writes().length, 0);
            dlg.reject();
            wait(50);
            compare(h.writes().length, 0);
            h.findNamed("remove-1").clicked();
            dlg.accept();
            tryVerify(() => h.writes().length === 1);
            const doc = JSON.parse(h.writes()[0][2][0]);
            compare(doc.rule.map(r => r.profile), ["desktop", "background"]);
        }

        function test_scheduler_change() {
            load();
            loaded();
            const combo = h.findNamed("scheduler-default");
            tryVerify(() => combo.model.length === 6);
            compare(combo.model, ["Not set (Kernel default)", "Kernel default", "bpfland", "bpfland (gaming)", "lavd", "devbox"]);
            tryCompare(combo, "currentIndex", 2);
            h.pick(combo, "lavd");
            tryVerify(() => h.writes().length === 1);
            const doc = JSON.parse(h.writes()[0][2][0]);
            compare(doc.sched_ext.default, "lavd");
            compare(doc.sched_ext.enable, true);
            compare(doc.sched_ext.power.performance, "lavd");
        }

        function test_scheduler_failure_restores() {
            load({ dbus: { "SetSettings": { error: "org.freedesktop.DBus.Error.AccessDenied: not authorized" } } });
            loaded();
            const combo = h.findNamed("scheduler-default");
            tryVerify(() => combo.model.length === 6);
            h.pick(combo, "Kernel default");
            tryVerify(() => h.findLabel("You need to enter your password to change this.") !== null);
            tryVerify(() => combo.currentIndex === 2);
        }

        function test_a_settings_document_too_big_makes_no_call() {
            const rules = [];
            for (let i = 0; i < 480; ++i)
                rules.push({ profile: "desktop", name: "app" + i });
            load({ dbus: { "Settings": { value: JSON.stringify(h.settingsDoc(rules)) } } });
            loaded();
            h.findNamed("rule-name").text = "kate";
            const profile = h.findNamed("rule-profile");
            tryVerify(() => profile.model.indexOf("desktop") >= 0);
            h.findNamed("rule-add").clicked();
            tryVerify(() => h.findLabel("Too many settings to change here; use scdr's own settings.") !== null);
            compare(h.writes().length, 0);
        }

        function test_untrusted_text_plain() {
            load();
            loaded();
            const label = h.findLabel("<b>x</b> — realtime");
            verify(label !== null);
            compare(label.textFormat, Text.PlainText);
        }

        function test_highlight_scrolls() {
            load();
            loaded();
            const page = h.loader.item;
            compare(page.flick.contentY, 0);
            page.highlight = "rules";
            tryVerify(() => page.flick.contentY > 0);
            const y = page.sections.rules.mapToItem(page.flick, 0, 0).y;
            verify(y >= -1 && y < page.flick.height, "the rules section is in view: " + y);
        }
    }
}
