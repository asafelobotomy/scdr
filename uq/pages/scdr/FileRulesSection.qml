// The rules from scdr's own files (shipped and /etc), for reading. They come after the
// person's own rules.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "rules.js" as Rules

ColumnLayout {
    id: section

    required property var host
    property bool open: false
    readonly property var list: host.settings ? Rules.baseRules(host.settings.base) : ({ rows: [], total: 0 })

    visible: host.ready && host.settings !== null
    Layout.fillWidth: true
    spacing: 4

    SettingRow {
        settingId: "file-rules"
        label: "Rules from scdr's files"
        highlight: section.host.highlight
        Layout.fillWidth: true
        QQC2.Button {
            objectName: "file-rules-toggle"
            text: section.open ? "Hide" : "Show"
            enabled: section.list.total > 0
            onClicked: section.open = !section.open
        }
    }
    UqLabel {
        text: section.list.total === 0 ? "scdr's files have no rules." : section.list.total + (section.list.total === 1 ? " rule" : " rules") + ", tried after yours. Change them by adding your own rule for the same app."
        color: section.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    Repeater {
        model: section.open ? section.list.rows : []
        delegate: ColumnLayout {
            id: fileRule
            required property var modelData
            Layout.fillWidth: true
            spacing: 0
            UqLabel {
                text: fileRule.modelData.profile + " — " + fileRule.modelData.summary
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            UqLabel {
                visible: fileRule.modelData.source !== ""
                text: fileRule.modelData.source
                color: section.host.mutedColor
                elide: Text.ElideMiddle
                Layout.fillWidth: true
            }
        }
    }
    UqLabel {
        visible: section.open && section.list.total > section.list.rows.length
        text: "And " + (section.list.total - section.list.rows.length) + " more. See them all with scdrctl list."
        color: section.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
