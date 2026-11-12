// The person's own rules: a quick "add a rule for this app", then each rule to edit (profile,
// conditions, order, removal).
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "logic.js" as Logic

ColumnLayout {
    id: section

    required property var host
    property alias field: ruleField
    property alias profile: profileCombo
    readonly property var rules: host.settings ? host.settings.layerRules : []

    visible: host.ready && host.settings !== null
    Layout.fillWidth: true
    spacing: 6

    SettingRow {
        settingId: "rules"
        label: "Your app rules"
        highlight: section.host.highlight
        Layout.fillWidth: true
    }
    UqLabel {
        text: "Rules are tried from the top and the first match wins; yours come before scdr's own. A condition takes patterns like firefox or *chrom*, several separated by commas, and a rule needs all of its conditions to match."
        color: section.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    RowLayout {
        Layout.fillWidth: true
        QQC2.TextField {
            id: ruleField
            objectName: "rule-name"
            placeholderText: "App name, like firefox"
            maximumLength: 64
            enabled: section.host.busy === ""
            Layout.fillWidth: true
            onTextChanged: section.host.ruleCheck = text === "" ? "" : Logic.checkRuleName(text)
            onAccepted: section.host.addRule()
        }
        UqComboBox {
            id: profileCombo
            objectName: "rule-profile"
            model: section.host.profileNames
            enabled: section.host.busy === ""
        }
        QQC2.Button {
            objectName: "rule-add"
            text: "Add rule"
            enabled: section.host.busy === "" && ruleField.text !== "" && section.host.ruleCheck === ""
            onClicked: section.host.addRule()
        }
    }
    UqLabel {
        visible: section.host.ruleCheck !== ""
        text: section.host.ruleCheck
        color: section.host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    AreaStatus {
        host: section.host
        areas: ["rules"]
    }
    UqLabel {
        visible: section.rules.length === 0
        text: "You haven't added any rules."
        color: section.host.mutedColor
    }
    Repeater {
        model: section.rules
        delegate: RuleCard {
            required property int index
            host: section.host
            rule: section.rules[index]
            count: section.host.settings ? section.host.settings.layer.rule.length : 0
        }
    }
}
