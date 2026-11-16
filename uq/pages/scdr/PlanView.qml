// A preview (Plans.planOf, or a plan the page builds itself): the summary, then the changes
// grouped as uQ's own confirmation sheet shows them. Text is plain.
import QtQuick
import QtQuick.Layouts
import org.uq.settings
import "plans.js" as Plans

ColumnLayout {
    id: view

    property var plan: null
    property color mutedColor: "grey"
    property color errorColor: "red"

    visible: view.plan !== null
    spacing: 4

    UqLabel {
        text: view.plan !== null ? (view.plan.problem !== "" ? view.plan.problem : view.plan.summary) : ""
        color: view.plan !== null && (view.plan.problem !== "" || view.plan.refuses) ? view.errorColor : palette.windowText
        font.bold: true
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    UqLabel {
        visible: view.plan !== null && view.plan.refuses
        text: view.plan !== null ? view.plan.reason : ""
        color: view.errorColor
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    Repeater {
        model: view.plan !== null && view.plan.problem === "" ? Plans.planGroups(view.plan) : []

        ColumnLayout {
            id: group

            required property var modelData

            spacing: 2
            Layout.fillWidth: true

            UqLabel {
                text: group.modelData.title
                color: view.mutedColor
                font.bold: true
                Layout.topMargin: 4
            }
            Repeater {
                model: group.modelData.items

                UqLabel {
                    required property var modelData

                    text: Plans.changeText(modelData)
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                    Layout.leftMargin: 8
                }
            }
        }
    }
    UqLabel {
        visible: view.plan !== null && view.plan.more > 0
        text: view.plan !== null ? "And " + view.plan.more + " more." : ""
        color: view.mutedColor
    }
}
