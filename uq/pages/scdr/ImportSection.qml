// Rules from ananicy-cpp or system76-scheduler files, previewed with `scdrctl import --dry-run
// --json`: what would be added and what would be left out. Writing them needs an administrator
// (the rules go to /etc/scdr/rules.d), so the page shows the exact command for a terminal.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "plans.js" as Plans

ColumnLayout {
    id: section

    required property var host
    property string kind: "ananicy"
    property var plan: null
    property string problem: ""
    property bool loading: false
    property string checked: ""

    function preview() {
        const path = source.text.trim();
        section.problem = Plans.importProblem(path);
        section.plan = null;
        if (section.problem !== "" || !section.host.ctx || section.loading)
            return;
        section.loading = true;
        section.host.ctx.command.run("preview-import-" + section.kind, [path], section.host.live(result => {
            section.loading = false;
            section.plan = Plans.planOf(result);
            section.checked = path;
        }));
    }

    Layout.fillWidth: true
    spacing: 6

    SettingRow {
        settingId: "import"
        label: "Bring rules from another tool"
        highlight: section.host.highlight
        Layout.fillWidth: true
        ChoiceBox {
            objectName: "import-kind"
            choices: [{ text: "ananicy-cpp rules", value: "ananicy" }, { text: "system76-scheduler file", value: "system76" }]
            current: section.kind
            onPicked: v => {
                section.kind = v;
                section.plan = null;
            }
        }
    }
    RowLayout {
        Layout.fillWidth: true
        QQC2.TextField {
            id: source
            objectName: "import-path"
            placeholderText: section.kind === "ananicy" ? "/etc/ananicy.d" : "/etc/system76-scheduler/config.kdl"
            maximumLength: 400
            Layout.fillWidth: true
            onAccepted: section.preview()
            onTextChanged: section.problem = ""
        }
        QQC2.Button {
            objectName: "import-preview"
            text: "Preview"
            enabled: source.text.trim() !== "" && !section.loading
            onClicked: section.preview()
        }
    }
    QQC2.ProgressBar {
        visible: section.loading
        indeterminate: true
        Layout.fillWidth: true
    }
    UqLabel {
        visible: section.problem !== ""
        text: section.problem
        color: section.host.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    PlanView {
        plan: section.plan
        mutedColor: section.host.mutedColor
        errorColor: section.host.errorColor
        Layout.fillWidth: true
    }
    UqLabel {
        visible: section.plan !== null && section.plan.problem === "" && !section.plan.refuses
        text: "To write these rules, run this in a terminal. It asks for your password, and `scdrctl reload` makes scdr read them."
        color: section.host.mutedColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    UqLabel {
        objectName: "import-command"
        visible: section.plan !== null && section.plan.problem === "" && !section.plan.refuses
        text: Plans.importCommand(section.kind, section.checked, "")
        font.family: "monospace"
        wrapMode: Text.WrapAnywhere
        Layout.fillWidth: true
    }
}
