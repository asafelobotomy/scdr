// The apps scdr gives a boost right now, with their icons, the profile and why.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "words.js" as Words

ColumnLayout {
    id: section

    required property var host
    readonly property var boosted: host.ready ? host.view.state.boosted : []
    readonly property int shown: 12

    Layout.fillWidth: true
    spacing: 6

    SettingRow {
        settingId: "boosted"
        label: !section.host.ready ? "Boosted apps" : (section.boosted.length === 0 ? "Nothing is boosted right now" : Words.boostedCount(section.boosted.length))
        highlight: section.host.highlight
        Layout.fillWidth: true
    }
    QQC2.ProgressBar {
        visible: section.host.view.status === "loading"
        indeterminate: true
        Layout.fillWidth: true
    }
    Repeater {
        model: section.boosted.slice(0, section.shown)

        RowLayout {
            id: line

            required property var modelData

            spacing: 8
            Layout.fillWidth: true

            UqAppIcon {
                name: line.modelData.app !== "" ? line.modelData.app : line.modelData.name
                label: line.modelData.name
                size: 28
            }
            ColumnLayout {
                spacing: 0
                Layout.fillWidth: true
                UqLabel {
                    text: line.modelData.name
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
                UqLabel {
                    text: Words.boostedDetail(line.modelData)
                    color: section.host.mutedColor
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
            }
        }
    }
    UqLabel {
        visible: section.boosted.length > section.shown
        text: "And " + (section.boosted.length - section.shown) + " more."
        color: section.host.mutedColor
    }
}
