// Profiles: the settings a rule or role applies. Changing a profile from scdr's files copies it
// whole into your settings, where it replaces the one in the files; Undo drops the copy.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "profiles.js" as Profiles
import "words.js" as Words

ColumnLayout {
    id: section

    required property var host
    readonly property var s: host.settings
    property string selected: ""
    property string nameCheck: ""
    readonly property var view: s && selected !== "" ? Profiles.profileView(s, selected) : null

    visible: host.ready && s !== null
    Layout.fillWidth: true
    spacing: 6

    function change(fn, after) {
        section.host.edit("profiles", fn, after);
    }

    function setField(field, value) {
        const name = section.selected;
        section.change((layer, doc) => Profiles.setField(layer, doc, name, field, value));
    }

    function add() {
        const name = newName.text;
        const from = section.selected;
        section.nameCheck = Profiles.checkNewName(section.s, name);
        if (section.nameCheck !== "")
            return;
        section.change((layer, doc) => Profiles.addProfile(layer, doc, name, from), () => {
            newName.text = "";
            section.selected = name;
        });
    }

    function remove() {
        const name = section.selected;
        const shipped = section.view.shipped;
        section.host.confirmDialog.ask(Words.confirmText(shipped ? "profile-revert" : "profile-remove", name), () => {
            section.change((layer, doc) => Profiles.removeProfile(layer, doc, name), () => {
                if (!shipped)
                    section.selected = "";
            });
        });
    }

    // Keeps a profile selected while there is one.
    Connections {
        target: section.host
        function onProfileNamesChanged() {
            const names = section.host.profileNames;
            if (names.indexOf(section.selected) < 0)
                section.selected = names.length ? names[0] : "";
        }
    }

    SettingRow {
        settingId: "profiles"
        label: "Profiles"
        highlight: section.host.highlight
        Layout.fillWidth: true
        ChoiceBox {
            objectName: "profile-pick"
            choices: section.host.profileNames.map(n => ({ text: n, value: n }))
            current: section.selected
            onPicked: v => section.selected = v
        }
    }
    RowLayout {
        Layout.fillWidth: true
        QQC2.TextField {
            id: newName
            objectName: "profile-name"
            placeholderText: "New profile name"
            maximumLength: 40
            enabled: section.host.busy === ""
            Layout.fillWidth: true
            onTextChanged: section.nameCheck = text === "" ? "" : Profiles.checkNewName(section.s, text)
            onAccepted: section.add()
        }
        QQC2.Button {
            objectName: "profile-add"
            text: "Add profile"
            enabled: section.host.busy === "" && newName.text !== "" && section.nameCheck === ""
            onClicked: section.add()
        }
    }
    UqLabel {
        visible: section.nameCheck !== ""
        text: section.nameCheck
        color: section.host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    UqLabel {
        visible: newName.text !== "" && section.selected !== ""
        text: "A new profile starts out inheriting everything from the one selected above."
        color: section.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    ColumnLayout {
        visible: section.view !== null
        Layout.fillWidth: true
        spacing: 4
        UqLabel {
            objectName: "profile-origin"
            text: section.view ? section.view.origin : ""
            color: section.host.mutedColor
            elide: Text.ElideMiddle
            Layout.fillWidth: true
        }
        Repeater {
            model: [{ key: "inherit", label: "Inherits" }, { key: "sched", label: "Policy" }, { key: "io", label: "Disk priority" }]
            delegate: RowLayout {
                id: pickRow
                required property var modelData
                readonly property var cur: section.view ? section.view.fields[modelData.key] : undefined
                Layout.fillWidth: true
                UqLabel {
                    text: pickRow.modelData.label
                    Layout.preferredWidth: 220
                }
                ChoiceBox {
                    objectName: "profile-" + pickRow.modelData.key
                    choices: !section.s ? [] : pickRow.modelData.key === "inherit" ? Profiles.inheritChoices(section.s, section.selected)
                        : pickRow.modelData.key === "sched" ? Profiles.policyChoices(pickRow.cur) : Profiles.ioChoices(pickRow.cur)
                    current: pickRow.cur === undefined ? null : pickRow.cur
                    enabled: section.host.busy === ""
                    Layout.fillWidth: true
                    onPicked: v => section.setField(pickRow.modelData.key, v)
                }
            }
        }
        Repeater {
            model: Profiles.NUMBERS
            delegate: RowLayout {
                id: numberRow
                required property var modelData
                Layout.fillWidth: true
                UqLabel {
                    text: numberRow.modelData.label
                    elide: Text.ElideRight
                    Layout.preferredWidth: 220
                }
                OptionalNumber {
                    objectName: "profile-" + numberRow.modelData.key
                    from: numberRow.modelData.from
                    to: numberRow.modelData.to
                    suffix: numberRow.modelData.suffix
                    value: section.view ? section.view.fields[numberRow.modelData.key] : undefined
                    revision: section.host.revision
                    enabled: section.host.busy === ""
                    onChosen: v => section.setField(numberRow.modelData.key, v)
                }
            }
        }
        QQC2.Button {
            objectName: "profile-remove"
            visible: section.view !== null && section.view.edited
            text: section.view && section.view.shipped ? "Undo changes" : "Delete profile"
            enabled: section.host.busy === ""
            onClicked: section.remove()
        }
        UqLabel {
            text: "Unset fields are left as they are, or come from the inherited profile. Unit weights apply to an app's whole unit, so give them only to profiles for whole apps or services. The fifo and rr policies can starve the rest of the system."
            color: section.host.mutedColor
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
    }
    AreaStatus {
        host: section.host
        areas: ["profiles"]
    }
}
