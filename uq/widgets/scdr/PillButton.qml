// A small push button for the popup (the shell's own controls aren't open to widgets).
import QtQuick

Rectangle {
    id: button

    property var ctx
    property string text: ""
    property bool primary: false
    signal clicked

    implicitWidth: caption.implicitWidth + 20
    implicitHeight: caption.implicitHeight + 8
    radius: height / 2
    opacity: enabled ? 1 : 0.5
    color: !ctx ? "transparent"
         : primary ? (mouse.containsMouse ? Qt.lighter(ctx.Theme.accent, 1.1) : ctx.Theme.accent)
         : (mouse.containsMouse ? ctx.Theme.overlay : ctx.Theme.surfaceAlt)
    border.width: 1
    border.color: ctx ? ctx.Theme.border : "transparent"
    Accessible.role: Accessible.Button
    Accessible.name: text
    Accessible.onPressAction: if (enabled) button.clicked()

    Caption {
        id: caption
        ctx: button.ctx
        anchors.centerIn: parent
        text: button.text
        color: button.ctx ? (button.primary ? button.ctx.Theme.accentText : button.ctx.Theme.text) : "white"
    }
    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: if (button.enabled) button.clicked()
    }
}
