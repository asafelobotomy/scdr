// The Scheduling page's tabs beyond the settings editors: Overview, Rules' Explain and running
// apps, Tools and Preferences, offscreen with the stub org.uq.settings module and a fake ctx.
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
        name: "SchedulingTabs"
        when: windowShown

        function load(over) {
            h.fake.calls = [];
            h.arrange(over);
            h.loader.source = "";
            h.loader.source = h.pageUrl;
            verify(h.loader.status === Loader.Ready, "the page loads");
            h.loader.item.ctx = h.fake.ctx;
            tryVerify(() => h.findLabel("scdr is running") !== null);
            tryVerify(() => h.findNamed("rule-0") !== null);
        }

        function runs(id) {
            return h.fake.calls.filter(c => c[0] === "run" && c[1] === id);
        }

        function test_overview_shows_pressure_boosts_and_facts() {
            load();
            const bar = h.findNamed("pressure-bar");
            tryVerify(() => Math.abs(bar.value - 34.2) < 0.01);
            verify(h.findLabel("Apps that aren't focused are left alone. They are lowered above 20 %.") !== null);
            verify(h.findLabel("firefox") !== null);
            verify(h.findLabel("nvme0n1") === null);
            tryVerify(() => h.findLabel("balanced") !== null);
            verify(h.findLabel("amd-pstate-epp (active)") !== null);
            verify(h.findLabel("Disk nvme0n1") !== null);
        }

        function test_down_shows_a_banner_and_keeps_the_tabs() {
            const down = "exit 1: The name org.scdr.Daemon1 was not provided by any .service files";
            h.fake.calls = [];
            h.arrange({ commands: { "state": { code: 1, error: down } },
                        dbus: { "Settings": { error: "org.freedesktop.DBus.Error.ServiceUnknown: x" }, "Status": { error: "org.freedesktop.DBus.Error.ServiceUnknown: x" } } });
            h.loader.source = "";
            h.loader.source = h.pageUrl;
            h.loader.item.ctx = h.fake.ctx;
            tryVerify(() => h.findNamed("banner-down") !== null && h.findNamed("banner-down").visible);
            verify(h.findNamed("tab-Preferences") !== null && h.findNamed("tab-Tools") !== null && h.findNamed("tab-Rules") !== null);
            verify(h.findNamed("tab-Scheduler") === null, "nothing to show on the scheduler tab while scdr is off");
            h.findNamed("banner-down").button.clicked();
            tryVerify(() => runs("start").length === 1);
        }

        function test_preferences_are_live_and_switch_by_command() {
            load();
            h.tab("Preferences");
            const iocost = h.findNamed("pref-system.iocost");
            tryVerify(() => iocost !== null && !iocost.checked);
            iocost.checked = true;
            iocost.toggled();
            tryVerify(() => runs("config-set").length === 1);
            compare(runs("config-set")[0][2], ["system.iocost", "on"]);
            // The file changed: the row follows.
            h.fake.commands = Object.assign({}, h.fake.commands, { "config": { code: 0, output: h.configDoc({ "system.iocost": { value: true, source: "you" } }) } });
            h.fake.change("config");
            tryVerify(() => h.findNamed("pref-system.iocost").checked);
            verify(h.findLabel("Set here · now On") !== null);
        }

        function test_a_number_and_a_list_are_written_as_text() {
            load();
            h.tab("Preferences");
            const rescan = h.findNamed("pref-system.rescan_secs");
            tryVerify(() => rescan !== null);
            rescan.chosen(120);
            tryVerify(() => runs("config-set").length === 1);
            compare(runs("config-set")[0][2], ["system.rescan_secs", "120"]);
            tryVerify(() => h.loader.item.workingKey === "");
            h.fake.calls = [];
            tryVerify(() => h.findNamed("pref-split.apps") !== null && h.findNamed("pref-split.apps").text === "claude");
            const apps = h.findNamed("pref-split.apps");
            apps.text = "claude, aider";
            apps.accepted();
            tryVerify(() => runs("config-set").length === 1);
            compare(runs("config-set")[0][2], ["split.apps", "claude, aider"]);
            tryVerify(() => h.loader.item.workingKey === "");
            h.fake.calls = [];
            apps.text = "";
            apps.accepted();
            tryVerify(() => runs("config-set").length === 1);
            compare(runs("config-set")[0][2], ["split.apps", "[]"]);
        }

        function test_a_refusal_is_shown_under_its_row() {
            load({ commands: { "config-set": { code: 1, error: "exit 1", output: { ok: false, error: "/var/lib/scdr/settings.toml: rescan_secs must be at most 86400" } } } });
            h.tab("Preferences");
            const rescan = h.findNamed("pref-system.rescan_secs");
            tryVerify(() => rescan !== null);
            rescan.chosen(90000);
            tryVerify(() => h.findLabel("/var/lib/scdr/settings.toml: rescan_secs must be at most 86400") !== null);
        }

        function test_dropping_a_value_unsets_it() {
            load();
            h.tab("Preferences");
            tryVerify(() => h.findNamed("pref-reset-split.memory_high") !== null);
            const apps = h.findNamed("pref-split.memory_high");
            apps.text = "2G";
            apps.accepted();
            tryVerify(() => runs("config-set").length === 1);
            compare(runs("config-set")[0][2], ["split.memory_high", "2G"]);
        }

        function test_reset_is_previewed_and_can_be_declined() {
            load();
            h.tab("Preferences");
            const button = h.findNamed("reset-mine");
            tryVerify(() => button !== null && button.enabled);
            h.fake.confirms = false;
            button.clicked();
            tryVerify(() => runs("config-reset").length === 1);
            verify(h.findLabel("Cancelled") === null);
            tryVerify(() => h.loader.item.workingKey === "");
            h.fake.confirms = true;
            button.clicked();
            tryVerify(() => runs("config-reset").length === 2);
            tryVerify(() => runs("config").length >= 1);
        }

        function test_a_config_that_does_not_load_is_a_banner() {
            load({ commands: { "config": { code: 1, error: "exit 1", output: { ok: false, error: "/etc/scdr/scdr.toml: bad key" } } } });
            tryVerify(() => h.findNamed("banner-config").visible);
            verify(h.findLabel("scdr's files don't load together: /etc/scdr/scdr.toml: bad key") !== null);
        }

        function test_explain_asks_the_daemon() {
            load();
            h.tab("Rules");
            const field = h.findNamed("explain-pid");
            field.text = "2101";
            field.accepted();
            tryVerify(() => h.fake.calls.some(c => c[0] === "call" && c[1] === "Explain" && c[2][0] === 2101));
            tryVerify(() => h.findLabel("managed") !== null);
            verify(h.findLabel("scdr.toml #3 (name = firefox)") !== null);
            verify(h.findLabel("a note") !== null);
        }

        function test_explain_of_nothing_says_so() {
            load({ dbus: { "Explain": { value: JSON.stringify({ pid: 1, name: null, state: "", notes: [], errors: [] }) } } });
            h.tab("Rules");
            const field = h.findNamed("explain-pid");
            field.text = "999";
            field.accepted();
            tryVerify(() => h.findLabel("No process has that number.") !== null);
        }

        function test_running_apps_filter_and_explain() {
            load();
            h.tab("Rules");
            h.findNamed("running-refresh").clicked();
            tryVerify(() => h.findLabel("2 running") !== null);
            verify(h.findLabel("desktop · scdr.toml #3") !== null);
            verify(h.findLabel("<b>x</b>") !== null);
            compare(h.findLabel("<b>x</b>").textFormat, Text.PlainText);
        }

        function test_check_and_reload() {
            load();
            h.tab("Tools");
            h.findNamed("check").clicked();
            tryVerify(() => h.findLabel("13 profiles and 11 rules load.") !== null);
            verify(h.findLabel("Warning: rule 3 shadows rule 5") !== null);
            h.findNamed("reload").clicked();
            tryVerify(() => h.fake.calls.some(c => c[0] === "call" && c[1] === "Reload"));
            tryVerify(() => h.findLabel("scdr read its files again: 13 profiles, 11 rules.") !== null);
        }

        function test_check_that_fails_shows_why() {
            load({ commands: { "check": { code: 1, error: "exit 1", output: { ok: false, error: "/etc/scdr/rules.d/x.toml: unknown field" } } } });
            h.tab("Tools");
            h.findNamed("check").clicked();
            tryVerify(() => h.findLabel("/etc/scdr/rules.d/x.toml: unknown field") !== null);
        }

        function test_benchmark_streams_progress() {
            load();
            h.tab("Tools");
            const run = h.findNamed("bench-run");
            tryVerify(() => run.enabled);
            verify(h.findLabel("It takes about 40 seconds and runs every CPU flat out, so the screen may stutter.") !== null);
            run.clicked();
            tryVerify(() => h.fake.calls.some(c => c[0] === "stream" && c[1] === "bench"));
            tryVerify(() => h.findLabel("Done.") !== null);
            verify(h.findLabel("baseline") !== null);
            verify(h.findLabel("median 80 µs · 99% 900 µs · worst 5000 µs") !== null);
        }

        function test_benchmark_cannot_run_while_scdrd_does() {
            load({ commands: { "bench-check": { code: 0, output: { ok: true, can_run: false, reason: "scdrd is running and would change the numbers.", seconds: 40 } } } });
            h.tab("Tools");
            tryVerify(() => h.findLabel("scdrd is running and would change the numbers.") !== null);
            verify(!h.findNamed("bench-run").enabled);
        }

        function test_benchmark_declined_changes_nothing() {
            load();
            h.tab("Tools");
            tryVerify(() => h.findNamed("bench-run").enabled);
            h.fake.confirms = false;
            h.findNamed("bench-run").clicked();
            wait(100);
            verify(h.findLabel("Done.") === null);
            verify(h.findLabel("Cancelled") === null);
        }

        function test_import_previews_and_shows_the_command() {
            load();
            h.tab("Tools");
            const path = h.findNamed("import-path");
            path.text = "rules.d";
            h.findNamed("import-preview").clicked();
            tryVerify(() => h.findLabel("Use a full path starting with /.") !== null);
            compare(runs("preview-import-ananicy").length, 0);
            path.text = "/etc/ananicy.d";
            h.findNamed("import-preview").clicked();
            tryVerify(() => runs("preview-import-ananicy").length === 1);
            compare(runs("preview-import-ananicy")[0][2], ["/etc/ananicy.d"]);
            tryVerify(() => h.findLabel("1 profile, 1 rule") !== null);
            verify(h.findLabel("Left out") !== null);
            verify(h.findLabel("1 rule: 1 name pattern (can be undone)") !== null);
            tryVerify(() => h.findNamed("import-command").text === "sudo scdrctl import ananicy /etc/ananicy.d --output /etc/scdr/rules.d/50-ananicy.toml");
        }

        function test_import_that_would_refuse_shows_the_reason() {
            load({ commands: { "preview-import-ananicy": { code: 0, output: h.plan({ summary: "1 profile", changes: [], would_refuse: true, reason: "/etc/scdr/rules.d/50-ananicy.toml already exists." }) } } });
            h.tab("Tools");
            h.findNamed("import-path").text = "/etc/ananicy.d";
            h.findNamed("import-preview").clicked();
            tryVerify(() => h.findLabel("/etc/scdr/rules.d/50-ananicy.toml already exists.") !== null);
            verify(h.findNamed("import-command").visible === false);
        }

        function test_installer_commands_have_previews() {
            load();
            tryVerify(() => h.findNamed("install") !== null);
            h.findNamed("install").clicked();
            tryVerify(() => runs("installer-install").length === 1);
        }
    }
}
