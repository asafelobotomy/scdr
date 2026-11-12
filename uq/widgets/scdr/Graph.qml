// One live graph of the popup, drawn with Canvas (KQuickCharts isn't open to widgets): a title,
// the series as filled lines over the whole history, an optional threshold line, and a legend
// with each series' latest value. The history is kept by Widget.qml; this only draws it.
import QtQuick

Column {
    id: graph

    property var ctx
    // The history (logic.js newHistory) and a counter bumped on each change.
    property var hist: null
    property int revision: 0
    property string title: ""
    // Parallel lists: history keys, colours, legend names and legend values (formatted).
    property var keys: []
    property var colors: []
    property var names: []
    property var values: []
    property real yMax: 100
    // A horizontal reference (same unit as the values); none when negative.
    property real line: -1
    property string lineLabel: ""
    property color lineColor: ctx ? ctx.Theme.warning : "orange"
    property int plotHeight: 52

    spacing: 3

    function rgba(c, a) {
        return "rgba(" + Math.round(c.r * 255) + "," + Math.round(c.g * 255) + "," + Math.round(c.b * 255) + "," + a + ")";
    }

    onRevisionChanged: canvas.requestPaint()
    onYMaxChanged: canvas.requestPaint()
    onKeysChanged: canvas.requestPaint()

    Caption {
        ctx: graph.ctx
        width: graph.width
        text: graph.title
        color: graph.ctx ? graph.ctx.Theme.textMuted : "grey"
        size: graph.ctx ? graph.ctx.Theme.fontSize - 1 : 10
    }

    Rectangle {
        width: graph.width
        height: graph.plotHeight
        radius: 4
        color: graph.ctx ? graph.ctx.Theme.bg : "black"
        border.width: 1
        border.color: graph.ctx ? graph.ctx.Theme.overlay : "grey"
        clip: true

        Canvas {
            id: canvas
            anchors.fill: parent
            anchors.margins: 1
            onWidthChanged: requestPaint()
            onHeightChanged: requestPaint()
            onPaint: {
                var g = getContext("2d");
                g.reset();
                var h = graph.hist;
                if (!h || width < 2 || height < 2)
                    return;
                var top = graph.yMax > 0 ? graph.yMax : 1;
                var n = h.length;
                var dx = (width - 1) / Math.max(1, n - 1);
                for (var s = 0; s < graph.keys.length; s++) {
                    var a = h.data[graph.keys[s]];
                    if (!a || !a.length)
                        continue;
                    var c = graph.colors[s];
                    var off = n - a.length;
                    g.beginPath();
                    for (var i = 0; i < a.length; i++) {
                        var x = (off + i) * dx;
                        var y = height - 1 - Math.min(a[i], top) / top * (height - 2);
                        if (i === 0)
                            g.moveTo(x, y);
                        else
                            g.lineTo(x, y);
                    }
                    g.lineWidth = 1;
                    g.strokeStyle = graph.rgba(c, 1);
                    g.stroke();
                    g.lineTo((off + a.length - 1) * dx, height);
                    g.lineTo(off * dx, height);
                    g.closePath();
                    g.fillStyle = graph.rgba(c, 0.15);
                    g.fill();
                }
            }
        }

        Rectangle {
            id: ref
            visible: graph.line >= 0 && graph.line <= graph.yMax
            width: parent.width
            height: 1
            y: parent.height * (1 - graph.line / Math.max(graph.yMax, 0.001))
            color: graph.lineColor
            opacity: 0.7
        }
        Caption {
            ctx: graph.ctx
            visible: ref.visible && graph.lineLabel !== ""
            anchors.right: parent.right
            anchors.rightMargin: 3
            y: Math.max(0, ref.y - height)
            text: graph.lineLabel
            color: graph.lineColor
            size: graph.ctx ? graph.ctx.Theme.fontSize - 2 : 9
        }
    }

    Flow {
        width: graph.width
        spacing: 10
        Repeater {
            model: graph.names.length
            delegate: Row {
                required property int index
                spacing: 4
                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 8
                    height: 8
                    radius: 2
                    color: graph.colors[index] || "grey"
                }
                Caption {
                    ctx: graph.ctx
                    text: graph.names[index] + " " + (graph.values[index] !== undefined ? graph.values[index] : "")
                    size: graph.ctx ? graph.ctx.Theme.fontSize - 1 : 10
                }
            }
        }
    }
}
