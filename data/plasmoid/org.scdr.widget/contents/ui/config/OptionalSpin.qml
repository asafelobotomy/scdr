// A number that can be left unset: unchecked shows the fallback value and
// reports undefined. `decimals` scales the spin box for fractional values.

import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2

RowLayout {
    id: row

    property var value
    property var fallback
    property real from: 0
    property real to: 100
    property int decimals: 0
    property string suffix: ""

    signal edited(var value)

    readonly property real scale: Math.pow(10, decimals)

    // Where a newly set value starts: the fallback, else 0 within range.
    function start() {
        return fallback !== undefined && fallback !== null ? fallback : Math.min(Math.max(0, from), to);
    }

    QQC2.CheckBox {
        id: set
        checked: row.value !== undefined && row.value !== null
        onToggled: row.edited(checked ? row.start() : undefined)
        QQC2.ToolTip.text: checked ? i18n("Set here") : i18n("Not set: uses the value from the files below")
        QQC2.ToolTip.visible: hovered
    }
    QQC2.SpinBox {
        enabled: set.checked
        from: Math.round(row.from * row.scale)
        to: Math.round(row.to * row.scale)
        editable: true
        value: {
            const v = set.checked ? row.value : row.fallback;
            return Math.round((v === undefined || v === null ? row.start() : v) * row.scale);
        }
        textFromValue: (v, locale) => !set.checked && (row.fallback === undefined || row.fallback === null)
            ? i18n("default")
            : Number(v / row.scale).toLocaleString(locale, "f", row.decimals) + row.suffix
        valueFromText: (text, locale) => Math.round(Number.fromLocaleString(locale, text.replace(row.suffix, "").trim()) * row.scale)
        onValueModified: row.edited(row.decimals > 0 ? value / row.scale : value)
    }
}
