// "Working…" while a change to one of `areas` is in flight, and why the last one failed.
import QtQuick
import QtQuick.Layouts
import org.uq.settings

ColumnLayout {
    id: status

    required property var host
    property var areas: []
    readonly property string problemText: status.areas.map(a => status.host.problem(a)).filter(s => s).join("\n")

    Layout.fillWidth: true
    spacing: 2

    UqLabel {
        visible: status.areas.some(a => status.host.working(a))
        text: "Working…"
        color: status.host.mutedColor
    }
    UqLabel {
        visible: status.problemText !== ""
        text: status.problemText
        color: status.host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
