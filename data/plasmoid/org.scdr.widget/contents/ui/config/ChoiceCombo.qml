// A combo box over [{text, value}] choices; `value: null` means "not set".

import QtQuick
import QtQuick.Controls as QQC2

QQC2.ComboBox {
    id: combo

    property var choices: []
    property var current: null

    signal picked(var value)

    function indexOf(v) {
        const want = v === undefined ? null : v;
        for (let i = 0; i < choices.length; i++) {
            if (choices[i].value === want) {
                return i;
            }
        }
        return -1;
    }

    implicitContentWidthPolicy: QQC2.ComboBox.WidestTextWhenCompleted
    model: choices.map(c => c.text)
    currentIndex: indexOf(current)
    onActivated: index => picked(choices[index].value)
}
