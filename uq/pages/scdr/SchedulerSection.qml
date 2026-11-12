// Which CPU scheduler scdr runs: whether it picks one at all, by default, while a game runs and
// per power profile, what runs now, and a trial started below (with Stop).
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "schedulers.js" as Schedulers
import "words.js" as Words

ColumnLayout {
    id: section

    required property var host
    readonly property var s: host.settings
    // "default", "gaming" and each power profile, with their labels.
    readonly property var slots: s ? [{ slot: "default", label: "Default" }, { slot: "gaming", label: "While a game runs" }]
        .concat(s.powerProfiles.map(p => ({ slot: p, label: "On " + p }))) : []

    visible: host.ready && s !== null
    Layout.fillWidth: true
    spacing: 6

    function change(fn) {
        section.host.edit("scheduler", fn);
    }

    ColumnLayout {
        visible: section.s !== null && section.s.trial !== null
        Layout.fillWidth: true
        RowLayout {
            Layout.fillWidth: true
            UqLabel {
                objectName: "trial-text"
                text: Words.trialText(section.s ? section.s.trial : null)
                color: section.host.errorColor
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            QQC2.Button {
                objectName: "trial-stop"
                text: "Stop trying"
                enabled: section.host.busy === ""
                onClicked: section.host.write("trial", "EndSchedulerTrial", [])
            }
        }
    }
    SettingRow {
        settingId: "scheduler"
        label: "CPU scheduler"
        highlight: section.host.highlight
        Layout.fillWidth: true
    }
    SwitchRow {
        objectName: "scheduler-enable"
        label: "Let scdr pick an extra CPU scheduler"
        value: section.s !== null && section.s.schedExt
        enabled: section.host.busy === ""
        Layout.fillWidth: true
        onChosen: on => section.change(layer => Schedulers.setEnabled(layer, on))
    }
    UqLabel {
        visible: section.s !== null && !section.s.schedExt
        text: "Off: the kernel's own scheduler runs, whatever is picked below. Picking an extra scheduler turns this on."
        color: section.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    Repeater {
        model: section.slots
        delegate: RowLayout {
            id: slotRow
            required property var modelData
            Layout.fillWidth: true
            UqLabel {
                text: slotRow.modelData.label
                Layout.preferredWidth: 180
                elide: Text.ElideRight
            }
            ChoiceBox {
                objectName: "scheduler-" + slotRow.modelData.slot
                choices: section.s ? Schedulers.slotChoices(section.s, slotRow.modelData.slot) : []
                current: section.s ? Schedulers.slotCurrent(section.s, slotRow.modelData.slot) : null
                enabled: section.host.busy === ""
                Layout.fillWidth: true
                onPicked: v => section.change(layer => Schedulers.setSlot(layer, slotRow.modelData.slot, v))
            }
        }
    }
    UqLabel {
        text: "Order: while a game runs, then the power profile, then the default. A scheduler you start by hand is left running until scdr's choice changes."
        color: section.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    UqLabel {
        visible: section.s !== null && section.s.currentScheduler !== ""
        text: "Running now: " + (section.s ? section.s.currentScheduler : "")
        color: section.host.mutedColor
    }
    AreaStatus {
        host: section.host
        areas: ["scheduler", "trial"]
    }
}
