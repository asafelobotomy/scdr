// The Scheduling page for uQ Settings (uq_api = 3), laid out like Settings' own pages: six tabs of
// cards. Overview (state, CPU pressure, boosted apps, what scdr sees), Roles, Scheduler, Rules
// (yours, profiles, files, Explain), Tools (check, reload, benchmark, import) and Preferences
// (every other setting, followed live from scdr's files). Reads follow scdr's State (watched);
// profile, rule and scheduler changes are applied at once over D-Bus, with long deadlines since
// they may wait for a password; everything else is a command of `scdrctl` (page.toml) that uQ
// previews and confirms. Pure logic is in the .js files; every piece of text goes through UqLabel.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "logic.js" as Logic
import "plans.js" as Plans
import "rules.js" as Rules
import "words.js" as Words

SettingsPage {
    id: page

    pageId: "scdr"

    property var ctx
    property var theme
    /// The [[setting]] row Settings search jumped to; the rows flash, and their tab opens.
    property string highlight

    // What was last read.
    property var view: ({ status: "loading" })
    property var settings: null
    property string settingsError: ""
    property bool settingsDown: false
    property bool agentKnown: false
    property bool agentOn: false
    // What is in flight ("" when nothing is) and why the last change failed, by area.
    property string busy: ""
    property var problems: ({})
    property bool showAdvanced: false
    property var profileNames: []
    // Goes up with every read of the settings, so controls that snapped back show them again.
    property int revision: 0
    // The settings files as `scdrctl config show` reads them (Plans.configView), the facts from
    // the daemon's Status, and the key of the preference whose change is running.
    property var config: Plans.configView(null)
    property int configRevision: 0
    property var facts: []
    property string workingKey: ""
    property var prefNotes: ({})
    // Callbacks that arrive after the page is gone do nothing.
    property var life: ({ alive: true })
    Component.onDestruction: life.alive = false

    readonly property alias confirmDialog: confirm
    readonly property bool ready: view.status === "ready" && !settingsDown
    readonly property bool down: view.status === "not-running" || settingsDown
    readonly property string mutedColor: colorOf("text_muted", "#707070")
    readonly property string errorColor: colorOf("error", "#c00000")
    readonly property string advancedText: [view.detail, settings ? settings.loaderError : "", settingsDown ? "" : settingsError]
        .filter(s => s).join("\n")
    function live(fn) {
        const life = page.life;
        return result => {
            if (life.alive)
                fn(result);
        };
    }

    function colorOf(key, fallback) {
        const c = page.ctx && page.ctx.theme && page.ctx.theme.colors;
        return (c && c[key]) || fallback;
    }

    function problem(area) {
        return page.problems[area] || "";
    }

    function setProblem(area, text) {
        const copy = {};
        for (const k in page.problems)
            copy[k] = page.problems[k];
        copy[area] = text;
        page.problems = copy;
    }

    function working(area) {
        return page.busy.indexOf(area) === 0;
    }

    // ---- Reads ----

    // A watch and a write's re-read can overlap; the one that lost has nothing new to say.
    function skippable(result) {
        return result && result.error && /already running/i.test(String(result.error));
    }

    function onState(result) {
        if (page.skippable(result))
            return;
        page.view = Logic.readState(result);
        // scdr may have been started from elsewhere: read its settings again.
        if (page.view.status === "ready" && (page.settingsDown || page.settings === null))
            page.ctx.dbus.call("Settings", [], page.live(page.onSettings));
        if (page.view.status === "ready" && page.facts.length === 0)
            page.readStatus();
    }

    function onAgent(result) {
        if (page.skippable(result))
            return;
        const on = Logic.agentEnabled(result);
        page.agentKnown = on !== null;
        page.agentOn = on === true;
    }

    function onSettings(result) {
        if (result.error) {
            page.settingsDown = Logic.notRunning(result.error);
            page.settingsError = String(result.error).slice(0, 400);
            return;
        }
        const s = Logic.parseSettings(result.value);
        page.settingsDown = false;
        page.settingsError = s.error || "";
        if (s.error)
            return;
        page.settings = s;
        page.revision++;
        if (JSON.stringify(s.profiles) !== JSON.stringify(page.profileNames))
            page.profileNames = s.profiles;
    }

    function refresh() {
        if (!page.ctx)
            return;
        page.ctx.command.run("state", [], page.live(page.onState));
        page.ctx.command.run("agent-state", [], page.live(page.onAgent));
        page.ctx.dbus.call("Settings", [], page.live(page.onSettings));
        page.readStatus();
    }

    function onConfig(result) {
        if (page.skippable(result))
            return;
        page.config = Plans.configView(result);
        page.configRevision++;
    }

    // What the daemon says about the system, for the Overview.
    function readStatus() {
        if (page.ctx)
            page.ctx.dbus.call("Status", [], page.live(result => {
                page.facts = Plans.statusFacts(result);
            }));
    }

    onCtxChanged: {
        if (!page.ctx)
            return;
        page.ctx.command.watch("state", page.live(page.onState));
        page.ctx.command.watch("agent-state", page.live(page.onAgent));
        page.ctx.command.watch("config", page.live(page.onConfig));
        page.ctx.dbus.call("Settings", [], page.live(page.onSettings));
        page.readStatus();
    }

    // ---- Changes ----

    function finish(area, result) {
        page.busy = "";
        if (result && result.cancelled)
            return false;
        if (result && result.error) {
            page.setProblem(area, Logic.writeError(result));
            return false;
        }
        return true;
    }

    // One of scdrctl's commands (confirmed and previewed by uQ), then a fresh read of everything.
    function command(area, id, args, after) {
        page.busy = area;
        page.setProblem(area, "");
        page.ctx.command.run(id, args, page.live(result => {
            const ok = page.finish(area, result);
            if (ok && after)
                after(result);
            page.refresh();
        }));
    }

    // One D-Bus write that reports its answer to `after`, then a fresh read of everything.
    function dbusWrite(area, method, args, after) {
        page.busy = area;
        page.setProblem(area, "");
        page.ctx.dbus.call(method, args, page.live(result => {
            if (page.finish(area, result) && after)
                after(result);
            page.refresh();
        }));
    }

    // One setting of the Preferences tab: `arg` is the new value as `config set` takes it, or
    // undefined to drop the value made here. scdr's refusal is shown under the row.
    function setPref(row, arg) {
        if (page.workingKey !== "" || !page.ctx)
            return;
        const key = row.key;
        let id = "config-unset";
        let args = [key];
        if (arg !== undefined) {
            const text = Plans.setArg(row, arg);
            const why = Plans.setProblem(key, text);
            if (why !== "") {
                page.notePref(key, why);
                return;
            }
            id = "config-set";
            args = [key, text];
        }
        page.notePref(key, "");
        page.workingKey = key;
        page.ctx.command.run(id, args, page.live(result => {
            page.workingKey = "";
            if (Plans.isCancelled(result))
                return;
            const env = Plans.envelope(result);
            if (!env.ok)
                page.notePref(key, env.error);
            page.ctx.command.run("config", [], page.live(page.onConfig));
            page.refresh();
        }));
    }

    function notePref(key, text) {
        const copy = {};
        for (const k in page.prefNotes)
            copy[k] = page.prefNotes[k];
        if (text === "")
            delete copy[key];
        else
            copy[key] = text;
        page.prefNotes = copy;
    }

    // Drops every setting, rule and profile made here; uQ shows what goes (a preview) first.
    function resetMine() {
        if (page.workingKey !== "" || !page.ctx)
            return;
        page.notePref("reset", "");
        page.workingKey = "reset";
        page.ctx.command.run("config-reset", [], page.live(result => {
            page.workingKey = "";
            if (Plans.isCancelled(result))
                return;
            const env = Plans.envelope(result);
            if (!env.ok)
                page.notePref("reset", env.error);
            page.ctx.command.run("config", [], page.live(page.onConfig));
            page.refresh();
        }));
    }

    // One D-Bus write, then a fresh read of everything.
    function write(area, method, args) {
        page.busy = area;
        page.setProblem(area, "");
        page.ctx.dbus.call(method, args, page.live(result => {
            page.finish(area, result);
            page.refresh();
        }));
    }

    // Reads Settings again, lets `change(layer, settings)` change a copy of the layer, and sends
    // it back; `after` runs once it is saved.
    function edit(area, change, after) {
        page.busy = area;
        page.setProblem(area, "");
        const stop = text => {
            page.busy = "";
            page.setProblem(area, text);
            page.refresh();
        };
        page.ctx.dbus.call("Settings", [], page.live(read => {
            if (read.error)
                return stop(Logic.writeError(read));
            const edited = Logic.applyEdit(read.value, change);
            if (edited.error)
                return stop(edited.error);
            page.ctx.dbus.call("SetSettings", [edited.doc], page.live(result => {
                if (page.finish(area, result) && after)
                    after();
                page.refresh();
            }));
        }));
    }

    // `rule` is an entry of settings.layerRules.
    function removeRule(rule) {
        const index = rule.index;
        const key = rule.key;
        confirm.ask(Words.confirmText("rule-remove", rule.name), () => page.edit("rules", layer => Rules.removeRule(layer, index, key)), Plans.removeRulePlan(rule));
    }

    function setAgent(on) {
        page.busy = "agent";
        page.setProblem("agent", "");
        page.ctx.command.run(on ? "agent-on" : "agent-off", [], page.live(result => {
            page.finish("agent", result);
            page.ctx.command.run("agent-state", [], page.live(page.onAgent));
        }));
    }

    function start() {
        page.busy = "start";
        page.setProblem("start", "");
        page.ctx.command.run("start", [], page.live(result => {
            page.finish("start", result);
            page.refresh();
        }));
    }

    banners: [
        Banner {
            objectName: "banner-down"
            visible: page.down
            text: "scdr isn't running. It changes how much CPU time each app gets; start it to use most of this page."
            buttonText: "Start scdr"
            buttonEnabled: page.busy === ""
            tone: page.errorColor
            onClicked: page.start()
        },
        Banner {
            objectName: "banner-config"
            visible: !page.config.ready && page.config.error !== "" && !page.down
            text: "scdr's files don't load together: " + page.config.error
            tone: page.errorColor
        },
        Banner {
            objectName: "banner-trial"
            visible: page.settings !== null && page.settings.trial !== null
            text: Words.trialText(page.settings ? page.settings.trial : null)
            buttonText: "Stop trying"
            buttonEnabled: page.busy === ""
            tone: page.mutedColor
            onClicked: page.write("trial", "EndSchedulerTrial", [])
        }
    ]

    ConfirmDialog {
        id: confirm
        mutedColor: page.mutedColor
        errorColor: page.errorColor
    }

    OverviewTab {
        host: page
    }
    RolesTab {
        host: page
    }
    SchedulerTab {
        host: page
    }
    RulesTab {
        host: page
    }
    ToolsTab {
        host: page
    }

    SettingsTab {
        title: "Preferences"
        iconName: "configure"

        PrefsSection {
            host: page
            Layout.fillWidth: true
        }
    }
}
