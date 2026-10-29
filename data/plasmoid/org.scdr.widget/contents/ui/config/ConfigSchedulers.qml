// Schedulers: which sched_ext scheduler scdr runs (by default, while
// gaming, per power profile), EEVDF's knobs, and the schedulers available,
// with a trial run and the system's installer for more.

import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.kcmutils as KCM
import org.kde.plasma.plasma5support as P5Support

KCM.SimpleKCM {
    id: page

    property SettingsStore store: SettingsStore {}
    property bool unsavedChanges: store.dirty
    property string installer: ""
    property string installStatus: ""

    function saveConfig() {
        store.save();
    }

    readonly property var schedulers: (store.revision, store.info.schedulers || [])
    readonly property var profiles: store.info.power_profiles || []

    function schedLabel(v) {
        if (v === undefined || v === null || v === "eevdf") {
            return i18n("Kernel default (EEVDF)");
        }
        const bare = v.startsWith("scx_") ? v.slice(4) : v;
        return bare.endsWith(":auto") ? bare.slice(0, -5) : bare;
    }

    // Every installed scheduler in every mode scx_loader configures.
    function schedChoices(fallback) {
        const out = [{ text: i18n("Not set (%1)", schedLabel(fallback)), value: null },
                     { text: schedLabel("eevdf"), value: "eevdf" }];
        for (const s of schedulers) {
            if (!s.installed) {
                continue;
            }
            for (const m of s.modes) {
                out.push({ text: m === "auto" ? s.name : s.name + ":" + m, value: m === "auto" ? s.name : s.name + ":" + m });
            }
        }
        return out;
    }

    function withCurrent(choices, v) {
        if (v !== undefined && v !== null && !choices.some(c => c.value === v)) {
            choices.push({ text: i18n("%1 (not installed)", schedLabel(v)), value: v });
        }
        return choices;
    }

    readonly property var preemptChoices: [
        { text: i18n("Not set"), value: null },
        { text: i18n("none"), value: "none" },
        { text: i18n("voluntary"), value: "voluntary" },
        { text: i18n("full"), value: "full" },
        { text: i18n("lazy"), value: "lazy" },
    ]

    P5Support.DataSource {
        id: exec
        engine: "executable"
        connectedSources: []
        onNewData: (source, data) => {
            disconnectSource(source);
            if (source.endsWith(" which")) {
                page.installer = (data.stdout || "").trim();
                return;
            }
            page.installStatus = data["exit code"] === 0
                ? i18n("Done.")
                : (data.stderr || data.stdout || i18n("The installer exited with an error.")).trim();
            page.store.load();
        }
    }

    function runInstaller(action) {
        installStatus = i18n("Waiting for the installer…");
        exec.connectSource("scdrctl schedulers " + action);
    }

    Component.onCompleted: exec.connectSource("scdrctl schedulers which")

    ColumnLayout {
        spacing: Kirigami.Units.largeSpacing

        SettingsBanner {
            store: page.store
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            type: Kirigami.MessageType.Warning
            visible: !!page.store.info.trial
            text: page.store.info.trial
                  ? i18n("Trying %1 (was %2). It isn't saved; stop the trial to go back.",
                         page.schedLabel(page.store.info.trial.trying), page.schedLabel(page.store.info.trial.was))
                  : ""
            actions: Kirigami.Action {
                text: i18n("Stop trying")
                icon.name: "media-playback-stop"
                onTriggered: page.store.call("EndSchedulerTrial", [], () => page.store.load())
            }
        }

        Kirigami.FormLayout {
            Layout.fillWidth: true

            Kirigami.Separator {
                Kirigami.FormData.isSection: true
                Kirigami.FormData.label: i18n("Scheduler policy")
            }
            QQC2.Label {
                Kirigami.FormData.label: i18n("Running now:")
                text: page.schedLabel(page.store.info.current_scheduler)
            }
            QQC2.Switch {
                Kirigami.FormData.label: i18n("sched_ext:")
                text: i18n("Let scdr pick a sched_ext scheduler")
                checked: (page.store.revision, page.store.effective(["sched_ext", "enable"]) === true)
                onToggled: page.store.set(["sched_ext", "enable"], checked)
            }
            ChoiceCombo {
                Kirigami.FormData.label: i18n("Default:")
                enabled: (page.store.revision, page.store.effective(["sched_ext", "enable"]) === true)
                choices: (page.store.revision, page.withCurrent(page.schedChoices(page.store.baseValue(["sched_ext", "default"])),
                                                                page.store.layerValue(["sched_ext", "default"])))
                current: (page.store.revision, page.store.layerValue(["sched_ext", "default"]))
                onPicked: v => page.store.set(["sched_ext", "default"], v === null ? undefined : v)
            }
            ChoiceCombo {
                Kirigami.FormData.label: i18n("While a game runs:")
                enabled: (page.store.revision, page.store.effective(["sched_ext", "enable"]) === true)
                choices: (page.store.revision, page.withCurrent(page.schedChoices(page.store.baseValue(["sched_ext", "gaming"])),
                                                                page.store.layerValue(["sched_ext", "gaming"])))
                current: (page.store.revision, page.store.layerValue(["sched_ext", "gaming"]))
                onPicked: v => page.store.set(["sched_ext", "gaming"], v === null ? undefined : v)
            }
            Repeater {
                model: page.profiles
                delegate: ChoiceCombo {
                    required property string modelData
                    Kirigami.FormData.label: i18n("On %1:", modelData)
                    enabled: (page.store.revision, page.store.effective(["sched_ext", "enable"]) === true)
                    choices: (page.store.revision, page.withCurrent(page.schedChoices(page.store.baseValue(["sched_ext", "power", modelData])),
                                                                    page.store.layerValue(["sched_ext", "power", modelData])))
                    current: (page.store.revision, page.store.layerValue(["sched_ext", "power", modelData]))
                    onPicked: v => page.store.set(["sched_ext", "power", modelData], v === null ? undefined : v)
                }
            }
            QQC2.Label {
                Layout.maximumWidth: Kirigami.Units.gridUnit * 24
                wrapMode: Text.WordWrap
                font: Kirigami.Theme.smallFont
                opacity: 0.7
                text: i18n("Order: while a game runs, then the power profile, then the default. A scheduler you start by hand is left running until scdr's choice changes.")
            }

            Kirigami.Separator {
                Kirigami.FormData.isSection: true
                Kirigami.FormData.label: i18n("EEVDF (while no sched_ext scheduler runs)")
            }
            ChoiceCombo {
                Kirigami.FormData.label: i18n("Preemption:")
                choices: page.preemptChoices
                current: (page.store.revision, page.store.layerValue(["eevdf", "preempt"]))
                onPicked: v => page.store.set(["eevdf", "preempt"], v === null ? undefined : v)
            }
            OptionalSpin {
                Kirigami.FormData.label: i18n("Base slice:")
                from: 100; to: 100000; suffix: " µs"
                value: (page.store.revision, page.store.layerValue(["eevdf", "base_slice_us"]))
                fallback: (page.store.revision, page.store.baseValue(["eevdf", "base_slice_us"]))
                onEdited: v => page.store.set(["eevdf", "base_slice_us"], v)
            }
            Repeater {
                model: page.profiles
                delegate: RowLayout {
                    id: eevdfRow
                    required property string modelData
                    Kirigami.FormData.label: i18n("On %1:", modelData)
                    ChoiceCombo {
                        choices: page.preemptChoices
                        current: (page.store.revision, page.store.layerValue(["eevdf", "power", eevdfRow.modelData, "preempt"]))
                        onPicked: v => page.store.set(["eevdf", "power", eevdfRow.modelData, "preempt"], v === null ? undefined : v)
                    }
                    OptionalSpin {
                        from: 100; to: 100000; suffix: " µs"
                        value: (page.store.revision, page.store.layerValue(["eevdf", "power", eevdfRow.modelData, "base_slice_us"]))
                        fallback: (page.store.revision, page.store.baseValue(["eevdf", "power", eevdfRow.modelData, "base_slice_us"]))
                        onEdited: v => page.store.set(["eevdf", "power", eevdfRow.modelData, "base_slice_us"], v)
                    }
                }
            }

            Kirigami.Separator {
                Kirigami.FormData.isSection: true
                Kirigami.FormData.label: i18n("Get schedulers")
            }
            RowLayout {
                Kirigami.FormData.label: page.installer ? i18n("Using %1:", page.installer) : i18n("Installer:")
                QQC2.Button {
                    text: i18n("Install")
                    icon.name: "install"
                    enabled: page.installer !== ""
                    onClicked: page.runInstaller("install")
                    QQC2.ToolTip.text: i18n("Install scx-scheds and scx-tools")
                    QQC2.ToolTip.visible: hovered
                }
                QQC2.Button {
                    text: i18n("Update")
                    icon.name: "update-none"
                    enabled: page.installer !== ""
                    onClicked: page.runInstaller("update")
                    QQC2.ToolTip.text: i18n("Update the system, schedulers included (Arch doesn't support updating one package alone)")
                    QQC2.ToolTip.visible: hovered
                }
                QQC2.Button {
                    text: i18n("Latest from Git")
                    icon.name: "vcs-normal"
                    enabled: page.installer === "pamac" || page.installer === "terminal"
                    onClicked: page.runInstaller("git")
                    QQC2.ToolTip.text: i18n("Build scx-scheds-git from the AUR, for schedulers newer than the release")
                    QQC2.ToolTip.visible: hovered
                }
            }
            QQC2.Label {
                visible: page.installStatus !== "" || page.installer === ""
                Layout.maximumWidth: Kirigami.Units.gridUnit * 24
                wrapMode: Text.WordWrap
                text: page.installer === "" ? i18n("No installer found: install scx-scheds and scx-tools with your package manager.")
                                            : page.installStatus
            }
        }

        Kirigami.Heading {
            level: 4
            text: i18n("Available schedulers")
        }
        QQC2.Label {
            Layout.fillWidth: true
            visible: !!page.store.info.scx_loader_error
            wrapMode: Text.WordWrap
            text: i18n("scx_loader isn't available (%1). Install schedulers above.", page.store.info.scx_loader_error || "")
        }
        Repeater {
            model: page.schedulers
            delegate: Kirigami.AbstractCard {
                id: card
                required property var modelData
                Layout.fillWidth: true
                contentItem: RowLayout {
                    spacing: Kirigami.Units.largeSpacing
                    ColumnLayout {
                        Layout.fillWidth: true
                        RowLayout {
                            Kirigami.Heading {
                                level: 5
                                text: card.modelData.name
                            }
                            QQC2.Label {
                                text: card.modelData.installed ? "" : i18n("not installed")
                                color: Kirigami.Theme.neutralTextColor
                                font: Kirigami.Theme.smallFont
                            }
                        }
                        QQC2.Label {
                            Layout.fillWidth: true
                            wrapMode: Text.WordWrap
                            opacity: 0.8
                            text: card.modelData.description || i18n("No description. See scx_%1 --help.", card.modelData.name)
                        }
                    }
                    QQC2.ComboBox {
                        id: mode
                        model: card.modelData.modes
                        visible: card.modelData.modes.length > 1
                    }
                    QQC2.Button {
                        text: i18n("Try")
                        icon.name: "media-playback-start"
                        enabled: card.modelData.installed
                        onClicked: {
                            const m = mode.currentText || "auto";
                            page.store.call("TryScheduler", [card.modelData.name + ":" + m], () => page.store.load());
                        }
                        QQC2.ToolTip.text: i18n("Run it now without saving; stop the trial to go back")
                        QQC2.ToolTip.visible: hovered
                    }
                }
            }
        }
    }
}
