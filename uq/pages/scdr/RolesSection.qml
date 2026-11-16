// The boost switches: the app you use, audio, games and (under CPU pressure) the background.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "words.js" as Words

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
                label: Words.roleLabel(roleRow.modelData.role)
                value: roleRow.modelData.enabled
                enabled: host.busy === ""
                Layout.fillWidth: true
                onChosen: on => host.write("role", "SetRoleEnabled", [roleRow.modelData.role, on])
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
}
