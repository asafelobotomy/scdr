// The schedulers scdr knows, with a trial run of each, and the system's installer for more
// (`scdrctl schedulers which|install|update|git`, which ask for a password themselves).
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "schedulers.js" as Schedulers
import "words.js" as Words

ColumnLayout {
    id: section

    required property var host
    readonly property var s: host.settings
    property string installer: ""
    property bool installing: false
    property string installStatus: ""

    visible: host.ready && s !== null
    Layout.fillWidth: true
    spacing: 6

    // Which installer `scdrctl schedulers` would use; asked once the page has its context.
    function findInstaller() {
        section.host.ctx.command.run("installer-which", [], section.host.live(result => {
            section.installer = Words.installerOf(result);
        }));
    }

    function install(id) {
        section.installing = true;
        section.installStatus = "Waiting for the installer…";
        section.host.ctx.command.run(id, [], section.host.live(result => {
            section.installing = false;
            section.installStatus = Words.installResult(result);
            section.host.refresh();
        }));
    }

    SettingRow {
        settingId: "available-schedulers"
        label: "Available schedulers"
        highlight: section.host.highlight
        Layout.fillWidth: true
    }
    UqLabel {
        visible: text !== ""
        text: Words.loaderText(section.s)
        color: section.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    Repeater {
        model: section.s ? section.s.cards : []
        delegate: ColumnLayout {
            id: card
            required property var modelData
            Layout.fillWidth: true
            Layout.topMargin: 4
            spacing: 2
            RowLayout {
                Layout.fillWidth: true
                UqLabel {
                    text: card.modelData.name
                    font.bold: true
                }
                UqLabel {
                    visible: !card.modelData.installed
                    text: "not installed"
                    color: section.host.mutedColor
                }
                Item {
                    Layout.fillWidth: true
                }
                UqComboBox {
                    id: mode
                    objectName: "mode-" + card.modelData.name
                    visible: card.modelData.modes.length > 1
                    model: card.modelData.modes
                }
                QQC2.Button {
                    objectName: "try-" + card.modelData.name
                    text: "Try"
                    enabled: card.modelData.installed && section.host.busy === ""
                    onClicked: {
                        const arg = Schedulers.tryArg(card.modelData.name, mode.visible ? mode.currentText : "auto");
                        if (arg !== "")
                            section.host.write("try", "TryScheduler", [arg]);
                    }
                }
            }
            UqLabel {
                text: Words.cardText(card.modelData)
                color: section.host.mutedColor
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }
    }
    UqLabel {
        text: "Try runs a scheduler now without saving it; stop the trial to go back."
        color: section.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    AreaStatus {
        host: section.host
        areas: ["try"]
    }
    RowLayout {
        Layout.fillWidth: true
        Layout.topMargin: 6
        UqLabel {
            text: Words.installerLabel(section.installer)
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        QQC2.Button {
            objectName: "install"
            text: "Install"
            enabled: section.installer !== "" && !section.installing
            onClicked: section.install("installer-install")
        }
        QQC2.Button {
            objectName: "update"
            text: "Update"
            enabled: section.installer !== "" && !section.installing
            onClicked: section.install("installer-update")
        }
        QQC2.Button {
            objectName: "install-git"
            text: "Latest from Git"
            enabled: (section.installer === "pamac" || section.installer === "terminal") && !section.installing
            onClicked: section.install("installer-git")
        }
    }
    UqLabel {
        visible: section.installStatus !== ""
        text: section.installStatus
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
