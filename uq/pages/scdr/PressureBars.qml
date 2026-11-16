// How busy the CPU is, as a bar with the two thresholds: above the first, apps that aren't
// focused are lowered; below the second they go back. Followed from scdr's State.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "logic.js" as Logic

ColumnLayout {
    id: section

    required property var host
    readonly property var s: host.settings
    readonly property var state: host.ready ? host.view.state : null
    readonly property var pressure: section.state !== null ? section.state.cpuPressure : null
    readonly property real high: section.s !== null ? Logic.lookup(section.s.layer, ["pressure", "high"]) ?? Logic.lookup(section.s.base, ["pressure", "high"]) ?? 20 : 20
    readonly property real low: section.s !== null ? Logic.lookup(section.s.layer, ["pressure", "low"]) ?? Logic.lookup(section.s.base, ["pressure", "low"]) ?? 5 : 5

    Layout.fillWidth: true
    spacing: 6

    SettingRow {
        settingId: "overview-pressure"
        label: section.pressure !== null ? "CPU busy: " + Math.round(section.pressure) + " %" : (section.host.down ? "CPU pressure isn't read while scdr is off" : "Reading CPU pressure…")
        highlight: section.host.highlight
        Layout.fillWidth: true
    }
    QQC2.ProgressBar {
        objectName: "pressure-bar"
        visible: section.pressure !== null || !section.host.down
        indeterminate: section.pressure === null
        from: 0
        to: 100
        value: section.pressure !== null ? section.pressure : 0
        Layout.fillWidth: true
    }
    UqLabel {
        visible: section.state !== null
        text: section.state !== null && section.state.pressureActive
            ? "Apps that aren't focused are lowered now. They go back below " + Math.round(section.low) + " %."
            : "Apps that aren't focused are left alone. They are lowered above " + Math.round(section.high) + " %."
        color: section.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
