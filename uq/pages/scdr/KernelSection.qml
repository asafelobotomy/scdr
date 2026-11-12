// The kernel scheduler's own knobs, used while no extra scheduler runs: preemption and the base
// time slice, by default and per power profile.
import QtQuick
import QtQuick.Layouts
import org.uq.settings
import "logic.js" as Logic
import "schedulers.js" as Schedulers

ColumnLayout {
    id: section

    required property var host
    readonly property var s: host.settings
    readonly property var rows: s ? [{ power: null, label: "Default" }].concat(s.powerProfiles.map(p => ({ power: p, label: "On " + p }))) : []

    visible: host.ready && s !== null
    Layout.fillWidth: true
    spacing: 6

    function path(power, knob) {
        return power === null ? ["eevdf", knob] : ["eevdf", "power", power, knob];
    }

    SettingRow {
        settingId: "kernel-tuning"
        label: "Kernel scheduler tuning"
        highlight: section.host.highlight
        Layout.fillWidth: true
    }
    UqLabel {
        text: "Used while no extra scheduler runs. Preemption sets how soon a waiting task may interrupt a running one; the time slice is how long a task runs before others get a turn."
        color: section.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    Repeater {
        model: section.rows
        delegate: ColumnLayout {
            id: knobRow
            required property var modelData
            Layout.fillWidth: true
            spacing: 2
            UqLabel {
                text: knobRow.modelData.label
                font.bold: true
            }
            RowLayout {
                Layout.fillWidth: true
                UqLabel {
                    text: "Preemption"
                    Layout.preferredWidth: 140
                }
                ChoiceBox {
                    objectName: "preempt-" + (knobRow.modelData.power || "default")
                    choices: section.s ? Schedulers.preemptChoices(section.s, knobRow.modelData.power) : []
                    current: section.s ? (Logic.lookup(section.s.layer, section.path(knobRow.modelData.power, "preempt")) ?? null) : null
                    enabled: section.host.busy === ""
                    Layout.fillWidth: true
                    onPicked: v => section.host.edit("kernel", layer => Schedulers.setPreempt(layer, knobRow.modelData.power, v))
                }
            }
            RowLayout {
                Layout.fillWidth: true
                UqLabel {
                    text: "Base time slice"
                    Layout.preferredWidth: 140
                }
                OptionalNumber {
                    objectName: "slice-" + (knobRow.modelData.power || "default")
                    from: 100
                    to: 100000
                    suffix: " µs"
                    value: section.s ? Logic.lookup(section.s.layer, section.path(knobRow.modelData.power, "base_slice_us")) : undefined
                    fallback: section.s ? Logic.lookup(section.s.base, section.path(knobRow.modelData.power, "base_slice_us")) : undefined
                    revision: section.host.revision
                    enabled: section.host.busy === ""
                    onChosen: v => section.host.edit("kernel", layer => Schedulers.setSlice(layer, knobRow.modelData.power, v))
                }
            }
        }
    }
    AreaStatus {
        host: section.host
        areas: ["kernel"]
    }
}
