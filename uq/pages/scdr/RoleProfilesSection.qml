// The profile each role gives processes no rule matches, by default and per power profile.
import QtQuick
import QtQuick.Layouts
import org.uq.settings
import "profiles.js" as Profiles
import "words.js" as Words

ColumnLayout {
    id: section

    required property var host
    readonly property var s: host.settings
    // null is the default column; the others are power profiles.
    readonly property var columns: s ? [null].concat(s.powerProfiles) : []

    visible: host.ready && s !== null
    Layout.fillWidth: true
    spacing: 6

    SettingRow {
        settingId: "role-profiles"
        label: "Profile each role gives"
        highlight: section.host.highlight
        Layout.fillWidth: true
    }
    UqLabel {
        text: "Roles give a profile to apps no rule matches, for what they're doing right now. A power profile's column replaces the default while that power profile is on."
        color: section.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    RowLayout {
        Layout.fillWidth: true
        Item {
            Layout.preferredWidth: 160
        }
        Repeater {
            model: section.columns
            delegate: UqLabel {
                required property var modelData
                text: modelData === null ? "Default" : modelData
                font.bold: true
                elide: Text.ElideRight
                Layout.fillWidth: true
                Layout.preferredWidth: 1
            }
        }
    }
    Repeater {
        model: Profiles.ROLES
        delegate: RowLayout {
            id: roleRow
            required property string modelData
            Layout.fillWidth: true
            UqLabel {
                text: Words.roleLabel(roleRow.modelData)
                wrapMode: Text.Wrap
                Layout.preferredWidth: 160
            }
            Repeater {
                model: section.columns
                delegate: ChoiceBox {
                    required property var modelData
                    objectName: "role-" + roleRow.modelData + "-" + (modelData === null ? "default" : modelData)
                    choices: section.s ? Profiles.roleChoices(section.s, roleRow.modelData, modelData) : []
                    current: section.s ? Profiles.roleCurrent(section.s, roleRow.modelData, modelData) : null
                    enabled: section.host.busy === ""
                    Layout.fillWidth: true
                    Layout.preferredWidth: 1
                    onPicked: v => {
                        const power = modelData;
                        section.host.edit("roleprof", (layer, doc) => Profiles.setRole(layer, doc, roleRow.modelData, power, v));
                    }
                }
            }
        }
    }
    AreaStatus {
        host: section.host
        areas: ["roleprof"]
    }
}
