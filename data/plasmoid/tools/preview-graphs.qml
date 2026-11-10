// Harness for preview-graphs.sh: LiveGraphs with three minutes of
// plausible made-up data (a build starting, a stutter, memory pressure).
import QtQuick
import QtQuick.Controls
import org.kde.kirigami as Kirigami

ApplicationWindow {
    id: win
    width: 380
    height: 560
    visible: true
    color: Kirigami.Theme.backgroundColor

    readonly property int n: 180

    function make(f) {
        const a = [];
        for (let i = 0; i < n; i++) a.push(f(i));
        return a;
    }
    function noise(i, k) { return (Math.sin(i * 12.9898 + k * 78.233) * 43758.5453) % 1 * 0.5 + 0.5; }

    Rectangle {
        id: frame
        anchors.fill: parent
        color: Kirigami.Theme.backgroundColor

    LiveGraphs {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.largeSpacing
        devbox: true
        revision: 1
        length: win.n
        hist: ({
            wait_max: win.make(i => (i > 90 ? 3 : 1.5) + win.noise(i, 1) * 4 + (i === 130 ? 34 : 0) + (i === 131 ? 18 : 0)),
            wait_avg: win.make(i => 0.05 + win.noise(i, 2) * (i > 90 ? 0.4 : 0.15)),
            interactive: win.make(i => 4000 + win.noise(i, 3) * 3000 + (i > 90 ? 6000 : 0)),
            batch: win.make(i => i > 90 ? 5000 + win.noise(i, 4) * 4000 : 100 * win.noise(i, 5)),
            damped: win.make(i => i > 100 ? 1500 * win.noise(i, 6) : 0),
            preempt: win.make(i => i > 90 ? 900 : 4),
            perf: win.make(i => i > 90 ? 95 + win.noise(i, 7) * 5 : 55 + win.noise(i, 8) * 20),
            cpu: win.make(i => i > 90 ? 30 + win.noise(i, 9) * 15 : 3),
            memory: win.make(i => i > 140 ? 12 + win.noise(i, 10) * 6 : 0),
            io: win.make(i => i > 145 ? 8 * win.noise(i, 11) : 0.5)
        })
    }
    }

    Timer {
        interval: 1500
        running: true
        onTriggered: frame.grabToImage(result => {
            result.saveToFile(Qt.resolvedUrl("graphs-preview.png").toString().replace("file://", ""));
            Qt.quit();
        })
    }
}
