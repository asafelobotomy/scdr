// The latency benchmark: it measures how late an interactive thread wakes while every CPU is
// busy, with and without scdr's levers. It needs scdr stopped or paused, so `bench --check` is
// read first; the run streams its progress (`--ndjson`).
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "plans.js" as Plans

ColumnLayout {
    id: section

    required property var host
    property var gate: ({ can: false, reason: "", seconds: 0, known: false })
    property var progress: null
    property bool running: false
    property string problem: ""

    function readGate() {
        if (!section.host.ctx)
            return;
        section.host.ctx.command.run("bench-check", [], section.host.live(result => {
            const g = Plans.benchCheck(result);
            section.gate = { can: g.can, reason: g.reason, seconds: g.seconds, known: true };
        }));
    }

    function run() {
        if (section.running || !section.gate.can || !section.host.ctx)
            return;
        section.problem = "";
        section.progress = Plans.startBench();
        section.running = true;
        section.host.ctx.command.stream("bench", [], section.host.live(ev => {
            section.progress = Plans.benchReduce(section.progress, ev);
        }), section.host.live(result => {
            section.running = false;
            const env = Plans.envelope(result);
            if (env.cancelled)
                section.progress = null;
            else if (!env.ok)
                section.problem = env.error;
            section.readGate();
        }));
    }

    Layout.fillWidth: true
    spacing: 6
    Component.onCompleted: Qt.callLater(section.readGate)

    SettingRow {
        settingId: "bench"
        label: "How quickly an app wakes up under load"
        highlight: section.host.highlight
        Layout.fillWidth: true
        QQC2.Button {
            objectName: "bench-run"
            text: "Run the benchmark"
            enabled: section.gate.can && !section.running
            onClicked: section.run()
        }
    }
    UqLabel {
        visible: section.gate.known && !section.gate.can
        text: section.gate.reason
        color: section.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    UqLabel {
        visible: section.gate.known && section.gate.can && section.progress === null
        text: "It takes about " + section.gate.seconds + " seconds and runs every CPU flat out, so the screen may stutter."
        color: section.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    QQC2.ProgressBar {
        objectName: "bench-bar"
        visible: section.progress !== null && !section.progress.done
        from: 0
        to: 100
        value: section.progress !== null ? section.progress.percent : 0
        Layout.fillWidth: true
    }
    UqLabel {
        visible: section.progress !== null
        text: section.progress !== null ? section.progress.line : ""
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    UqLabel {
        visible: section.problem !== ""
        text: section.problem
        color: section.host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    Repeater {
        model: section.progress !== null ? section.progress.results : []

        RowLayout {
            id: line

            required property var modelData

            Layout.fillWidth: true
            UqLabel {
                text: line.modelData.name
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            UqLabel {
                text: "median " + line.modelData.p50 + " · 99% " + line.modelData.p99 + " · worst " + line.modelData.max
                color: section.host.mutedColor
            }
        }
    }
}
