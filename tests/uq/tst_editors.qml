// The Scheduling page's editors (schedulers, trials, installer, roles, pressure, profiles,
// rules) offscreen, with the stub org.uq.settings module and a fake ctx:
//   QT_QPA_PLATFORM=offscreen qmltestrunner -import tests/uq/stub -input tests/uq
import QtQuick
import QtTest

Item {
    width: 700
    height: 500

    PageHarness {
        id: h
    }

    TestCase {
        name: "SchedulingEditors"
        when: windowShown

        function load(over) {
            h.fake.calls = [];
            h.arrange(over);
            h.loader.source = "";
            h.loader.source = h.pageUrl;
            verify(h.loader.status === Loader.Ready, "the page loads");
            h.loader.item.ctx = h.fake.ctx;
            tryVerify(() => h.findNamed("rule-0") !== null);
        }

        function settingsWith(over) {
            return { dbus: { "Settings": { value: JSON.stringify(h.settingsDoc(null, over)) } } };
        }

        function test_enable_switch() {
            load();
            const row = h.findNamed("scheduler-enable");
            tryVerify(() => row.value === true);
            h.flip(row, false);
            tryVerify(() => h.writes().length === 1);
            compare(h.sentLayer().sched_ext, { enable: false, default: "bpfland", power: { performance: "lavd" } });
        }

        function test_power_and_gaming_slots() {
            load();
            const perf = h.findNamed("scheduler-performance");
            tryCompare(perf, "currentText", "lavd");
            const gaming = h.findNamed("scheduler-gaming");
            tryCompare(gaming, "currentText", "Same as default");
            h.pick(gaming, "devbox");
            tryVerify(() => h.writes().length === 1);
            compare(h.sentLayer().sched_ext.gaming, "devbox");
            h.fake.calls = [];
            h.pick(perf, "Same as default");
            tryVerify(() => h.writes().length === 1);
            compare(h.sentLayer().sched_ext, { enable: true, default: "bpfland" });
        }

        function test_running_devbox_and_loader_error() {
            load(settingsWith({ current_scheduler: "scx_devbox:auto", scx_loader_error: "scx_loader is not running" }));
            tryVerify(() => h.findLabel("Running now: devbox") !== null);
            verify(h.findLabel("No extra schedulers are installed.") === null);
            verify(h.findLabel("Only the schedulers scdr runs itself can be used right now. Install the scheduler tools for the others.") !== null);
        }

        function test_try_and_stop() {
            load();
            const tryLavd = h.findNamed("try-lavd");
            tryVerify(() => tryLavd.enabled);
            verify(!h.findNamed("try-rusty").enabled, "a scheduler that isn't installed can't be tried");
            tryLavd.clicked();
            tryVerify(() => h.writes().length === 1);
            compare(h.writes()[0].slice(0, 3), ["call", "TryScheduler", ["lavd:auto"]]);
            const mode = h.findNamed("mode-bpfland");
            verify(mode.visible);
            verify(!h.findNamed("mode-lavd").visible);
            tryVerify(() => h.findNamed("try-bpfland").enabled);
            mode.currentIndex = 1;
            h.findNamed("try-bpfland").clicked();
            tryVerify(() => h.writes().length === 2);
            compare(h.writes()[1][2], ["bpfland:gaming"]);
        }

        function test_trial_banner() {
            load(settingsWith({ trial: { trying: "scx_devbox:auto", was: "eevdf" } }));
            tryVerify(() => h.findLabel("Trying devbox (was Kernel default). It isn't saved; stop the trial to go back.") !== null);
            h.findNamed("trial-stop").clicked();
            tryVerify(() => h.writes().length === 1);
            compare(h.writes()[0].slice(0, 3), ["call", "EndSchedulerTrial", []]);
        }

        function test_installer() {
            load();
            tryVerify(() => h.findLabel("Installs with Pamac.") !== null);
            verify(h.findNamed("install-git").enabled);
            h.findNamed("install").clicked();
            tryVerify(() => h.fake.calls.some(c => c[0] === "run" && c[1] === "installer-install"));
            tryVerify(() => h.findLabel("Done.") !== null);
        }

        function test_installer_missing_or_failing() {
            load({ commands: { "installer-which": { code: 1, error: "exit 1: Error: no package installer found" },
                               "installer-update": { code: 1, error: "exit 1: Error: pkcon exited with 1" } } });
            tryVerify(() => h.findLabel("No installer found: install scx-scheds and scx-tools with your package manager.") !== null);
            verify(!h.findNamed("install").enabled);
            load({ commands: { "installer-which": { code: 0, output: "packagekit\n" },
                               "installer-update": { code: 1, error: "exit 1: Error: pkcon exited with 1" } } });
            tryVerify(() => h.findNamed("update").enabled);
            verify(!h.findNamed("install-git").enabled, "PackageKit can't build from the AUR");
            h.findNamed("update").clicked();
            tryVerify(() => h.findLabel("exit 1: Error: pkcon exited with 1") !== null);
        }

        function test_role_profiles() {
            load();
            const fg = h.findNamed("role-foreground-default");
            tryCompare(fg, "currentText", "Not set (desktop)");
            const box = h.findNamed("role-game-performance");
            tryCompare(box, "currentText", "Same as default");
            h.pick(box, "background");
            tryVerify(() => h.writes().length === 1);
            compare(h.sentLayer().roles, { power: { performance: { game: "background" } } });
            h.fake.calls = [];
            h.pick(fg, "Off");
            tryVerify(() => h.writes().length === 1);
            compare(h.sentLayer().roles, { foreground: "" });
        }

        function test_pressure_and_system() {
            load();
            const high = h.findNamed("pressure-high");
            tryVerify(() => high.check.enabled);
            high.check.checked = true;
            high.check.toggled();
            tryVerify(() => h.writes().length === 1);
            compare(h.sentLayer().pressure, { high: 20 });
            h.fake.calls = [];
            const low = h.findNamed("pressure-low");
            tryVerify(() => low.check.enabled);
            low.chosen(25);
            tryVerify(() => h.findLabel("“Turn off below” has to be lower than “Turn on above”.") !== null);
            compare(h.writes().length, 0);
            h.pick(h.findNamed("system-autogroup"), "Off: nice values apply across the whole system");
            tryVerify(() => h.writes().length === 1);
            compare(h.sentLayer().system, { autogroup: false });
        }

        function test_kernel_tuning() {
            load();
            h.pick(h.findNamed("preempt-power-saver"), "lazy");
            tryVerify(() => h.writes().length === 1);
            compare(h.sentLayer().eevdf, { power: { "power-saver": { preempt: "lazy" } } });
        }

        function test_profile_edit_copies_shipped() {
            load();
            const pick = h.findNamed("profile-pick");
            tryVerify(() => pick.model.length === 3);
            h.pick(pick, "desktop");
            tryVerify(() => h.findLabel("From /usr/share/scdr/scdr.toml") !== null);
            verify(!h.findNamed("profile-remove").visible);
            h.pick(h.findNamed("profile-io"), "be:2");
            tryVerify(() => h.writes().length === 1);
            compare(h.sentLayer().profile, { desktop: { nice: -2, io: "be:2" } });
        }

        function test_profile_add_and_delete() {
            const doc = h.settingsDoc(null);
            doc.layer.profile = { mine: { nice: 5 } };
            load({ dbus: { "Settings": { value: JSON.stringify(doc) } } });
            const pick = h.findNamed("profile-pick");
            tryVerify(() => pick.model.length === 4);
            h.pick(pick, "game");
            h.findNamed("profile-name").text = "Bad Name";
            tryVerify(() => !h.findNamed("profile-add").enabled);
            h.findNamed("profile-name").text = "quiet";
            tryVerify(() => h.findNamed("profile-add").enabled);
            h.findNamed("profile-add").clicked();
            tryVerify(() => h.writes().length === 1);
            compare(h.sentLayer().profile, { mine: { nice: 5 }, quiet: { inherit: "game" } });
            h.fake.calls = [];
            h.pick(pick, "mine");
            tryCompare(h.findNamed("profile-remove"), "text", "Delete profile");
            h.findNamed("profile-remove").clicked();
            compare(h.page.confirmDialog.message, "Delete the profile mine?");
            h.page.confirmDialog.accept();
            tryVerify(() => h.writes().length === 1);
            compare(h.sentLayer().profile, undefined);
        }

        function test_rule_editor() {
            load();
            verify(!h.findNamed("up-0").enabled);
            verify(!h.findNamed("down-2").enabled);
            h.findNamed("up-1").clicked();
            tryVerify(() => h.writes().length === 1);
            compare(h.sentLayer().rule.map(r => r.profile), ["game", "desktop", "background"]);
            h.fake.calls = [];
            const field = h.findNamed("value-2-exe");
            tryVerify(() => field.enabled);
            compare(field.text, "/x/*");
            field.text = "/opt/x/*, /usr/bin/x";
            field.editingFinished();
            tryVerify(() => h.writes().length === 1);
            compare(h.sentLayer().rule[2], { profile: "background", exe: ["/opt/x/*", "/usr/bin/x"], name: "x" });
        }

        function test_rule_conditions() {
            load();
            const field = h.findNamed("value-0-name");
            tryVerify(() => field.enabled);
            field.text = " , ";
            field.editingFinished();
            tryVerify(() => h.findLabel("Type at least one value, or remove the condition.") !== null);
            compare(h.writes().length, 0);
            verify(!h.findNamed("drop-0-name").enabled, "the only condition stays");
            h.pick(h.findNamed("field-0-name"), "cmdline");
            tryVerify(() => h.writes().length === 1);
            compare(h.sentLayer().rule[0], { profile: "desktop", cmdline: "firefox" });
            h.fake.calls = [];
            h.findNamed("add-condition-1").clicked();
            tryVerify(() => h.writes().length === 1);
            compare(h.sentLayer().rule[1], { profile: "game", name: ["steam*"], exe: "*" });
            h.fake.calls = [];
            h.pick(h.findNamed("rule-profile-1"), "background");
            tryVerify(() => h.writes().length === 1);
            compare(h.sentLayer().rule[1].profile, "background");
        }

        function test_file_rules() {
            load();
            const toggle = h.findNamed("file-rules-toggle");
            verify(h.findLabel("background — name = baloo*") === null);
            toggle.clicked();
            tryVerify(() => h.findLabel("background — name = baloo*") !== null);
            verify(h.findLabel("/usr/share/scdr/scdr.toml") !== null);
        }

        function test_new_sections_scroll() {
            load();
            for (const id of ["profiles", "available-schedulers", "file-rules", "system"]) {
                h.page.highlight = id;
                tryVerify(() => {
                    const y = h.page.sections[id].mapToItem(h.page.flick, 0, 0).y;
                    return y >= -1 && y < h.page.flick.height;
                }, 3000, id + " is in view");
            }
        }
    }
}
