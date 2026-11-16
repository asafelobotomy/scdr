// Preferences: the settings that have no tab of their own, read from `scdrctl config show` (and
// followed live from the settings files), each with its own refusal. Then where the files are and
// "Reset my changes", previewed by `scdrctl config reset --dry-run --json`.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "plans.js" as Plans

ColumnLayout {
    id: section

    required property var host
    readonly property var view: host.config
    // The keys of each group: the rows are only rebuilt when this list changes, not on each read.
    property var keys: ({})
    onViewChanged: {
        const next = {};
        cards.forEach(c => next[c.group] = Plans.rowsIn(view, [c.group]).map(r => r.key));
        if (JSON.stringify(next) !== JSON.stringify(keys))
            keys = next;
    }
    readonly property var cards: [
        { group: "System", icon: "preferences-system", id: "system", help: "How often scdr looks again, and two kernel switches." },
        { group: "Agent tools", icon: "utilities-terminal", id: "split", help: "Give the helper processes of agent apps a group of their own, so the app you use and its tools can be treated apart." },
        { group: "Left alone", icon: "dialog-cancel", id: "exceptions", help: "Programs scdr never changes." }
    ]

    spacing: 12
    Layout.fillWidth: true

    Repeater {
        model: section.cards

        SettingsCard {
            id: card

            required property var modelData

            title: card.modelData.group
            iconName: card.modelData.icon
            help: card.modelData.help
            visible: section.view.ready && (section.keys[card.modelData.group] || []).length > 0
            Layout.fillWidth: true

            Repeater {
                model: section.keys[card.modelData.group] || []

                PrefRow {
                    required property var modelData
                    required property int index

                    host: section.host
                    rowKey: modelData
                    settingId: index === 0 ? card.modelData.id : ""
                    Layout.fillWidth: true
                }
            }
        }
    }
    SettingsCard {
        title: "Settings files"
        iconName: "document-properties"
        help: "What you change here is saved in one file. Delete it, or reset below, and scdr's own files apply again."
        Layout.fillWidth: true

        SettingRow {
            settingId: "config-files"
            label: section.view.ready || section.view.file !== "" ? "Your settings are in" : "Reading the settings files…"
            highlight: section.host.highlight
            Layout.fillWidth: true
        }
        QQC2.ProgressBar {
            visible: !section.view.ready && section.view.error === ""
            indeterminate: true
            Layout.fillWidth: true
        }
        UqLabel {
            visible: section.view.file !== ""
            text: section.view.file
            wrapMode: Text.WrapAnywhere
            Layout.fillWidth: true
        }
        UqLabel {
            visible: !section.view.ready && section.view.error !== ""
            text: section.view.error
            color: section.host.errorColor
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        SettingRow {
            settingId: "reset"
            label: section.view.layer.empty ? "You haven't changed anything here" : "Settings, rules and profiles you made here"
            highlight: section.host.highlight
            Layout.fillWidth: true
            QQC2.Button {
                objectName: "reset-mine"
                text: "Reset my changes"
                enabled: section.view.ready && !section.view.layer.empty && section.host.workingKey === ""
                onClicked: section.host.resetMine()
            }
        }
        UqLabel {
            visible: section.host.prefNotes["reset"] !== undefined
            text: section.host.prefNotes["reset"] || ""
            color: section.host.errorColor
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
    }
}
