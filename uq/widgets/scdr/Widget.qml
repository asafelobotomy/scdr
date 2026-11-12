// uQ bar widget for scdr. The icon is a small CPU glyph with a sparkline of CPU pressure (two bars
// when paused); clicking it opens Details.qml. Everything comes from scdrd's State property on the
// system bus; the graphs' history lives here, in the bar, so it keeps collecting while the popup
// is closed. All text goes through ctx.UqText (Caption.qml); see docs/integrating.md in uQ.
import QtQuick
import "logic.js" as L

Item {
    id: root

    property var ctx
    // The parsed State (logic.js parseState), or null before the first read.
    property var scdr: null
    readonly property bool paused: scdr !== null && scdr.paused
    readonly property var scheduler: L.schedulerInfo(scdr ? scdr.scheduler : [])
    // The live graphs' history (logic.js newHistory), changed in place; `revision` says when.
    property var hist: L.newHistory(L.HISTORY_LENGTH)
    property int revision: 0
    // A change still waiting for polkit after the bridge's 2 s deadline (logic.js settled).
    property var pending: null
    // Which write is on its way: "pause", "role:<name>" or "".
    property string busy: ""
    property string problem: ""
    property bool getting: false
    // Whether the popup shows, and when it last closed. The shell closes it on a press outside
    // it, the icon included, before the click arrives: a click right after a close was meant to
    // close it, not to open it again.
    property bool popupShown: false
    property real popupClosedAt: 0

    property Component popup: Component {
        Details {
            w: root
            Component.onCompleted: root.popupShown = true
            Component.onDestruction: {
                root.popupShown = false;
                root.popupClosedAt = Date.now();
            }
        }
    }

    function togglePopup() {
        if (popupShown)
            ctx.closePopup();
        else if (Date.now() - popupClosedAt > 300)
            ctx.openPopup();
    }

    function ingest(text) {
        var s = L.parseState(text);
        if (!s) {
            problem = "scdr's state couldn't be read.";
            return;
        }
        if (problem === "scdr's state couldn't be read.")
            problem = "";
        scdr = s;
        if (L.record(hist, s, Date.now()) > 0)
            revision++;
        if (pending && L.settled(pending, s)) {
            pending = null;
            pendingTimeout.stop();
        }
    }

    function refresh() {
        if (getting || !ctx)
            return;
        getting = true;
        ctx.dbus.get(function (r) {
            root.getting = false;
            if (r && r.value && r.value.State !== undefined)
                root.ingest(r.value.State);
            else if (r && r.error && root.scdr === null)
                root.problem = "scdr's state couldn't be read: " + String(r.error).slice(0, 160);
        });
    }

    // Pause, Resume or SetRoleEnabled. `expect` is what State shows once it's done.
    function write(method, args, tag, expect) {
        if (busy !== "" || !ctx)
            return;
        busy = tag;
        problem = "";
        ctx.dbus.call(method, args, function (r) {
            root.busy = "";
            var o = L.callOutcome(r);
            if (o.status === "waiting") {
                root.pending = expect;
                pendingTimeout.restart();
            } else if (o.status === "error") {
                root.problem = o.message;
            }
            root.refresh();
        });
    }

    function togglePause() {
        if (scdr === null)
            return;
        if (paused)
            write("Resume", [], "pause", { kind: "resume" });
        else
            write("Pause", [], "pause", { kind: "pause" });
    }

    function setRole(role, on) {
        write("SetRoleEnabled", [role, on], "role:" + role, { kind: "role", role: role, enabled: on });
    }

    Component.onCompleted: {
        ctx.dbus.onProps(function (changed) {
            if (changed && changed.State !== undefined)
                root.ingest(changed.State);
        });
        refresh();
    }

    // Without scx_devbox samples State only changes when a value does: record pressure once a
    // second (as Plasma's widget does), and re-read State now and then in case a change was missed.
    Timer {
        interval: 1000
        repeat: true
        running: root.scdr !== null
        property int count: 0
        onTriggered: {
            if (L.tick(root.hist, root.scdr, Date.now())) {
                root.revision++;
                if (++count % 10 === 0)
                    root.refresh();
            }
        }
    }
    // polkit gives up on its own after a while; stop saying "waiting" by then.
    Timer {
        id: pendingTimeout
        interval: 120000
        onTriggered: root.pending = null
    }

    // ---- The bar icon ----------------------------------------------------------------------

    Rectangle {
        anchors.fill: parent
        radius: root.ctx ? root.ctx.Theme.radius - 2 : 4
        color: mouse.containsMouse && root.ctx ? root.ctx.Theme.overlay : "transparent"
    }

    Canvas {
        id: glyph
        anchors.centerIn: parent
        width: 20
        height: 20
        property var tint: !root.ctx ? "white"
                         : root.paused || root.scdr === null ? root.ctx.Theme.textMuted
                         : root.scdr.pressureActive ? root.ctx.Theme.warning : root.ctx.Theme.text
        property var spark: root.ctx ? root.ctx.Theme.accent : "skyblue"
        onTintChanged: requestPaint()
        onSparkChanged: requestPaint()
        Connections {
            target: root
            function onRevisionChanged() { glyph.requestPaint(); }
            function onPausedChanged() { glyph.requestPaint(); }
        }
        onPaint: {
            var g = getContext("2d");
            g.reset();
            g.strokeStyle = String(tint);
            g.fillStyle = String(tint);
            g.lineWidth = 1.5;
            // The chip: a rounded square with three pins a side.
            g.beginPath();
            g.roundedRect(3.75, 3.75, 12.5, 12.5, 2, 2);
            g.stroke();
            for (var i = 0; i < 3; i++) {
                var p = 6.5 + i * 3.5;
                g.fillRect(p - 0.75, 0.5, 1.5, 2.5);
                g.fillRect(p - 0.75, 17, 1.5, 2.5);
                g.fillRect(0.5, p - 0.75, 2.5, 1.5);
                g.fillRect(17, p - 0.75, 2.5, 1.5);
            }
            if (root.paused) {
                g.fillRect(7.25, 7, 1.75, 6);
                g.fillRect(11, 7, 1.75, 6);
                return;
            }
            // Inside: the last few seconds of CPU pressure, 0-100 %.
            var t = L.tail(root.hist, "cpu", 9);
            g.strokeStyle = String(spark);
            g.lineWidth = 1.25;
            g.beginPath();
            for (var j = 0; j < t.length; j++) {
                var x = 6 + j * (8 / Math.max(1, t.length - 1));
                var y = 14 - Math.min(100, Math.max(0, t[j])) / 100 * 8;
                if (j === 0)
                    g.moveTo(x, y);
                else
                    g.lineTo(x, y);
            }
            g.stroke();
        }
    }

    Accessible.role: Accessible.Button
    Accessible.name: "Scheduling: " + L.statusLabel(scdr, pending) + ", " + scheduler.name
    Accessible.onPressAction: togglePopup()

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        onClicked: root.togglePopup()
    }
}
