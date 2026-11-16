// Roles: the boosts, the profile each one gives, the focus agent and the CPU pressure thresholds.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings

SettingsTab {
    id: tab

    required property var host

    title: "Roles"
    iconName: "games-config-board"

    SettingsCard {
        title: "Boosts"
        iconName: "go-up"
        help: "Each switch turns one boost on or off. Apps no rule matches get the profile of their role."
        visible: tab.host.ready
        Layout.fillWidth: true

        RolesSection {
            id: rolesSection
            host: tab.host
        }
    }
    SettingsCard {
        title: "Profile each role gives"
        iconName: "view-list-details"
        help: "Which profile each boost applies, for every power profile."
        visible: tab.host.ready && tab.host.settings !== null
        Layout.fillWidth: true

        RoleProfilesSection {
            id: roleProfilesSection
            host: tab.host
        }
    }
    SettingsCard {
        title: "Focused window"
        iconName: "window"
        help: "The session agent tells scdr which window you are using."
        Layout.fillWidth: true

        FollowSection {
            id: followSection
            host: tab.host
        }
    }
    SettingsCard {
        title: "When the CPU is busy"
        iconName: "cpu"
        help: "Apps that aren't focused get the background role above the first value, until pressure drops below the second."
        visible: tab.host.ready && tab.host.settings !== null
        Layout.fillWidth: true

        PressureSection {
            id: pressureSection
            host: tab.host
        }
    }
}
