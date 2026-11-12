// A number that can be left unset: unticked, the box shows what applies instead (`fallback`)
// and reports undefined. `decimals` scales the spin box for fractional values. A change is
// reported 800 ms after the last step, so a few clicks make one change; like ChoiceBox, it
// shows `value` again after reporting, until the page changes it (`revision` re-reads it).
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

RowLayout {
    id: row

    property var value
    property var fallback
    property real from: 0
    property real to: 100
    property int decimals: 0
    property string suffix: ""
    property int revision: 0

    signal chosen(var value)

    readonly property bool isSet: row.value !== undefined && row.value !== null
    readonly property bool hasFallback: typeof row.fallback === "number"
    readonly property real factor: Math.pow(10, row.decimals)
    readonly property alias check: set
    readonly property alias spin: spinBox

    // Where a newly set value starts: the fallback, else 0 within range.
    function start() {
        return row.hasFallback ? row.fallback : Math.min(Math.max(0, row.from), row.to);
    }

    function sync() {
        set.checked = row.isSet;
        spinBox.value = Math.round((row.isSet ? row.value : row.start()) * row.factor);
    }

    onValueChanged: later.restart()
    onFallbackChanged: later.restart()
    onRevisionChanged: later.restart()
    Component.onCompleted: row.sync()
    spacing: 4

    QQC2.CheckBox {
        id: set
        text: "Set"
        enabled: row.enabled
        onToggled: {
            const on = set.checked;
            settle.stop();
            later.restart();
            row.chosen(on ? row.start() : undefined);
        }
    }
    QQC2.SpinBox {
        id: spinBox
        enabled: row.enabled && set.checked
        editable: true
        from: Math.round(row.from * row.factor)
        to: Math.round(row.to * row.factor)
        textFromValue: (v, locale) => !set.checked && !row.hasFallback ? "default" : (v / row.factor).toFixed(row.decimals) + row.suffix
        valueFromText: (text, locale) => Math.round(Number(text.replace(row.suffix, "").trim()) * row.factor)
        onValueModified: settle.restart()
    }
    Timer {
        id: settle
        interval: 800
        onTriggered: {
            later.restart();
            row.chosen(row.decimals > 0 ? spinBox.value / row.factor : spinBox.value);
        }
    }
    // Runs sync() once things settle; dies with the control, unlike Qt.callLater.
    Timer {
        id: later
        interval: 0
        onTriggered: row.sync()
    }
}
