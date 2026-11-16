// Tools: check and reload, the benchmark and the importers.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings

SettingsTab {
    id: tab

    required property var host

    title: "Tools"
    iconName: "configure"

    SettingsCard {
        title: "Check and reload"
        iconName: "dialog-ok-apply"
        help: "Checking reads scdr's files without changing anything."
        Layout.fillWidth: true

        ToolsSection {
            host: tab.host
        }
    }
    SettingsCard {
        title: "Benchmark"
        iconName: "speedometer"
        help: "Measures how late an interactive thread wakes while every CPU is busy."
        Layout.fillWidth: true

        BenchSection {
            host: tab.host
        }
    }
    SettingsCard {
        title: "Import rules"
        iconName: "document-import"
        help: "Reads ananicy-cpp or system76-scheduler rules and says what would be written."
        wide: true
        Layout.fillWidth: true

        ImportSection {
            host: tab.host
        }
    }
}
