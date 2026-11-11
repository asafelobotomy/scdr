// The boost switches and the list of apps boosted right now.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "logic.js" as Logic

ColumnLayout {
    required property var host
    visible: host.ready
    Layout.fillWidth: true
    spacing: 6

    SettingRow {
        settingId: "roles"
        label: "Boost the app you use, audio and games"
        highlight: host.highlight
        Layout.fillWidth: true
        UqLabel {
            visible: host.working("role")
            text: "Working…"
            color: host.mutedColor
        }
    }
    Repeater {
        model: host.ready ? host.view.state.roles : []
        delegate: ColumnLayout {
            id: roleRow
            required property var modelData
            Layout.fillWidth: true
            spacing: 0
            SwitchRow {
                label: Logic.roleLabel(roleRow.modelData.role)
                value: roleRow.modelData.enabled
                enabled: host.busy === ""
                Layout.fillWidth: true
                onChosen: on => host.write("role:" + roleRow.modelData.role, "SetRoleEnabled", [roleRow.modelData.role, on])
            }
            UqLabel {
                text: "Uses the " + roleRow.modelData.profile + " profile"
                color: host.mutedColor
            }
        }
    }
    UqLabel {
        visible: host.problem("role") !== ""
        text: host.problem("role")
        color: host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    UqLabel {
        text: "Boosted now"
        font.bold: true
        Layout.topMargin: 8
    }
    UqLabel {
        visible: host.ready && host.view.state.boosted.length === 0
        text: "Nothing is boosted right now."
        color: host.mutedColor
    }
    Repeater {
        model: host.ready ? host.view.state.boosted.slice(0, 20) : []
        delegate: UqLabel {
            required property var modelData
            text: Logic.boostedLine(modelData)
            elide: Text.ElideRight
            Layout.fillWidth: true
        }
    }
}
