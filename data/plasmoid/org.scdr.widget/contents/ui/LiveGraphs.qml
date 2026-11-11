// Live graphs for the popup: how long the focused app waits to run, what
// the scheduler is doing, and system pressure. History is kept by main.qml
// (it keeps collecting while the popup is closed); this only draws it.
// Plain QtQuick + Kirigami + KQuickCharts, so it can be previewed outside
// Plasma (see tools/preview-graphs.qml).

import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.quickcharts as Charts
import org.kde.quickcharts.controls as ChartsControls

ColumnLayout {
    id: graphs

    // { name: [values, oldest first] }, all of length `length`.
    property var hist: ({})
    // Bumped by the owner after each change, so the charts redraw.
    property int revision: 0
    property int length: 180
    // Whether scx_devbox samples have arrived (its two graphs need them).
    property bool devbox: false

    spacing: Kirigami.Units.smallSpacing

    function series(name) {
        revision; // dependency: re-evaluated on every update
        return hist[name] ? hist[name].slice() : [];
    }
    function last(name) {
        revision;
        const a = hist[name];
        return a && a.length ? a[a.length - 1] : 0;
    }
    function peak(name) {
        revision;
        const a = hist[name];
        return a && a.length ? Math.max.apply(null, a) : 0;
    }
    function fmt(v, digits) {
        return Number(v).toFixed(digits);
    }

    readonly property real frameMs: 1000 / 60
    // The wait chart's scale: at least a frame and a bit, more for spikes.
    readonly property real waitTo: Math.max(20, Math.ceil(peak("wait_max") * 1.15))

    component Caption: QQC2.Label {
        Layout.fillWidth: true
        elide: Text.ElideRight
        font: Kirigami.Theme.smallFont
        opacity: 0.7
    }

    // Legends list each series' latest value, formatted, not the chart's raw first one.
    function legend(names, colors, values) {
        return names.map((n, i) => ({ name: n, color: colors[i], value: values[i] }));
    }

    // 1. Focused app: queued-to-running wait, worst and average per second.
    Caption {
        visible: graphs.devbox
        text: i18nc("@title graph", "Focused app: time waiting to run")
    }
    Item {
        visible: graphs.devbox
        Layout.fillWidth: true
        Layout.preferredHeight: Kirigami.Units.gridUnit * 3
        Charts.LineChart {
            id: waitChart
            anchors.fill: parent
            smooth: true
            fillOpacity: 0.2
            lineWidth: 1
            xRange { from: 0; to: graphs.length - 1; automatic: false }
            yRange { from: 0; to: graphs.waitTo; automatic: false }
            valueSources: [
                Charts.ArraySource { array: graphs.series("wait_max") },
                Charts.ArraySource { array: graphs.series("wait_avg") }
            ]
            colorSource: Charts.ArraySource {
                array: [Kirigami.Theme.negativeTextColor, Kirigami.Theme.highlightColor]
            }
            nameSource: Charts.ArraySource {
                array: [i18nc("@label", "Worst"), i18nc("@label", "Average")]
            }
        }
        // One frame at 60 Hz: waits above it can show as a stutter.
        Rectangle {
            width: parent.width
            height: 1
            y: parent.height * (1 - graphs.frameMs / graphs.waitTo)
            color: Kirigami.Theme.neutralTextColor
            opacity: 0.6
        }
        QQC2.Label {
            anchors.right: parent.right
            y: parent.height * (1 - graphs.frameMs / graphs.waitTo) - height
            text: i18nc("@label one frame at 60 Hz", "1 frame")
            font: Kirigami.Theme.smallFont
            color: Kirigami.Theme.neutralTextColor
        }
    }
    ChartsControls.Legend {
        visible: graphs.devbox
        Layout.fillWidth: true
        model: graphs.legend([i18nc("@label", "Worst"), i18nc("@label", "Average")],
                             waitChart.colorSource.array,
                             [i18nc("@info milliseconds", "%1 ms", graphs.fmt(graphs.last("wait_max"), 1)),
                              i18nc("@info milliseconds", "%1 ms", graphs.fmt(graphs.last("wait_avg"), 2))])
    }

    // 2. What the scheduler does with the work it gets, per second.
    Caption {
        visible: graphs.devbox
        Layout.topMargin: Kirigami.Units.smallSpacing
        text: i18nc("@title graph", "Tasks scheduled per second (%1 preempted)", graphs.last("preempt"))
    }
    Charts.LineChart {
        id: schedChart
        visible: graphs.devbox
        Layout.fillWidth: true
        Layout.preferredHeight: Kirigami.Units.gridUnit * 3
        smooth: true
        fillOpacity: 0.15
        lineWidth: 1
        xRange { from: 0; to: graphs.length - 1; automatic: false }
        yRange { from: 0; to: 100; automatic: true }
        valueSources: [
            Charts.ArraySource { array: graphs.series("interactive") },
            Charts.ArraySource { array: graphs.series("batch") },
            Charts.ArraySource { array: graphs.series("damped") }
        ]
        colorSource: Charts.ArraySource {
            array: [Kirigami.Theme.highlightColor, Kirigami.Theme.neutralTextColor, Kirigami.Theme.disabledTextColor]
        }
        nameSource: Charts.ArraySource {
            array: [i18nc("@label", "Interactive"), i18nc("@label", "Batch"),
                    i18nc("@label tasks of unfocused apps held back", "Held back")]
        }
    }
    ChartsControls.Legend {
        visible: graphs.devbox
        Layout.fillWidth: true
        model: graphs.legend(schedChart.nameSource.array, schedChart.colorSource.array,
                             [graphs.last("interactive"), graphs.last("batch"), graphs.last("damped")]
                                 .map(v => Math.round(v).toLocaleString(Qt.locale(), "f", 0)))
    }

    // 3. Pressure (PSI some avg10) and the CPU frequency target, in %.
    Caption {
        Layout.topMargin: Kirigami.Units.smallSpacing
        text: graphs.devbox ? i18nc("@title graph", "Pressure and CPU frequency target, %")
                            : i18nc("@title graph", "Pressure, %")
    }
    Charts.LineChart {
        id: pressureChart
        Layout.fillWidth: true
        Layout.preferredHeight: Kirigami.Units.gridUnit * 3
        smooth: true
        fillOpacity: 0.1
        lineWidth: 1
        xRange { from: 0; to: graphs.length - 1; automatic: false }
        yRange { from: 0; to: 100; automatic: false }
        valueSources: graphs.devbox ? [cpuSource, memSource, ioSource, perfSource]
                                    : [cpuSource, memSource, ioSource]
        colorSource: Charts.ArraySource {
            array: [Kirigami.Theme.highlightColor, Kirigami.Theme.negativeTextColor,
                    Kirigami.Theme.neutralTextColor, Kirigami.Theme.positiveTextColor]
        }
        nameSource: Charts.ArraySource {
            array: [i18nc("@label", "CPU"), i18nc("@label", "Memory"), i18nc("@label", "IO"),
                    i18nc("@label CPU frequency target", "Frequency")]
        }
        Charts.ArraySource { id: cpuSource; array: graphs.series("cpu") }
        Charts.ArraySource { id: memSource; array: graphs.series("memory") }
        Charts.ArraySource { id: ioSource; array: graphs.series("io") }
        Charts.ArraySource { id: perfSource; array: graphs.series("perf") }
    }
    ChartsControls.Legend {
        Layout.fillWidth: true
        model: graphs.legend(pressureChart.nameSource.array.slice(0, graphs.devbox ? 4 : 3),
                             pressureChart.colorSource.array,
                             [graphs.last("cpu"), graphs.last("memory"), graphs.last("io"), graphs.last("perf")]
                                 .map(v => i18nc("@info percent", "%1%", Math.round(v))))
    }
}
