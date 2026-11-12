// One of the person's rules: its profile, its place in the order, its conditions.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "rules.js" as Rules

ColumnLayout {
    id: card

    required property var host
    // An entry of host.settings.layerRules, and how many there are.
    required property var rule
    required property int count
    property string check: ""
    readonly property int at: rule.index

    Layout.fillWidth: true
    Layout.topMargin: 6
    spacing: 2

    function change(fn) {
        const index = card.rule.index;
        const key = card.rule.key;
        card.check = "";
        card.host.edit("rules", (layer, doc) => fn(layer, doc, index, key));
    }

    RowLayout {
        Layout.fillWidth: true
        UqLabel {
            objectName: "rule-" + card.at
            text: card.rule.name + " → " + card.rule.profile
            font.bold: true
            elide: Text.ElideRight
            Layout.fillWidth: true
        }
        QQC2.Button {
            objectName: "up-" + card.at
            text: "Up"
            enabled: card.host.busy === "" && card.at > 0
            onClicked: card.change((layer, doc, i, key) => Rules.moveRule(layer, i, key, -1))
        }
        QQC2.Button {
            objectName: "down-" + card.at
            text: "Down"
            enabled: card.host.busy === "" && card.at < card.count - 1
            onClicked: card.change((layer, doc, i, key) => Rules.moveRule(layer, i, key, 1))
        }
        QQC2.Button {
            objectName: "remove-" + card.at
            text: "Remove"
            enabled: card.host.busy === ""
            onClicked: card.host.removeRule(card.rule)
        }
    }
    RowLayout {
        Layout.fillWidth: true
        UqLabel {
            text: "Profile"
            Layout.preferredWidth: 110
        }
        ChoiceBox {
            objectName: "rule-profile-" + card.at
            choices: {
                const names = card.host.profileNames.slice();
                if (names.indexOf(card.rule.profile) < 0)
                    names.push(card.rule.profile);
                return names.map(n => ({ text: n, value: n }));
            }
            current: card.rule.profile
            enabled: card.host.busy === ""
            Layout.fillWidth: true
            onPicked: v => card.change((layer, doc, i, key) => Rules.setProfile(layer, doc, i, key, v))
        }
    }
    Repeater {
        model: card.rule.conditions
        delegate: RowLayout {
            id: cond
            required property var modelData
            Layout.fillWidth: true
            ChoiceBox {
                objectName: "field-" + card.at + "-" + cond.modelData.field
                choices: Rules.fieldChoices(card.rule.conditions, cond.modelData.field)
                current: cond.modelData.field
                enabled: card.host.busy === ""
                Layout.preferredWidth: 110
                onPicked: v => {
                    const from = cond.modelData.field;
                    card.change((layer, doc, i, key) => Rules.renameCondition(layer, i, key, from, v));
                }
            }
            QQC2.TextField {
                objectName: "value-" + card.at + "-" + cond.modelData.field
                text: cond.modelData.text
                placeholderText: cond.modelData.field === "uid" ? "1000, 1001" : "firefox, *chrom*"
                enabled: card.host.busy === ""
                Layout.fillWidth: true
                onEditingFinished: {
                    if (text === cond.modelData.text)
                        return;
                    const parsed = Rules.parseValue(cond.modelData.field, text);
                    if (parsed.error) {
                        card.check = parsed.error;
                        return;
                    }
                    const field = cond.modelData.field;
                    const typed = text;
                    card.change((layer, doc, i, key) => Rules.setCondition(layer, i, key, field, typed));
                }
            }
            QQC2.Button {
                objectName: "drop-" + card.at + "-" + cond.modelData.field
                text: "Remove"
                enabled: card.host.busy === "" && card.rule.conditions.length > 1
                onClicked: {
                    const field = cond.modelData.field;
                    card.change((layer, doc, i, key) => Rules.removeCondition(layer, i, key, field));
                }
            }
        }
    }
    QQC2.Button {
        objectName: "add-condition-" + card.at
        text: "Add condition"
        enabled: card.host.busy === "" && card.rule.conditions.length < Rules.FIELDS.length
        onClicked: card.change((layer, doc, i, key) => Rules.addCondition(layer, i, key))
    }
    UqLabel {
        visible: card.check !== ""
        text: card.check
        color: card.host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
