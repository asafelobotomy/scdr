// The widget's popup: scdr's state with Pause/Resume, the per-role switches, the live graphs
// (ported from the Plasma widget's LiveGraphs.qml) and what is boosted now. `w` is Widget.qml's
// root, which keeps the state and the history.
import QtQuick
import "logic.js" as L

Column {
    id: details

    // Set by the shell after loading (the same ctx as the widget's); `w.ctx` is used instead, as
    // it is there from the start.
    property var ctx
    property var w
    readonly property var c: w ? w.ctx : null
    readonly property var t: c ? c.Theme : null
    readonly property var s: w ? w.scdr : null
    readonly property var h: w ? w.hist : null
    readonly property int rev: w ? w.revision : 0
    readonly property bool devbox: h !== null && rev >= 0 && h.devbox
    readonly property int small: t ? t.fontSize - 1 : 10
    readonly property var boostedRows: L.grouped(s ? s.boosted : [])
    readonly property int maxBoosted: 6

    width: 340
    spacing: 8

    function last(key) {
        return rev >= 0 && h ? L.last(h, key) : 0;
    }

    // ---- Header: state, scheduler, Pause/Resume ---------------------------------------------
    Item {
        width: details.width
        height: Math.max(heading.implicitHeight, pauseButton.implicitHeight)
        Column {
            id: heading
            anchors.left: parent.left
            anchors.right: pauseButton.left
            anchors.rightMargin: 8
            anchors.verticalCenter: parent.verticalCenter
            spacing: 1
            Caption {
                ctx: details.c
                width: parent.width
                text: "Scheduling"
                bold: true
                size: details.t ? details.t.fontSize + 2 : 13
            }
            Caption {
                ctx: details.c
                width: parent.width
                text: details.w ? L.statusLabel(details.s, details.w.pending) : ""
                color: !details.t ? "grey" : details.s && details.s.paused ? details.t.warning : details.t.success
            }
        }
        PillButton {
            id: pauseButton
            objectName: "pause"
            ctx: details.c
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            visible: details.s !== null
            enabled: details.w !== null && details.w.busy === "" && details.w.pending === null
            primary: details.s !== null && details.s.paused
            text: details.s && details.s.paused ? "Resume" : "Pause"
            onClicked: details.w.togglePause()
        }
    }

    Grid {
        columns: 2
        columnSpacing: 12
        rowSpacing: 3
        width: details.width
        visible: details.s !== null

        Caption { ctx: details.c; width: 90; text: "Scheduler"; color: details.t ? details.t.textMuted : "grey" }
        Column {
            width: details.width - 102
            Caption {
                ctx: details.c
                objectName: "scheduler"
                width: parent.width
                text: details.w ? details.w.scheduler.name : ""
            }
            Caption {
                ctx: details.c
                width: parent.width
                visible: text !== ""
                text: details.w ? details.w.scheduler.note : ""
                color: details.t ? details.t.textMuted : "grey"
                size: details.small
            }
        }
        Caption { ctx: details.c; width: 90; text: "Power profile"; color: details.t ? details.t.textMuted : "grey" }
        Caption {
            ctx: details.c
            width: details.width - 102
            text: details.s && details.s.powerProfile !== "" ? details.s.powerProfile : "–"
        }
        Caption { ctx: details.c; width: 90; text: "CPU pressure"; color: details.t ? details.t.textMuted : "grey" }
        Caption { ctx: details.c; width: details.width - 102; text: L.pressureLine(details.s) }
        Caption { ctx: details.c; width: 90; text: "Managed"; color: details.t ? details.t.textMuted : "grey" }
        Caption {
            ctx: details.c
            width: details.width - 102
            text: details.s ? details.s.managed + (details.s.managed === 1 ? " process" : " processes") : "–"
        }
    }

    // ---- Roles --------------------------------------------------------------------------------
    Caption {
        ctx: details.c
        visible: details.s !== null && details.s.roles.length > 0
        text: "Boosts"
        bold: true
    }
    Repeater {
        model: details.s ? details.s.roles : []
        delegate: Item {
            required property var modelData
            width: details.width
            height: Math.max(roleName.implicitHeight, roleSwitch.implicitHeight)
            Caption {
                id: roleName
                ctx: details.c
                anchors.left: parent.left
                anchors.right: roleSwitch.left
                anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                text: L.roleLabel(modelData.role) + "  (" + modelData.profile + ")"
            }
            Toggle {
                id: roleSwitch
                objectName: "role-" + modelData.role
                ctx: details.c
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                name: L.roleLabel(modelData.role)
                checked: modelData.enabled
                enabled: details.s !== null && !details.s.paused && details.w.busy === "" && details.w.pending === null
                onToggled: function (on) { details.w.setRole(modelData.role, on); }
            }
        }
    }
    Caption {
        ctx: details.c
        width: details.width
        visible: details.s !== null && details.s.agents === 0
        wrap: true
        size: details.small
        color: details.t ? details.t.textMuted : "grey"
        text: "No session agent: the focus, audio and game boosts need systemctl --user enable --now scdr-session"
    }

    // ---- Live graphs ----------------------------------------------------------------------------
    Caption { ctx: details.c; visible: details.s !== null; text: "Live"; bold: true }
    Graph {
        objectName: "wait-graph"
        visible: details.devbox
        width: details.width
        ctx: details.c
        hist: details.h
        revision: details.rev
        title: "Focused app: time waiting to run"
        keys: ["wait_max", "wait_avg"]
        colors: details.t ? [details.t.error, details.t.accent] : []
        names: ["Worst", "Average"]
        values: [L.fixed(details.last("wait_max"), 1) + " ms", L.fixed(details.last("wait_avg"), 2) + " ms"]
        yMax: details.rev >= 0 && details.h ? L.waitScale(details.h) : 20
        // One frame at 60 Hz: waits above it can show as a stutter.
        line: L.FRAME_MS
        lineLabel: "1 frame"
    }
    Graph {
        objectName: "sched-graph"
        visible: details.devbox
        width: details.width
        ctx: details.c
        hist: details.h
        revision: details.rev
        title: "Tasks scheduled per second (" + Math.round(details.last("preempt")) + " preempted)"
        keys: ["interactive", "batch", "damped"]
        colors: details.t ? [details.t.accent, details.t.warning, details.t.textMuted] : []
        names: ["Interactive", "Batch", "Held back"]
        values: [details.last("interactive"), details.last("batch"), details.last("damped")].map(function (v) { return String(Math.round(v)); })
        yMax: details.rev >= 0 && details.h ? L.schedScale(details.h) : 10
    }
    Graph {
        objectName: "pressure-graph"
        visible: details.s !== null
        width: details.width
        ctx: details.c
        hist: details.h
        revision: details.rev
        title: details.devbox ? "Pressure and CPU frequency target, %" : "Pressure, %"
        keys: details.devbox ? ["cpu", "memory", "io", "perf"] : ["cpu", "memory", "io"]
        colors: details.t ? [details.t.accent, details.t.error, details.t.warning, details.t.success] : []
        names: details.devbox ? ["CPU", "Memory", "IO", "Frequency"] : ["CPU", "Memory", "IO"]
        values: [details.last("cpu"), details.last("memory"), details.last("io"), details.last("perf")].map(function (v) { return Math.round(v) + "%"; })
        yMax: 100
    }

    // ---- Boosted now --------------------------------------------------------------------------
    Caption { ctx: details.c; visible: details.s !== null; text: "Boosted now"; bold: true }
    Repeater {
        model: details.boostedRows.slice(0, details.maxBoosted)
        delegate: Caption {
            required property var modelData
            ctx: details.c
            width: details.width
            text: L.boostedLine(modelData)
        }
    }
    Caption {
        ctx: details.c
        width: details.width
        visible: details.s !== null && (details.boostedRows.length === 0 || details.boostedRows.length > details.maxBoosted)
        color: details.t ? details.t.textMuted : "grey"
        text: details.boostedRows.length > details.maxBoosted
              ? "and " + (details.boostedRows.length - details.maxBoosted) + " more"
              : (details.s && details.s.paused ? "Paused" : "Nothing boosted right now")
    }

    Caption {
        objectName: "problem"
        ctx: details.c
        width: details.width
        visible: details.w !== null && details.w.problem !== ""
        wrap: true
        color: details.t ? details.t.error : "red"
        text: details.w ? details.w.problem : ""
    }
}
