// The CPU scheduler choice.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "logic.js" as Logic

ColumnLayout {
    required property var host
    property alias combo: schedulerCombo
    visible: host.ready && host.settings !== null
    Layout.fillWidth: true
    spacing: 6

    SettingRow {
        settingId: "scheduler"
        label: "CPU scheduler"
        highlight: host.highlight
        Layout.fillWidth: true
        UqLabel {
            visible: host.working("scheduler")
            text: "Working…"
            color: host.mutedColor
        }
        UqComboBox {
            id: schedulerCombo
            objectName: "scheduler-choice"
            model: host.schedulerLabels
            enabled: host.busy === ""
            onActivated: index => host.setScheduler(index)
        }
    }
    UqLabel {
        visible: host.settings !== null && host.settings.currentScheduler !== ""
        text: "Running now: " + (host.settings ? host.settings.currentScheduler : "")
        color: host.mutedColor
    }
    UqLabel {
        visible: host.settings !== null && host.settings.loaderError !== ""
        text: "No extra schedulers are installed."
        color: host.mutedColor
    }
    UqLabel {
        visible: host.problem("scheduler") !== ""
        text: host.problem("scheduler")
        color: host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
