// Overview: scdr's state with Pause and Resume, CPU pressure, the boosted apps and what scdr sees.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings

SettingsTab {
    id: tab

    required property var host

    title: "Overview"
    iconName: "view-list-details"

    SettingsCard {
        title: "Scheduling"
        iconName: "preferences-system-performance"
        help: "scdr gives the app you use, audio and games a bigger share of the CPU, and the rest a smaller one."
        wide: true
        Layout.fillWidth: true

        StatusSection {
            id: statusSection
            host: tab.host
        }
    }
    SettingsCard {
        title: "CPU pressure"
        iconName: "cpu"
        help: "The share of time tasks wait for a CPU, over the last 10 seconds."
        Layout.fillWidth: true

        PressureBars {
            host: tab.host
            Layout.fillWidth: true
        }
    }
    SettingsCard {
        title: "Boosted now"
        iconName: "go-up"
        help: "The apps that get more CPU time right now, and why."
        Layout.fillWidth: true

        BoostedSection {
            host: tab.host
            Layout.fillWidth: true
        }
    }
    SettingsCard {
        title: "This system"
        iconName: "computer"
        help: "What scdr sees: governor, disks, power profile and pressure."
        wide: true
        Layout.fillWidth: true

        FactsSection {
            host: tab.host
            Layout.fillWidth: true
        }
    }
}
