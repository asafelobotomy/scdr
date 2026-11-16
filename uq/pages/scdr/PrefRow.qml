// One setting of `scdrctl config show`: its label, a control that fits its type, where the value
// comes from, and why scdr refused the last change. A change runs `config set` or `config unset`
// (the page's setPref); the row shows the file's value again until it follows the change.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "plans.js" as Plans

ColumnLayout {
    id: line

    required property var host
    /// The key of the setting; the row itself is looked up in the page's config each time it
    /// is read again, so the controls stay while the values change.
    required property string rowKey
    property string settingId: ""
    readonly property var row: line.host.config.rows.find(r => r.key === line.rowKey) || Plans.emptyRow(line.rowKey)

    // Which control the row shows: switch, tristate (on, off or left to the kernel), number,
    // choice (a fixed list), or text (a name, or a comma-separated list).
    readonly property string kind: line.row.type === "bool" ? (line.row.nullable ? "tristate" : "switch")
        : (line.row.type === "uint" || line.row.type === "float" ? "number"
        : (line.row.type === "text" && line.row.choices.length > 0 ? "choice" : "text"))
    readonly property bool mine: line.row.source === "you"
    readonly property var effective: line.row.value !== null && line.row.value !== undefined ? line.row.value : line.row.fallback
    readonly property var note: line.host.prefNotes[line.row.key] || ""
    readonly property bool working: line.host.workingKey === line.row.key

    Layout.fillWidth: true
    spacing: 2

    SettingRow {
        settingId: line.settingId
        label: line.row.label
        highlight: line.host.highlight
        Layout.fillWidth: true

        QQC2.Switch {
            id: toggle
            objectName: line.kind === "switch" ? "pref-" + line.row.key : ""
            visible: line.kind === "switch"
            checked: line.effective === true
            enabled: line.host.workingKey === ""
            onToggled: line.host.setPref(line.row, toggle.checked ? "on" : "off")
            // A refused change snaps the switch back to what the file says.
            Connections {
                target: line
                function onEffectiveChanged() {
                    toggle.checked = line.effective === true;
                }
                function onNoteChanged() {
                    toggle.checked = line.effective === true;
                }
            }
        }
        ChoiceBox {
            objectName: line.kind === "tristate" ? "pref-" + line.row.key : ""
            visible: line.kind === "tristate"
            choices: [{ text: "Leave to the kernel", value: null }, { text: "On", value: true }, { text: "Off", value: false }]
            current: line.row.value === undefined ? null : line.row.value
            enabled: line.host.workingKey === ""
            onPicked: v => line.host.setPref(line.row, v === null ? undefined : (v ? "on" : "off"))
        }
        OptionalNumber {
            objectName: line.kind === "number" ? "pref-" + line.row.key : ""
            visible: line.kind === "number"
            from: line.row.from
            to: line.row.to
            decimals: line.row.type === "float" ? 1 : 0
            suffix: line.row.unit !== "" ? " " + line.row.unit : ""
            value: line.mine ? line.row.value : undefined
            fallback: typeof line.effective === "number" ? line.effective : undefined
            revision: line.host.configRevision
            enabled: line.host.workingKey === ""
            onChosen: v => line.host.setPref(line.row, v === undefined ? undefined : String(v))
        }
        ChoiceBox {
            objectName: line.kind === "choice" ? "pref-" + line.row.key : ""
            visible: line.kind === "choice"
            choices: [{ text: "Not set", value: null }].concat(line.row.choices.map(c => ({ text: c, value: c })))
            current: line.mine && typeof line.row.value === "string" ? line.row.value : null
            enabled: line.host.workingKey === ""
            onPicked: v => line.host.setPref(line.row, v === null ? undefined : v)
        }
        QQC2.TextField {
            id: text
            objectName: line.kind === "text" ? "pref-" + line.row.key : ""
            visible: line.kind === "text"
            text: line.row.type === "list" ? Plans.listText(line.row.value) : (line.mine && typeof line.row.value === "string" ? line.row.value : "")
            placeholderText: line.row.type === "list" ? "name, name, …" : "Not set"
            maximumLength: 400
            implicitWidth: 240
            enabled: line.host.workingKey === ""
            onAccepted: line.host.setPref(line.row, text.text.trim() === "" && line.row.type === "text" ? undefined : text.text)
        }
        QQC2.Button {
            objectName: "pref-reset-" + line.row.key
            visible: line.mine && line.row.type !== "list"
            flat: true
            text: "Use the default"
            enabled: line.host.workingKey === ""
            onClicked: line.host.setPref(line.row, undefined)
        }
    }
    UqLabel {
        visible: line.row.help !== ""
        text: line.row.help
        color: line.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    UqLabel {
        visible: line.row.inherited.length > 0
        text: "Also from scdr's files: " + Plans.listText(line.row.inherited)
        color: line.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    UqLabel {
        text: line.working ? "Saving…" : Plans.sourceText(line.row) + " · now " + Plans.valueText(line.row)
        color: line.host.mutedColor
        Layout.fillWidth: true
        elide: Text.ElideRight
    }
    UqLabel {
        visible: line.note !== ""
        text: line.note
        color: line.host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
