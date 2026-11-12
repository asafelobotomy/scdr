// The bar widget (uq/widgets/scdr/) offscreen with a fake widget ctx; no uQ needed:
//   QT_QPA_PLATFORM=offscreen qmltestrunner -import tests/uq/stub -input tests/uq
import QtQuick
import QtTest

Item {
    id: root
    width: 400
    height: 800

    readonly property url widgetUrl: Qt.resolvedUrl("../../uq/widgets/scdr/Widget.qml")

    // ---- A stand-in for uQ's widget ctx (shell/widgets/WidgetContext.qml) ----------------
    QtObject {
        id: theme
        property color bg: "#1f1f22"
        property color surface: "#2b2b2e"
        property color surfaceAlt: "#353538"
        property color overlay: "#464649"
        property color text: "#e4e4e7"
        property color textMuted: "#a9a9ac"
        property color accent: "#8ab4f8"
        property color accentText: "#1f1f22"
        property color border: "#5a5a5d"
        property color success: "#81c995"
        property color warning: "#fdd663"
        property color error: "#f28b82"
        property int radius: 10
        property int fontSize: 11
        property string fontUi: "sans"
    }
    Component {
        id: uqText
        Text {
            textFormat: Text.PlainText
            elide: Text.ElideRight
        }
    }
    QtObject {
        id: fake
        property var props: ({})
        property var handlers: []
        property var calls: []
        // method -> {value} | {error}
        property var answers: ({})
        property string stateText: ""
        property int popups: 0
        property int closes: 0
        // Bumped on each load: answers for an older load are dropped, as uQ does.
        property int generation: 0

        function emit(text) {
            stateText = text;
            props = { State: text };
            for (var i = 0; i < handlers.length; i++)
                handlers[i]({ State: text });
        }
    }
    readonly property var ctx: ({
        Theme: theme,
        theme: theme,
        UqText: uqText,
        UqInput: null,
        slot: "bar",
        dbus: {
            get props() { return fake.props; },
            call: function (method, args, cb) {
                fake.calls = fake.calls.concat([{ method: method, args: args }]);
                var a = fake.answers[method] || { value: null }, gen = fake.generation;
                Qt.callLater(function () { if (gen === fake.generation) cb(a); });
            },
            get: function (cb) {
                fake.calls = fake.calls.concat([{ method: "get" }]);
                var gen = fake.generation;
                Qt.callLater(function () {
                    if (gen === fake.generation)
                        cb(fake.stateText ? { value: { State: fake.stateText } } : { error: "not running" });
                });
            },
            onProps: function (cb) { fake.handlers = fake.handlers.concat([cb]); },
            onSignal: function (cb) {}
        },
        command: { run: function (id, args, cb) { cb({ error: "none" }); } },
        openPopup: function () { fake.popups++; },
        closePopup: function () { fake.closes++; },
        fail: function (reason) { console.warn("widget failed: " + reason); }
    })

    function stateDoc(over) {
        return JSON.stringify(Object.assign({
            paused: false,
            managed: 29,
            power_profile: "performance",
            cpu_pressure: 12.5,
            pressure_active: false,
            scheduler: ["sched_ext: devbox (started outside scdr; left alone)", "eevdf: base_slice=1600us preempt=full"],
            roles: [["game", "game", true], ["audio", "audio-client", true], ["foreground", "foreground", true], ["background", "background", false]],
            agents: 1,
            boosted: [{ pid: 10, name: "firefox", profile: "foreground", role: "foreground", why: "focused", app: "firefox" }],
            memory_pressure: 0.5,
            io_pressure: 2.0,
            devbox: [
                { seq: 1, time: 1, interactive: 90, batch: 3, focus: 2, damped: 1, preempt: 4, rescue: 0, perf_pct: 66, wait_max_ms: 1.5, wait_avg_ms: 0.03 },
                { seq: 2, time: 2, interactive: 80, batch: 2, focus: 1, damped: 0, preempt: 2, rescue: 0, perf_pct: 70, wait_max_ms: 2.5, wait_avg_ms: 0.04 }
            ]
        }, over || {}));
    }

    Loader {
        id: loader
        width: 26
        height: 26
    }
    Loader {
        id: popupLoader
        y: 40
    }

    function walk(item, name) {
        if (!item)
            return null;
        if (item.objectName === name)
            return item;
        for (var i = 0; i < item.children.length; i++) {
            var f = walk(item.children[i], name);
            if (f)
                return f;
        }
        return null;
    }

    TestCase {
        name: "ScdrWidget"
        when: windowShown

        function init() {
            fake.props = {};
            fake.handlers = [];
            fake.calls = [];
            fake.answers = {};
            fake.popups = 0;
            fake.closes = 0;
            fake.generation++;
            fake.stateText = root.stateDoc();
            popupLoader.sourceComponent = null;
            loader.source = "";
            loader.setSource(root.widgetUrl, { ctx: root.ctx });
            compare(loader.status, Loader.Ready, "the widget loads");
            tryVerify(function () { return loader.item.scdr !== null; });
        }

        function openPopup() {
            mouseClick(loader.item);
            compare(fake.popups, 1);
            popupLoader.sourceComponent = loader.item.popup;
            compare(popupLoader.status, Loader.Ready, "the popup loads");
            popupLoader.item.ctx = root.ctx;
            return popupLoader.item;
        }

        function test_reads_state_and_history() {
            var w = loader.item;
            compare(w.scheduler.name, "scx_devbox");
            compare(w.hist.lastSeq, 2);
            compare(w.hist.data.wait_max[w.hist.length - 1], 2.5);
            compare(w.hist.data.cpu[w.hist.length - 1], 12.5);
            // The same samples again add nothing; a new one is appended.
            var rev = w.revision;
            fake.emit(root.stateDoc());
            compare(w.revision, rev);
            fake.emit(root.stateDoc({ devbox: [{ seq: 3, interactive: 5, perf_pct: 50, wait_max_ms: 0.2, wait_avg_ms: 0.01 }] }));
            compare(w.revision, rev + 1);
            compare(w.hist.data.interactive[w.hist.length - 1], 5);
            compare(w.hist.data.interactive.length, w.hist.length);
        }

        function test_popup_shows_state() {
            var p = openPopup();
            compare(walk(p, "scheduler").text, "scx_devbox");
            verify(walk(p, "wait-graph").visible);
            verify(walk(p, "pressure-graph").visible);
            compare(walk(p, "pause").text, "Pause");
            verify(walk(p, "role-background") !== null);
        }

        function test_popup_keeps_its_size_and_closes() {
            var p = openPopup();
            var h = p.height;
            // Rows coming and going (more boosted apps) must not resize the popup window.
            var many = [];
            for (var i = 0; i < 12; i++)
                many.push({ pid: 100 + i, name: "app" + i, profile: "foreground", why: "focus" });
            fake.emit(root.stateDoc({ boosted: many }));
            compare(p.height, h);
            mouseClick(walk(p, "close"));
            compare(fake.closes, 1);
        }

        function test_pause_and_role() {
            var p = openPopup();
            mouseClick(walk(p, "pause"));
            tryVerify(function () { return fake.calls.some(function (c) { return c.method === "Pause"; }); });
            tryVerify(function () { return loader.item.busy === ""; });
            mouseClick(walk(p, "role-background"));
            tryVerify(function () {
                return fake.calls.some(function (c) { return c.method === "SetRoleEnabled" && c.args[0] === "background" && c.args[1] === true; });
            });
        }

        function test_waiting_for_password() {
            fake.answers = { Pause: { error: "no answer within 2 s" } };
            var w = loader.item;
            w.togglePause();
            tryVerify(function () { return w.pending !== null; });
            verify(!walk(openPopup(), "pause").enabled);
            fake.emit(root.stateDoc({ paused: true, devbox: [] }));
            compare(w.pending, null);
            verify(w.paused);
            compare(walk(popupLoader.item, "pause").text, "Resume");
        }

        function test_refused() {
            fake.answers = { Resume: { error: "org.freedesktop.DBus.Error.AccessDenied: not authorized" } };
            fake.emit(root.stateDoc({ paused: true }));
            var w = loader.item;
            w.togglePause();
            tryVerify(function () { return w.problem !== ""; });
            verify(w.problem.indexOf("Not allowed") === 0);
        }
    }
}
