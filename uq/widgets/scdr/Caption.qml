// A line of plain text through the shell's UqText (widgets may not use their own text types).
import QtQuick

Item {
    id: cap

    property var ctx
    property string text: ""
    property color color: ctx ? ctx.Theme.text : "#e4e4e7"
    property real size: ctx ? ctx.Theme.fontSize : 11
    property bool bold: false
    property bool wrap: false
    property bool alignRight: false

    implicitWidth: loader.item ? loader.item.implicitWidth : 0
    implicitHeight: loader.item ? loader.item.implicitHeight : 0
    Accessible.ignored: true

    Loader {
        id: loader
        sourceComponent: cap.ctx ? cap.ctx.UqText : null
        onLoaded: {
            item.text = Qt.binding(function () { return cap.text; });
            item.color = Qt.binding(function () { return cap.color; });
            item.font.pixelSize = Qt.binding(function () { return Math.max(1, Math.round(cap.size)); });
            item.font.bold = Qt.binding(function () { return cap.bold; });
            item.wrapMode = Qt.binding(function () { return cap.wrap ? Text.WordWrap : Text.NoWrap; });
            item.horizontalAlignment = Qt.binding(function () { return cap.alignRight ? Text.AlignRight : Text.AlignLeft; });
            item.width = Qt.binding(function () { return cap.width; });
        }
    }
}
