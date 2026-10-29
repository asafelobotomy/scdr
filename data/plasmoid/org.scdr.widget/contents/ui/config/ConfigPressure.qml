// Pressure & I/O: when apps that aren't focused get the background role,
// and the system-wide I/O and autogroup switches.

import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.kcmutils as KCM

KCM.SimpleKCM {
    id: page

    property SettingsStore store: SettingsStore {}
    property bool unsavedChanges: store.dirty

    function saveConfig() {
        store.save();
    }

    function spin(path) {
        return { value: store.layerValue(path), fallback: store.baseValue(path) };
    }

    ColumnLayout {
        SettingsBanner {
            store: page.store
        }

        Kirigami.FormLayout {
            Layout.fillWidth: true

            Kirigami.Separator {
                Kirigami.FormData.isSection: true
                Kirigami.FormData.label: i18n("Background demotion")
            }
            OptionalSpin {
                Kirigami.FormData.label: i18n("Turn on above:")
                from: 0; to: 100; decimals: 1; suffix: " %"
                value: (page.store.revision, page.store.layerValue(["pressure", "high"]))
                fallback: (page.store.revision, page.store.baseValue(["pressure", "high"]))
                onEdited: v => page.store.set(["pressure", "high"], v)
            }
            OptionalSpin {
                Kirigami.FormData.label: i18n("Turn off below:")
                from: 0; to: 100; decimals: 1; suffix: " %"
                value: (page.store.revision, page.store.layerValue(["pressure", "low"]))
                fallback: (page.store.revision, page.store.baseValue(["pressure", "low"]))
                onEdited: v => page.store.set(["pressure", "low"], v)
            }
            OptionalSpin {
                Kirigami.FormData.label: i18n("Hold for at least:")
                from: 0; to: 3600; suffix: " s"
                value: (page.store.revision, page.store.layerValue(["pressure", "hold_secs"]))
                fallback: (page.store.revision, page.store.baseValue(["pressure", "hold_secs"]))
                onEdited: v => page.store.set(["pressure", "hold_secs"], v)
            }
            QQC2.Label {
                Layout.maximumWidth: Kirigami.Units.gridUnit * 24
                wrapMode: Text.WordWrap
                font: Kirigami.Theme.smallFont
                opacity: 0.7
                text: i18n("CPU pressure is the share of time tasks wait for a CPU (PSI \"some\", 10 s average). Above the first threshold, apps that aren't focused get the background role until pressure drops below the second.")
            }

            Kirigami.Separator {
                Kirigami.FormData.isSection: true
                Kirigami.FormData.label: i18n("System")
            }
            ChoiceCombo {
                Kirigami.FormData.label: i18n("iocost:")
                choices: [
                    { text: i18n("Not set (%1)", page.store.baseValue(["system", "iocost"]) ? i18n("on") : i18n("off")), value: null },
                    { text: i18n("On: IOWeight works on disks without bfq"), value: true },
                    { text: i18n("Off"), value: false },
                ]
                current: (page.store.revision, page.store.layerValue(["system", "iocost"]))
                onPicked: v => page.store.set(["system", "iocost"], v === null ? undefined : v)
            }
            ChoiceCombo {
                Kirigami.FormData.label: i18n("Autogroup:")
                choices: [
                    { text: i18n("Not set"), value: null },
                    { text: i18n("Off: nice values apply system-wide"), value: false },
                    { text: i18n("On: the kernel groups tasks by session"), value: true },
                ]
                current: (page.store.revision, page.store.layerValue(["system", "autogroup"]))
                onPicked: v => page.store.set(["system", "autogroup"], v === null ? undefined : v)
            }
        }
    }
}
