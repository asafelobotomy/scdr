// Check that scdr's files load together (offline, `scdrctl check`) and make scdr read them again.
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
    property string reloaded: ""

    function check() {
        if (!section.host.ctx || section.loading)
            return;
        section.loading = true;
        section.view = null;
        section.host.ctx.command.run("check", [], section.host.live(result => {
            section.loading = false;
            section.view = Plans.checkView(result);
        }));
    }

    function reload() {
        section.reloaded = "";
        section.host.dbusWrite("reload", "Reload", [], result => {
            section.reloaded = result && !result.error ? "scdr read its files again: " + String(result.value || "done") + "." : "";
        });
    }

    Layout.fillWidth: true
    spacing: 6

    SettingRow {
        settingId: "tools-check"
        label: "Do scdr's files load together?"
        highlight: section.host.highlight
        Layout.fillWidth: true
        QQC2.Button {
            objectName: "check"
            text: "Check"
            enabled: !section.loading
            onClicked: section.check()
        }
        QQC2.Button {
            objectName: "reload"
            text: "Reload scdr"
            enabled: section.host.ready && section.host.busy === ""
            onClicked: section.reload()
        }
    }
    QQC2.ProgressBar {
        visible: section.loading || section.host.working("reload")
        indeterminate: true
        Layout.fillWidth: true
    }
    UqLabel {
        visible: section.view !== null
        text: section.view !== null ? section.view.text : ""
        color: section.view !== null && !section.view.ok ? section.host.errorColor : palette.windowText
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    Repeater {
        model: section.view !== null ? section.view.warnings : []

        UqLabel {
            required property var modelData

            text: "Warning: " + modelData
            color: section.host.mutedColor
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
    }
    UqLabel {
        visible: section.view !== null && section.view.ok && section.view.files.length > 0
        text: section.view !== null ? "Read, lowest priority first: " + section.view.files.join(", ") : ""
        color: section.host.mutedColor
        wrapMode: Text.WrapAnywhere
        Layout.fillWidth: true
    }
    UqLabel {
        visible: section.reloaded !== ""
        text: section.reloaded
        color: section.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    UqLabel {
        visible: section.host.problem("reload") !== ""
        text: section.host.problem("reload")
        color: section.host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
