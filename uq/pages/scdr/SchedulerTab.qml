// Scheduler: the sched_ext choice with trials, the installer for more schedulers, and the kernel's own knobs.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings

SettingsTab {
    id: tab

    required property var host

    title: "Scheduler"
    available: tab.host.ready && tab.host.settings !== null
    iconName: "preferences-system-performance"

    SettingsCard {
        title: "CPU scheduler"
        iconName: "cpu"
        help: "Which scheduler runs: by default, while a game runs and per power profile."
        visible: tab.host.ready && tab.host.settings !== null
        Layout.fillWidth: true

        SchedulerSection {
            id: schedulerSection
            host: tab.host
        }
    }
    SettingsCard {
        title: "Available schedulers"
        iconName: "system-software-install"
        help: "Install or update the extra schedulers. Your package manager asks for your password."
        visible: tab.host.ready && tab.host.settings !== null
        Layout.fillWidth: true

        AvailableSection {
            id: availableSection
            host: tab.host
        }
    }
    SettingsCard {
        title: "Kernel scheduler"
        iconName: "applications-system"
        help: "Used while no extra scheduler runs."
        visible: tab.host.ready && tab.host.settings !== null
        Layout.fillWidth: true

        KernelSection {
            id: kernelSection
            host: tab.host
        }
    }
}
