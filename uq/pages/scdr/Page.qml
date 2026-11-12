// The Scheduling page for uQ Settings: scdr's state, its boosts and the profile each role gives,
// the focus agent, the CPU scheduler (with trials and the installer), the kernel scheduler's
// knobs, CPU pressure, profiles, and rules. Same settings as the Plasma widget's pages. Reads
// follow scdr's State (watched); every change is applied at once over D-Bus, with long deadlines
// since it may wait for a password. Pure logic is in the .js files; every piece of text goes
// through UqLabel.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "logic.js" as Logic
import "rules.js" as Rules
import "words.js" as Words

Item {
    id: page

    property var ctx
    /// The search row to scroll to (from [[setting]] ids).
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
    property string ruleCheck: ""
    // Callbacks that arrive after the page is gone do nothing.
    property var life: ({ alive: true })
    Component.onDestruction: life.alive = false

    readonly property alias confirmDialog: confirm
    readonly property alias flick: scroller
    readonly property bool ready: view.status === "ready" && !settingsDown
    readonly property bool down: view.status === "not-running" || settingsDown
    readonly property string mutedColor: colorOf("text_muted", "#707070")
    readonly property string errorColor: colorOf("error", "#c00000")
    readonly property string advancedText: [view.detail, settings ? settings.loaderError : "", settingsDown ? "" : settingsError]
        .filter(s => s).join("\n")
    readonly property var sections: ({ "status": statusSection, "roles": rolesSection, "follow-focus": followSection,
                                        "role-profiles": roleProfilesSection, "scheduler": schedulerSection,
                                        "available-schedulers": availableSection, "kernel-tuning": kernelSection,
                                        "pressure": pressureSection, "system": pressureSection.systemRow,
                                        "profiles": profilesSection, "rules": rulesSection, "file-rules": fileRulesSection })

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
        if (JSON.stringify(s.profiles) !== JSON.stringify(page.profileNames)) {
            page.profileNames = s.profiles;
            rulesSection.profile.currentIndex = Math.max(0, s.profiles.indexOf("desktop"));
        }
        if (page.highlight !== "")
            Qt.callLater(page.scrollTo, page.highlight);
    }

    function refresh() {
        if (!page.ctx)
            return;
        page.ctx.command.run("state", [], page.live(page.onState));
        page.ctx.command.run("agent-state", [], page.live(page.onAgent));
        page.ctx.dbus.call("Settings", [], page.live(page.onSettings));
    }

    onCtxChanged: {
        if (!page.ctx)
            return;
        page.ctx.command.watch("state", page.live(page.onState));
        page.ctx.command.watch("agent-state", page.live(page.onAgent));
        page.ctx.dbus.call("Settings", [], page.live(page.onSettings));
        availableSection.findInstaller();
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

    function addRule() {
        const profile = page.profileNames[rulesSection.profile.currentIndex] || "";
        page.ruleCheck = Logic.checkRuleName(rulesSection.field.text);
        if (page.ruleCheck === "" && Logic.checkProfile(profile) !== "")
            page.ruleCheck = Logic.checkProfile(profile);
        if (page.ruleCheck !== "")
            return;
        const name = rulesSection.field.text;
        page.edit("rules", layer => Logic.addRule(layer, profile, name), () => {
            rulesSection.field.text = "";
            page.ruleCheck = "";
        });
    }

    // `rule` is an entry of settings.layerRules.
    function removeRule(rule) {
        const index = rule.index;
        const key = rule.key;
        confirm.ask(Words.confirmText("rule-remove", rule.name), () => page.edit("rules", layer => Rules.removeRule(layer, index, key)));
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

    function scrollTo(id) {
        const item = page.sections[id];
        if (!item || !item.visible)
            return;
        const y = item.mapToItem(scroller.contentItem, 0, 0).y;
        scroller.contentY = Math.max(0, Math.min(y, scroller.contentHeight - scroller.height));
    }

    onHighlightChanged: Qt.callLater(page.scrollTo, page.highlight)

    ConfirmDialog {
        id: confirm
    }

    Flickable {
        id: scroller
        anchors.fill: parent
        contentWidth: width
        contentHeight: column.implicitHeight + 32
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        QQC2.ScrollBar.vertical: QQC2.ScrollBar {}

        ColumnLayout {
            id: column
            x: 16
            y: 16
            width: scroller.width - 32
            spacing: 20

            StatusSection {
                id: statusSection
                host: page
            }
            RolesSection {
                id: rolesSection
                host: page
            }
            FollowSection {
                id: followSection
                host: page
            }
            RoleProfilesSection {
                id: roleProfilesSection
                host: page
            }
            SchedulerSection {
                id: schedulerSection
                host: page
            }
            AvailableSection {
                id: availableSection
                host: page
            }
            KernelSection {
                id: kernelSection
                host: page
            }
            PressureSection {
                id: pressureSection
                host: page
            }
            ProfilesSection {
                id: profilesSection
                host: page
            }
            RulesSection {
                id: rulesSection
                host: page
            }
            FileRulesSection {
                id: fileRulesSection
                host: page
            }
            // ---- Advanced details ----
            ColumnLayout {
                visible: page.advancedText !== ""
                Layout.fillWidth: true
                QQC2.Button {
                    flat: true
                    text: "Advanced details"
                    onClicked: page.showAdvanced = !page.showAdvanced
                }
                UqLabel {
                    visible: page.showAdvanced
                    text: page.advancedText
                    color: page.mutedColor
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }
            }
        }
    }
}
