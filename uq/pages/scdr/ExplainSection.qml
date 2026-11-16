// Why a running process got the priority it has: the daemon's `Explain` for a process number
// (also reachable from the list of running apps).
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "plans.js" as Plans

ColumnLayout {
    id: section

    required property var host
    property var view: null
    property bool loading: false
    property int pid: 0
    readonly property alias field: pidField

    function explain(pid) {
        if (!(pid > 0 && pid <= 4194304) || !section.host.ctx || section.loading)
            return;
        section.loading = true;
        section.pid = pid;
        section.host.ctx.dbus.call("Explain", [pid], section.host.live(result => {
            section.loading = false;
            section.view = Plans.explainView(result);
        }));
    }

    Layout.fillWidth: true
    spacing: 6

    SettingRow {
        settingId: "explain"
        label: "Why does a process have its priority?"
        highlight: section.host.highlight
        Layout.fillWidth: true
        QQC2.TextField {
            id: pidField
            objectName: "explain-pid"
            placeholderText: "Process number"
            validator: IntValidator { bottom: 1; top: 4194304 }
            inputMethodHints: Qt.ImhDigitsOnly
            implicitWidth: 140
            enabled: section.host.ready
            onAccepted: section.explain(Number(text))
        }
        QQC2.Button {
            objectName: "explain-go"
            text: "Explain"
            enabled: section.host.ready && !section.loading && pidField.text !== ""
            onClicked: section.explain(Number(pidField.text))
        }
    }
    QQC2.ProgressBar {
        visible: section.loading
        indeterminate: true
        Layout.fillWidth: true
    }
    UqLabel {
        visible: section.view !== null && !section.view.ok
        text: section.view !== null ? section.view.problem : ""
        color: section.host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    Repeater {
        model: section.view !== null && section.view.ok ? section.view.rows : []

        RowLayout {
            id: line

            required property var modelData

            Layout.fillWidth: true
            UqLabel {
                text: line.modelData.label
                color: section.host.mutedColor
                Layout.preferredWidth: 110
                Layout.alignment: Qt.AlignTop
            }
            UqLabel {
                text: line.modelData.text
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }
    }
    Repeater {
        model: section.view !== null && section.view.ok ? section.view.notes : []

        UqLabel {
            required property var modelData

            text: modelData
            color: section.host.mutedColor
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
    }
}
