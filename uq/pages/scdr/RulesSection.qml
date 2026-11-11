// The person's own app rules: the list, Remove, and the add form.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "logic.js" as Logic

ColumnLayout {
    required property var host
    property alias field: ruleField
    property alias profile: profileCombo
    visible: host.ready && host.settings !== null
    Layout.fillWidth: true
    spacing: 6

    SettingRow {
        settingId: "rules"
        label: "Your app rules"
        highlight: host.highlight
        Layout.fillWidth: true
        UqLabel {
            visible: host.working("rules")
            text: "Working…"
            color: host.mutedColor
        }
    }
    UqLabel {
        visible: host.settings !== null && host.settings.layerRules.length === 0
        text: "You haven't added any rules."
        color: host.mutedColor
    }
    Repeater {
        model: host.settings ? host.settings.layerRules : []
        delegate: RowLayout {
            id: ruleRow
            required property var modelData
            Layout.fillWidth: true
            UqLabel {
                objectName: "rule-" + ruleRow.modelData.index
                text: ruleRow.modelData.name + " → " + ruleRow.modelData.profile
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            QQC2.Button {
                objectName: "remove-" + ruleRow.modelData.index
                visible: ruleRow.modelData.simple
                enabled: host.busy === ""
                text: "Remove"
                onClicked: host.removeRule(ruleRow.modelData.name, ruleRow.modelData.index)
            }
        }
    }
    RowLayout {
        Layout.fillWidth: true
        QQC2.TextField {
            id: ruleField
            objectName: "rule-name"
            placeholderText: "App name, like firefox"
            maximumLength: 64
            enabled: host.busy === ""
            Layout.fillWidth: true
            onTextChanged: host.ruleCheck = text === "" ? "" : Logic.checkRuleName(text)
            onAccepted: host.addRule()
        }
        UqComboBox {
            id: profileCombo
            objectName: "rule-profile"
            model: host.profileNames
            enabled: host.busy === ""
        }
        QQC2.Button {
            objectName: "rule-add"
            text: "Add"
            enabled: host.busy === "" && ruleField.text !== "" && host.ruleCheck === ""
            onClicked: host.addRule()
        }
    }
    UqLabel {
        visible: host.ruleCheck !== ""
        text: host.ruleCheck
        color: host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    UqLabel {
        visible: host.problem("rules") !== ""
        text: host.problem("rules")
        color: host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    UqLabel {
        text: "Rules from scdr's files come after yours. See them with scdrctl list."
        color: host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
