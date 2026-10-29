// Status line shared by the settings pages: errors, saving, and where the
// settings live.

import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: banner

    required property var store
    property string saved: ""

    Layout.fillWidth: true

    Connections {
        target: banner.store
        function onSaved(summary) {
            banner.saved = summary;
        }
    }

    Kirigami.InlineMessage {
        Layout.fillWidth: true
        type: Kirigami.MessageType.Error
        visible: banner.store.error !== ""
        text: banner.store.error
    }
    Kirigami.InlineMessage {
        Layout.fillWidth: true
        type: Kirigami.MessageType.Positive
        visible: banner.saved !== "" && !banner.store.dirty && banner.store.error === ""
        text: i18n("Saved and applied: %1", banner.saved)
    }
    Kirigami.InlineMessage {
        Layout.fillWidth: true
        type: Kirigami.MessageType.Information
        visible: !banner.store.loaded && banner.store.error === ""
        text: i18n("Loading scdrd's settings…")
    }
}
