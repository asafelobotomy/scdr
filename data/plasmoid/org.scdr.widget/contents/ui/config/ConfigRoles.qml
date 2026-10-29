// Roles: the profile each role gives, by default and per power profile.

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

    readonly property var roles: [
        { key: "foreground", label: i18n("Focused app") },
        { key: "audio", label: i18n("Playing audio") },
        { key: "game", label: i18n("Games") },
        { key: "background", label: i18n("Background (under pressure)") },
    ]
    readonly property var powerProfiles: store.info.power_profiles || []

    function describe(v) {
        return v === undefined || v === null ? i18n("off") : v === "" ? i18n("off") : v;
    }

    function choices(role, power) {
        const names = (store.revision, store.profileNames());
        const out = [];
        const fallback = store.baseValue(path(role, power));
        out.push({ text: power && fallback === undefined ? i18n("Same as default") : i18n("Not set (%1)", describe(fallback)),
                   value: null });
        out.push({ text: i18n("Off"), value: "" });
        for (const n of names) {
            out.push({ text: n, value: n });
        }
        return out;
    }

    function path(role, power) {
        return power ? ["roles", "power", power, role] : ["roles", role];
    }

    ColumnLayout {
        SettingsBanner {
            store: page.store
        }

        QQC2.Label {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            text: i18n("Roles give a profile to processes no rule matches, for what they're doing right now. A power profile column overrides the default while that profile is active.")
        }

        GridLayout {
            columns: 2 + page.powerProfiles.length
            columnSpacing: Kirigami.Units.largeSpacing
            rowSpacing: Kirigami.Units.smallSpacing

            QQC2.Label { text: "" }
            QQC2.Label { text: i18n("Default"); font.bold: true }
            Repeater {
                model: page.powerProfiles
                delegate: QQC2.Label {
                    required property string modelData
                    text: modelData
                    font.bold: true
                }
            }

            Repeater {
                model: page.roles
                delegate: QQC2.Label {
                    required property var modelData
                    required property int index
                    Layout.row: index + 1
                    Layout.column: 0
                    text: modelData.label
                }
            }
            Repeater {
                model: page.roles.length * (1 + page.powerProfiles.length)
                delegate: ChoiceCombo {
                    required property int index
                    readonly property var role: page.roles[Math.floor(index / (1 + page.powerProfiles.length))].key
                    readonly property int col: index % (1 + page.powerProfiles.length)
                    readonly property var power: col === 0 ? null : page.powerProfiles[col - 1]
                    Layout.row: Math.floor(index / (1 + page.powerProfiles.length)) + 1
                    Layout.column: col + 1
                    Layout.fillWidth: true
                    choices: page.choices(role, power)
                    current: (page.store.revision, page.store.layerValue(page.path(role, power)))
                    onPicked: v => page.store.set(page.path(role, power), v === null ? undefined : v)
                }
            }
        }
    }
}
