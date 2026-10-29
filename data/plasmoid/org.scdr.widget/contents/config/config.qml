// Settings pages in the widget's Configure dialog. Each page edits the
// settings layer scdrd keeps in /var/lib/scdr; Apply asks polkit.

import QtQuick
import org.kde.plasma.configuration

ConfigModel {
    ConfigCategory {
        name: i18n("Schedulers")
        icon: "preferences-system-performance"
        source: "config/ConfigSchedulers.qml"
    }
    ConfigCategory {
        name: i18n("Pressure & I/O")
        icon: "speedometer"
        source: "config/ConfigPressure.qml"
    }
    ConfigCategory {
        name: i18n("Roles")
        icon: "window"
        source: "config/ConfigRoles.qml"
    }
    ConfigCategory {
        name: i18n("Profiles")
        icon: "view-list-details"
        source: "config/ConfigProfiles.qml"
    }
    ConfigCategory {
        name: i18n("Rules")
        icon: "view-filter"
        source: "config/ConfigRules.qml"
    }
}
