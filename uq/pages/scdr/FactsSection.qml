// What scdr knows about this system: version, uptime, power profile, governor, disks and pressure.
// Read from the daemon's Status; the Refresh button reads it again.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings

ColumnLayout {
    id: section

    required property var host

    Layout.fillWidth: true
    spacing: 4

    SettingRow {
        settingId: "system-facts"
        label: section.host.facts.length > 0 ? "What scdr sees" : (section.host.down ? "Nothing to show while scdr is off" : "Reading…")
        highlight: section.host.highlight
        Layout.fillWidth: true
        QQC2.Button {
            text: "Refresh"
            enabled: section.host.ready && section.host.busy === ""
            onClicked: section.host.readStatus()
        }
    }
    QQC2.ProgressBar {
        visible: section.host.facts.length === 0 && !section.host.down
        indeterminate: true
        Layout.fillWidth: true
    }
    Repeater {
        model: section.host.facts

        RowLayout {
            id: fact

            required property var modelData

            Layout.fillWidth: true
            UqLabel {
                text: fact.modelData.label
                color: section.host.mutedColor
                Layout.preferredWidth: 160
                elide: Text.ElideRight
            }
            UqLabel {
                text: fact.modelData.text
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }
    }
}
