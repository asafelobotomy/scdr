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
    property string ruleCheck: ""
    readonly property var rules: host.settings ? host.settings.layerRules : []

    visible: host.ready && host.settings !== null
    Layout.fillWidth: true
    spacing: 6

    function add() {
        const profile = section.host.profileNames[profileCombo.currentIndex] || "";
        section.ruleCheck = Logic.checkRuleName(ruleField.text);
        if (section.ruleCheck === "" && Logic.checkProfile(profile) !== "")
            section.ruleCheck = Logic.checkProfile(profile);
        if (section.ruleCheck !== "")
            return;
        const name = ruleField.text;
        section.host.edit("rules", layer => Logic.addRule(layer, profile, name), () => {
            ruleField.text = "";
            section.ruleCheck = "";
        });
    }

    // The profile an added rule gets starts out as "desktop", when there is one.
    Connections {
        target: section.host
        function onProfileNamesChanged() {
            profileCombo.currentIndex = Math.max(0, section.host.profileNames.indexOf("desktop"));
        }
    }

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
            onTextChanged: section.ruleCheck = text === "" ? "" : Logic.checkRuleName(text)
            onAccepted: section.add()
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
            enabled: section.host.busy === "" && ruleField.text !== "" && section.ruleCheck === ""
            onClicked: section.add()
        }
    }
    UqLabel {
        visible: section.ruleCheck !== ""
        text: section.ruleCheck
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
