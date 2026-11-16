// Rules: the person's own rules, profiles, the rules from scdr's files, Explain and the running apps.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings

SettingsTab {
    id: tab

    required property var host

    title: "Rules"
    iconName: "view-sort-ascending"

    SettingsCard {
        title: "Your app rules"
        iconName: "list-add"
        help: "Send an app to a profile. Your rules are tried before the ones in scdr's files."
        visible: tab.host.ready && tab.host.settings !== null
        wide: true
        Layout.fillWidth: true

        RulesSection {
            id: rulesSection
            host: tab.host
        }
    }
    SettingsCard {
        title: "Profiles"
        iconName: "document-properties"
        help: "The settings a rule or role applies. Undo drops your copy of a profile from scdr's files."
        visible: tab.host.ready && tab.host.settings !== null
        wide: true
        Layout.fillWidth: true

        ProfilesSection {
            id: profilesSection
            host: tab.host
        }
    }
    SettingsCard {
        title: "Rules from scdr's files"
        iconName: "folder"
        help: "Shipped and admin rules. They can't be changed here; your rules go first."
        visible: tab.host.ready && tab.host.settings !== null
        wide: true
        Layout.fillWidth: true

        FileRulesSection {
            id: fileRulesSection
            host: tab.host
        }
    }
    SettingsCard {
        title: "Explain a process"
        iconName: "help-about"
        help: "Which rule and profile apply to a running process, and what scdr did."
        wide: true
        Layout.fillWidth: true

        ExplainSection {
            id: explainSection
            host: tab.host
        }
    }
    SettingsCard {
        title: "Running apps"
        iconName: "system-run"
        help: "Read from the files, so it works while scdr is off."
        wide: true
        Layout.fillWidth: true

        RunningSection {
            host: tab.host
            explainer: explainSection
        }
    }
}
