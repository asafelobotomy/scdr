// When apps that aren't focused get the background role (CPU pressure thresholds), and the
// system-wide disk-weight and autogroup switches.
import QtQuick
import QtQuick.Layouts
import org.uq.settings
import "logic.js" as Logic
import "profiles.js" as Profiles

ColumnLayout {
    id: section

    required property var host
    readonly property var s: host.settings
    property alias systemRow: systemRow
    readonly property var thresholds: [
        { key: "high", label: "Turn on above", suffix: " %" },
        { key: "low", label: "Turn off below", suffix: " %" },
        { key: "hold_secs", label: "Hold for at least", suffix: " s" }
    ]

    visible: host.ready && s !== null
    Layout.fillWidth: true
    spacing: 6

    SettingRow {
        settingId: "pressure"
        label: "Background apps when the CPU is busy"
        highlight: section.host.highlight
        Layout.fillWidth: true
    }
    Repeater {
        model: section.thresholds
        delegate: RowLayout {
            id: pressureRow
            required property var modelData
            readonly property var spec: Profiles.PRESSURE[modelData.key]
            readonly property var base: section.s ? Logic.lookup(section.s.base, ["pressure", modelData.key]) : undefined
            Layout.fillWidth: true
            UqLabel {
                text: pressureRow.modelData.label
                Layout.preferredWidth: 180
            }
            OptionalNumber {
                objectName: "pressure-" + pressureRow.modelData.key
                from: pressureRow.spec.from
                to: pressureRow.spec.to
                decimals: pressureRow.spec.decimals
                suffix: pressureRow.modelData.suffix
                value: section.s ? Logic.lookup(section.s.layer, ["pressure", pressureRow.modelData.key]) : undefined
                fallback: typeof pressureRow.base === "number" ? pressureRow.base : pressureRow.spec.fallback
                revision: section.host.revision
                enabled: section.host.busy === ""
                onChosen: v => section.host.edit("pressure", (layer, doc) => Profiles.setPressure(layer, doc, pressureRow.modelData.key, v))
            }
        }
    }
    UqLabel {
        text: "CPU pressure is the share of time tasks wait for a CPU (10-second average). Above the first value, apps that aren't focused get the background role until it drops below the second."
        color: section.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    SettingRow {
        id: systemRow
        settingId: "system"
        label: "System-wide switches"
        highlight: section.host.highlight
        Layout.fillWidth: true
        Layout.topMargin: 8
    }
    Repeater {
        model: [{ key: "iocost", label: "Disk weights (iocost)" }, { key: "autogroup", label: "Autogroup" }]
        delegate: RowLayout {
            id: switchRow
            required property var modelData
            Layout.fillWidth: true
            UqLabel {
                text: switchRow.modelData.label
                Layout.preferredWidth: 180
            }
            ChoiceBox {
                objectName: "system-" + switchRow.modelData.key
                choices: section.s ? Profiles.switchChoices(section.s, switchRow.modelData.key) : []
                current: section.s ? Profiles.switchCurrent(section.s, switchRow.modelData.key) : null
                enabled: section.host.busy === ""
                Layout.fillWidth: true
                onPicked: v => section.host.edit("pressure", layer => Profiles.setSwitch(layer, switchRow.modelData.key, v))
            }
        }
    }
    AreaStatus {
        host: section.host
        areas: ["pressure"]
    }
}
