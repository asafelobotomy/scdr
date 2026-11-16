// The page's own "are you sure?" for changes made over D-Bus (uQ asks for [[command]]s itself,
// and shows their previews). Like that sheet, it can show what the change does, grouped: `plan` is
// a Plans.planOf-shaped document the page builds. Text is fixed or from logic.js; plain text.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings

QQC2.Dialog {
    id: dialog

    property string message
    property var action: null
    property var plan: null
    property color mutedColor: "grey"
    property color errorColor: "red"

    title: "Are you sure?"
    modal: true
    anchors.centerIn: QQC2.Overlay.overlay
    standardButtons: QQC2.Dialog.Cancel | QQC2.Dialog.Ok

    // Runs `fn` only if the person confirms. `plan` (optional) lists what it would change.
    function ask(text, fn, plan) {
        dialog.message = text;
        dialog.action = fn;
        dialog.plan = plan || null;
        dialog.open();
    }

    contentItem: ColumnLayout {
        implicitWidth: 380
        spacing: 8
        UqLabel {
            id: body
            text: dialog.message
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        PlanView {
            plan: dialog.plan
            mutedColor: dialog.mutedColor
            errorColor: dialog.errorColor
            Layout.fillWidth: true
        }
    }

    onAccepted: {
        const fn = dialog.action;
        dialog.action = null;
        if (fn)
            fn();
    }
    onRejected: dialog.action = null
}
