// SCDR panel widget: status and quick controls for scdrd, over the system
// bus. Reads the daemon's State property (updated through PropertiesChanged,
// no polling) and calls Pause/Resume/SetRoleEnabled, which polkit guards.
// The Configure dialog (contents/config) edits scdrd's settings layer.

import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.components as PlasmaComponents3
import org.kde.plasma.extras as PlasmaExtras
import org.kde.kirigami as Kirigami
import org.kde.plasma.workspace.dbus as DBus

PlasmoidItem {
    id: root

    readonly property string busName: "org.scdr.Daemon1"
    readonly property string objectPath: "/org/scdr/Daemon1"
    readonly property bool running: watcher.registered
    readonly property var scdr: running && propsLoader.item ? parse(propsLoader.item.properties.State) : null
    readonly property bool paused: scdr !== null && scdr.paused
    property string lastError: ""

    function parse(json) {
        if (!json) {
            return null;
        }
        try {
            return JSON.parse(json);
        } catch (e) {
            return null;
        }
    }

    function call(member, args) {
        lastError = "";
        const msg = { service: busName, path: objectPath, iface: busName, member: member, arguments: args || [] };
        const reply = DBus.SystemBus.asyncCall(msg);
        reply.finished.connect(() => {
            if (reply.isError) {
                root.lastError = reply.error.message;
            }
        });
    }

    function roleLabel(role) {
        return { game: i18n("Games"), audio: i18n("Audio"), foreground: i18n("Focused app"),
                 background: i18n("Background demotion") }[role] || role;
    }

    function roleIcon(role) {
        return { game: "input-gaming", audio: "audio-volume-high", foreground: "window",
                 background: "go-down" }[role] || "preferences-system-performance";
    }

    // One row per app (its app.slice unit), or per process name outside apps.
    function grouped(boosted) {
        const rows = {};
        for (const p of boosted) {
            const name = p.app || p.name;
            const key = name + "\u0000" + p.profile;
            if (!rows[key]) {
                rows[key] = { name: name, profile: p.profile, role: p.role, why: p.why, pids: [] };
            }
            rows[key].pids.push(p.pid);
        }
        return Object.values(rows).sort((a, b) => a.name.localeCompare(b.name));
    }

    function summary() {
        if (!running) {
            return i18n("scdrd isn't running");
        }
        if (!scdr) {
            return i18n("Connecting…");
        }
        if (paused) {
            return i18n("Paused");
        }
        const apps = grouped(scdr.boosted).length;
        return i18np("%2 processes managed, 1 app boosted", "%2 processes managed, %1 apps boosted",
                     apps, scdr.managed);
    }

    Plasmoid.icon: paused || !running ? "media-playback-paused" : "preferences-system-performance"
    Plasmoid.status: running && scdr && (scdr.boosted.length > 0 || scdr.pressure_active)
                     ? PlasmaCore.Types.ActiveStatus : PlasmaCore.Types.PassiveStatus
    toolTipMainText: i18n("SCDR")
    toolTipSubText: summary()

    DBus.DBusServiceWatcher {
        id: watcher
        busType: DBus.BusType.System
        watchedService: root.busName
    }

    // Recreated whenever scdrd (re)appears, so property tracking follows it.
    Loader {
        id: propsLoader
        active: root.running
        sourceComponent: DBus.Properties {
            busType: DBus.BusType.System
            service: root.busName
            path: root.objectPath
            iface: root.busName
        }
    }

    fullRepresentation: PlasmaExtras.Representation {
        Layout.minimumWidth: Kirigami.Units.gridUnit * 18
        Layout.minimumHeight: Kirigami.Units.gridUnit * 16
        collapseMarginsHint: true

        header: PlasmaExtras.PlasmoidHeading {
            RowLayout {
                anchors.fill: parent
                Kirigami.Heading {
                    Layout.fillWidth: true
                    level: 3
                    text: i18n("SCDR")
                }
                PlasmaComponents3.Label {
                    text: !root.running ? i18n("Stopped") : root.paused ? i18n("Paused") : i18n("Running")
                    opacity: 0.7
                }
                PlasmaComponents3.ToolButton {
                    visible: root.running && root.scdr !== null
                    icon.name: root.paused ? "media-playback-start" : "media-playback-pause"
                    text: root.paused ? i18n("Resume") : i18n("Pause")
                    display: QQC2.AbstractButton.IconOnly
                    onClicked: root.call(root.paused ? "Resume" : "Pause")
                    PlasmaComponents3.ToolTip.text: root.paused
                        ? i18n("Resume: apply priorities again")
                        : i18n("Pause: restore every priority, weight and scheduler scdr changed")
                    PlasmaComponents3.ToolTip.visible: hovered
                }
                PlasmaComponents3.ToolButton {
                    icon.name: "configure"
                    text: i18n("Settings…")
                    display: QQC2.AbstractButton.IconOnly
                    onClicked: Plasmoid.internalAction("configure").trigger()
                    PlasmaComponents3.ToolTip.text: i18n("Schedulers, pressure, roles, profiles and rules")
                    PlasmaComponents3.ToolTip.visible: hovered
                }
            }
        }

        PlasmaExtras.PlaceholderMessage {
            anchors.centerIn: parent
            width: parent.width - Kirigami.Units.gridUnit * 2
            visible: !root.running
            iconName: "preferences-system-performance"
            text: i18n("scdrd isn't running")
            explanation: i18n("Start it with: systemctl enable --now scdrd")
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: Kirigami.Units.smallSpacing * 2
            visible: root.running && root.scdr !== null
            spacing: Kirigami.Units.smallSpacing

            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: Kirigami.Units.largeSpacing
                PlasmaComponents3.Label { text: i18n("Scheduler:"); opacity: 0.7 }
                PlasmaComponents3.Label {
                    Layout.fillWidth: true
                    elide: Text.ElideRight
                    text: root.scdr && root.scdr.scheduler.length
                          ? root.scdr.scheduler[0].replace(/^sched_ext: /, "") : i18n("kernel default")
                }
                PlasmaComponents3.Label { text: i18n("CPU pressure:"); opacity: 0.7 }
                PlasmaComponents3.Label {
                    text: root.scdr && root.scdr.cpu_pressure !== null
                          ? Math.round(root.scdr.cpu_pressure) + "%"
                            + (root.scdr.pressure_active ? " " + i18n("(demoting background apps)") : "")
                          : "–"
                }
                PlasmaComponents3.Label { text: i18n("Power profile:"); opacity: 0.7 }
                PlasmaComponents3.Label { text: root.scdr && root.scdr.power_profile ? root.scdr.power_profile : "–" }
            }

            Kirigami.Heading { level: 5; text: i18n("Roles"); Layout.topMargin: Kirigami.Units.smallSpacing }
            Repeater {
                model: root.scdr ? root.scdr.roles : []
                delegate: PlasmaComponents3.Switch {
                    required property var modelData
                    Layout.fillWidth: true
                    icon.name: root.roleIcon(modelData[0])
                    text: i18n("%1 (%2)", root.roleLabel(modelData[0]), modelData[1])
                    checked: modelData[2]
                    enabled: !root.paused
                    onToggled: root.call("SetRoleEnabled", [modelData[0], checked])
                }
            }
            PlasmaComponents3.Label {
                Layout.fillWidth: true
                visible: root.scdr !== null && root.scdr.agents === 0
                wrapMode: Text.WordWrap
                opacity: 0.7
                font: Kirigami.Theme.smallFont
                text: i18n("No session agent: focus, audio and game roles need systemctl --user enable --now scdr-session")
            }

            Kirigami.Heading { level: 5; text: i18n("Boosted now"); Layout.topMargin: Kirigami.Units.smallSpacing }
            PlasmaComponents3.ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                ListView {
                    model: root.scdr ? root.grouped(root.scdr.boosted) : []
                    clip: true
                    delegate: PlasmaComponents3.ItemDelegate {
                        required property var modelData
                        width: ListView.view.width
                        icon.name: root.roleIcon(modelData.role)
                        text: modelData.pids.length > 1
                              ? i18n("%1 ×%2 — %3", modelData.name, modelData.pids.length, modelData.profile)
                              : i18n("%1 — %2", modelData.name, modelData.profile)
                        PlasmaComponents3.ToolTip.text: i18n("%1 (pids %2)", modelData.why, modelData.pids.join(", "))
                        PlasmaComponents3.ToolTip.visible: hovered
                    }
                    PlasmaExtras.PlaceholderMessage {
                        anchors.centerIn: parent
                        width: parent.width
                        visible: parent.count === 0
                        text: root.paused ? i18n("Paused") : i18n("Nothing boosted right now")
                    }
                }
            }

            PlasmaComponents3.Label {
                Layout.fillWidth: true
                visible: root.lastError !== ""
                wrapMode: Text.WordWrap
                color: Kirigami.Theme.negativeTextColor
                text: root.lastError
            }
        }
    }
}
