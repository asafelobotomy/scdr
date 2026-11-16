// The running apps and which rule gives each its profile: `scdrctl list`, read offline from the
// files, so it works while scdr is off. Filtered by name or profile; Explain asks the daemon why.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "plans.js" as Plans

ColumnLayout {
    id: section

    required property var host
    property var explainer: null
    property bool loading: false
    property bool everything: false
    property var last: null
    readonly property var view: section.last !== null ? Plans.processesView(section.last, filter.text, 60) : null

    function refresh() {
        if (!section.host.ctx || section.loading)
            return;
        section.loading = true;
        section.host.ctx.command.run(section.everything ? "list-all" : "list", [], section.host.live(result => {
            section.loading = false;
            section.last = result;
        }));
    }

    Layout.fillWidth: true
    spacing: 6

    SettingRow {
        settingId: "running"
        label: section.view === null ? "Running apps and their rules" : (section.view.ok ? section.view.total + " running" : "Running apps")
        highlight: section.host.highlight
        Layout.fillWidth: true
        QQC2.Button {
            objectName: "running-refresh"
            text: section.last === null ? "Show" : "Refresh"
            enabled: !section.loading
            onClicked: section.refresh()
        }
    }
    RowLayout {
        visible: section.last !== null
        Layout.fillWidth: true
        QQC2.TextField {
            id: filter
            placeholderText: "Filter by name or profile"
            maximumLength: 40
            Layout.fillWidth: true
        }
        QQC2.CheckBox {
            text: "Include apps no rule matches"
            checked: section.everything
            enabled: !section.loading
            onToggled: {
                section.everything = checked;
                section.refresh();
            }
        }
    }
    QQC2.ProgressBar {
        visible: section.loading
        indeterminate: true
        Layout.fillWidth: true
    }
    UqLabel {
        visible: section.view !== null && !section.view.ok
        text: section.view !== null ? section.view.problem : ""
        color: section.host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    Repeater {
        model: section.view !== null && section.view.ok ? section.view.rows : []

        RowLayout {
            id: line

            required property var modelData

            spacing: 8
            Layout.fillWidth: true
            UqAppIcon {
                name: line.modelData.name
                label: line.modelData.name
                size: 24
            }
            UqLabel {
                text: line.modelData.name
                elide: Text.ElideRight
                Layout.preferredWidth: 130
            }
            UqLabel {
                text: line.modelData.profile + (line.modelData.rule !== "" ? " · " + line.modelData.rule : "")
                color: section.host.mutedColor
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            QQC2.Button {
                flat: true
                text: "Explain"
                visible: section.explainer !== null
                enabled: section.host.ready
                onClicked: section.explainer.explain(line.modelData.pid)
            }
        }
    }
    UqLabel {
        visible: section.view !== null && section.view.ok && section.view.total > section.view.rows.length
        text: section.view !== null ? "Showing " + section.view.rows.length + " of " + section.view.total + ". Filter to find the rest." : ""
        color: section.host.mutedColor
    }
}
