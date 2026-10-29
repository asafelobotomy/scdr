// Rules: which processes get which profile. Rules added here are tried
// before those in the shipped and /etc files; the first match wins.

import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.kcmutils as KCM

KCM.SimpleKCM {
    id: page

    property SettingsStore store: SettingsStore {}
    property bool unsavedChanges: store.dirty

    function saveConfig() {
        store.save();
    }

    readonly property var fields: ["name", "exe", "cmdline", "cgroup", "parent", "ancestor", "uid",
                                   "not_name", "not_exe", "not_cmdline", "not_cgroup", "not_parent", "not_ancestor"]
    readonly property var rules: (store.revision, store.layerValue(["rule"]) || [])
    readonly property var otherRules: (store.revision, store.baseValue(["rule"]) || [])
    readonly property var profileChoices: (store.revision, store.profileNames().map(n => ({ text: n, value: n })))

    function conditions(rule) {
        return fields.filter(f => rule[f] !== undefined).map(f => ({ field: f, value: rule[f] }));
    }

    // Values from a Repeater's modelData are Qt lists, not JS arrays.
    function show(value) {
        return typeof value === "object" && value !== null ? Array.from(value).join(", ") : String(value);
    }

    function parse(field, text) {
        const parts = text.split(",").map(s => s.trim()).filter(s => s !== "");
        if (field === "uid") {
            return parts.map(Number).filter(n => Number.isInteger(n) && n >= 0);
        }
        return parts.length === 1 ? parts[0] : parts;
    }

    function summary(rule) {
        return conditions(rule).map(c => c.field + " = " + show(c.value)).join("; ") || i18n("matches everything");
    }

    function update(fn) {
        const copy = JSON.parse(JSON.stringify(rules));
        fn(copy);
        store.set(["rule"], copy.length ? copy : undefined);
    }

    ColumnLayout {
        spacing: Kirigami.Units.largeSpacing

        SettingsBanner {
            store: page.store
        }

        QQC2.Label {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            text: i18n("Rules are tried from the top; the first match wins. Conditions are globs, several separated by commas, and a rule needs all of its conditions to match. Use `scdrctl explain <process>` to see which rule a process gets.")
        }

        Kirigami.Heading {
            level: 4
            text: i18n("Your rules")
        }
        QQC2.Label {
            visible: page.rules.length === 0
            opacity: 0.7
            text: i18n("None yet.")
        }
        Repeater {
            model: page.rules
            delegate: Kirigami.AbstractCard {
                id: card
                required property var modelData
                required property int index
                Layout.fillWidth: true
                contentItem: ColumnLayout {
                    RowLayout {
                        QQC2.Label { text: i18n("Profile:") }
                        ChoiceCombo {
                            choices: page.profileChoices
                            current: card.modelData.profile
                            onPicked: v => page.update(r => r[card.index].profile = v)
                        }
                        Item { Layout.fillWidth: true }
                        QQC2.ToolButton {
                            icon.name: "go-up"
                            enabled: card.index > 0
                            onClicked: page.update(r => r.splice(card.index - 1, 0, r.splice(card.index, 1)[0]))
                            QQC2.ToolTip.text: i18n("Try earlier")
                            QQC2.ToolTip.visible: hovered
                        }
                        QQC2.ToolButton {
                            icon.name: "go-down"
                            enabled: card.index < page.rules.length - 1
                            onClicked: page.update(r => r.splice(card.index + 1, 0, r.splice(card.index, 1)[0]))
                            QQC2.ToolTip.text: i18n("Try later")
                            QQC2.ToolTip.visible: hovered
                        }
                        QQC2.ToolButton {
                            icon.name: "edit-delete"
                            onClicked: page.update(r => r.splice(card.index, 1))
                            QQC2.ToolTip.text: i18n("Remove rule")
                            QQC2.ToolTip.visible: hovered
                        }
                    }
                    Repeater {
                        model: page.conditions(card.modelData)
                        delegate: RowLayout {
                            id: cond
                            required property var modelData
                            ChoiceCombo {
                                choices: page.fields.map(f => ({ text: f, value: f }))
                                current: cond.modelData.field
                                onPicked: v => page.update(r => {
                                    const rule = r[card.index];
                                    const value = rule[cond.modelData.field];
                                    delete rule[cond.modelData.field];
                                    rule[v] = v === "uid" ? page.parse(v, page.show(value)) : value;
                                })
                            }
                            QQC2.TextField {
                                Layout.fillWidth: true
                                text: page.show(cond.modelData.value)
                                placeholderText: cond.modelData.field === "uid" ? i18n("1000, 1001") : i18n("firefox, *chrom*")
                                onEditingFinished: {
                                    const v = page.parse(cond.modelData.field, text);
                                    if (JSON.stringify(v) !== JSON.stringify(cond.modelData.value)) {
                                        page.update(r => r[card.index][cond.modelData.field] = v);
                                    }
                                }
                            }
                            QQC2.ToolButton {
                                icon.name: "list-remove"
                                onClicked: page.update(r => delete r[card.index][cond.modelData.field])
                                QQC2.ToolTip.text: i18n("Remove condition")
                                QQC2.ToolTip.visible: hovered
                            }
                        }
                    }
                    QQC2.Button {
                        text: i18n("Add condition")
                        icon.name: "list-add"
                        readonly property var free: page.fields.filter(f => card.modelData[f] === undefined)
                        enabled: free.length > 0
                        onClicked: page.update(r => r[card.index][free[0]] = free[0] === "uid" ? [1000] : "*")
                    }
                }
            }
        }
        QQC2.Button {
            text: i18n("Add rule")
            icon.name: "list-add"
            enabled: page.profileChoices.length > 0
            onClicked: page.update(r => r.push({ profile: page.profileChoices[0].value, name: "" }))
        }

        Kirigami.Heading {
            level: 4
            text: i18n("From the shipped and /etc files")
        }
        Repeater {
            model: page.otherRules
            delegate: QQC2.ItemDelegate {
                required property var modelData
                Layout.fillWidth: true
                enabled: false
                contentItem: ColumnLayout {
                    QQC2.Label {
                        Layout.fillWidth: true
                        elide: Text.ElideRight
                        text: i18n("%1 — %2", modelData.profile, page.summary(modelData))
                    }
                    QQC2.Label {
                        Layout.fillWidth: true
                        elide: Text.ElideMiddle
                        font: Kirigami.Theme.smallFont
                        opacity: 0.7
                        text: modelData._source || ""
                    }
                }
            }
        }
    }
}
