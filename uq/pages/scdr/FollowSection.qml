// Whether the focus agent follows the focused window.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "logic.js" as Logic

ColumnLayout {
    required property var host
    Layout.fillWidth: true
    spacing: 6

    SwitchRow {
        settingId: "follow-focus"
        label: "Follow the focused window"
        highlight: host.highlight
        value: host.agentOn
        enabled: host.agentKnown && host.busy === ""
        Layout.fillWidth: true
        onChosen: on => host.setAgent(on)
    }
    UqLabel {
        visible: !host.agentKnown
        text: "This can't be changed here right now."
        color: host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    UqLabel {
        visible: host.working("agent")
        text: "Working…"
        color: host.mutedColor
    }
    UqLabel {
        visible: host.agentOn && host.ready && host.view.state.agents === 0
        text: "Not following any window yet. It starts with your next login, or now if your session is running."
        color: host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    UqLabel {
        visible: host.problem("agent") !== ""
        text: host.problem("agent")
        color: host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
