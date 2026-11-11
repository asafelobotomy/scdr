// The top of the page: running or paused, CPU load, Pause and Resume, Start scdr.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "logic.js" as Logic

ColumnLayout {
    required property var host
    Layout.fillWidth: true
    spacing: 6

    SettingRow {
        settingId: "status"
        label: host.view.status === "ready" && !host.settingsDown
            ? (host.view.state.paused ? "scdr is paused" : "scdr is running")
            : (host.down ? "scdr isn't running" : "Checking…")
        highlight: host.highlight
        Layout.fillWidth: true
        QQC2.Button {
            visible: host.ready
            enabled: host.busy === ""
            text: host.ready && host.view.state.paused ? "Resume" : "Pause"
            onClicked: host.ready && host.view.state.paused
                ? host.write("status", "Resume", [])
                : host.confirmDialog.ask(Logic.confirmText("pause"), () => host.write("status", "Pause", []))
        }
        QQC2.Button {
            visible: host.down
            enabled: host.busy === ""
            text: "Start scdr"
            onClicked: host.start()
        }
    }
    UqLabel {
        visible: host.ready
        text: [Logic.pressureText(host.view.state), Logic.managedText(host.view.state)].filter(s => s).join(" · ")
        color: host.mutedColor
        Layout.fillWidth: true
    }
    UqLabel {
        visible: host.down
        text: "scdr changes how much CPU time each app gets. Start it to use this page."
        color: host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    UqLabel {
        visible: host.view.status === "ready" && !host.settingsDown && host.settingsError !== ""
        text: "scdr's settings couldn't be read."
        color: host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    ErrorCard {
        visible: host.view.status === "error"
        title: host.view.message || ""
        detail: "Try again in a moment."
        Layout.fillWidth: true
    }
    UqLabel {
        visible: host.working("status") || host.working("start")
        text: "Working…"
        color: host.mutedColor
    }
    UqLabel {
        visible: host.problem("status") !== "" || host.problem("start") !== ""
        text: host.problem("status") || host.problem("start")
        color: host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
