// An on/off switch for the popup's role rows: `toggled(on)` asks for the change; `checked`
// follows scdr's state, so the knob only moves once scdr has made it.
import QtQuick

Item {
    id: toggle

    property var ctx
    property bool checked: false
    property string name: ""
    signal toggled(bool on)

    implicitWidth: 30
    implicitHeight: 16
    opacity: enabled ? 1 : 0.5
    Accessible.role: Accessible.CheckBox
    Accessible.name: name
    Accessible.checkable: true
    Accessible.checked: checked
    Accessible.onToggleAction: if (enabled) toggle.toggled(!checked)
    Accessible.onPressAction: if (enabled) toggle.toggled(!checked)

    Rectangle {
        anchors.fill: parent
        radius: height / 2
        color: !toggle.ctx ? "grey" : toggle.checked ? toggle.ctx.Theme.accent : toggle.ctx.Theme.overlay
        border.width: 1
        border.color: toggle.ctx ? toggle.ctx.Theme.border : "transparent"
    }
    Rectangle {
        width: parent.height - 4
        height: width
        radius: width / 2
        y: 2
        x: toggle.checked ? parent.width - width - 2 : 2
        color: !toggle.ctx ? "white" : toggle.checked ? toggle.ctx.Theme.accentText : toggle.ctx.Theme.text
        Behavior on x {
            NumberAnimation { duration: 120 }
        }
    }
    MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: if (toggle.enabled) toggle.toggled(!toggle.checked)
    }
}
