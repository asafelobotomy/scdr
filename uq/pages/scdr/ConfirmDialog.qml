// The page's own "are you sure?" for changes made over D-Bus (uQ asks for commands itself).
// The text is fixed or comes from logic.js; a rule's name is shown as plain text.
import QtQuick
import QtQuick.Controls as QQC2
import org.uq.settings

QQC2.Dialog {
    id: dialog

    property string message
    property var action: null

    title: "Are you sure?"
    modal: true
    anchors.centerIn: QQC2.Overlay.overlay
    standardButtons: QQC2.Dialog.Cancel | QQC2.Dialog.Ok

    // Runs `fn` only if the person confirms.
    function ask(text, fn) {
        dialog.message = text;
        dialog.action = fn;
        dialog.open();
    }

    contentItem: Item {
        implicitWidth: 360
        implicitHeight: body.implicitHeight
        UqLabel {
            id: body
            width: parent.width
            text: dialog.message
            wrapMode: Text.Wrap
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
