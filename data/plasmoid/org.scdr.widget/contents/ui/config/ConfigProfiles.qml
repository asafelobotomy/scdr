// Profiles: the settings a rule or role applies. Editing a profile from the
// shipped or /etc files makes a copy in the widget's settings, which
// replaces it whole; "Revert" drops the copy.

import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.kcmutils as KCM

KCM.SimpleKCM {
    id: page

    property SettingsStore store: SettingsStore {}
    property bool unsavedChanges: store.dirty
    property string selected: ""

    function saveConfig() {
        store.save();
    }

    readonly property var names: (store.revision, store.profileNames())
    readonly property var edited: (store.revision, store.layerValue(["profile", selected]))
    readonly property var shipped: (store.revision, store.baseValue(["profile", selected]))
    readonly property var current: edited !== undefined ? edited : (shipped || {})

    function origin(name) {
        const inLayer = store.layerValue(["profile", name]) !== undefined;
        const b = store.baseValue(["profile", name]);
        if (inLayer) {
            return b ? i18n("changed here") : i18n("added here");
        }
        return b && b._source ? b._source : "";
    }

    function setField(field, value) {
        const copy = {};
        for (const [k, v] of Object.entries(current)) {
            if (!k.startsWith("_")) {
                copy[k] = v;
            }
        }
        if (value === undefined || value === "") {
            delete copy[field];
        } else {
            copy[field] = value;
        }
        store.set(["profile", selected], copy);
    }

    readonly property var numberFields: [
        { key: "nice", label: i18n("Nice:"), from: -20, to: 19, suffix: "" },
        { key: "slice_us", label: i18n("EEVDF slice:"), from: 100, to: 100000, suffix: " µs" },
        { key: "oom_score_adj", label: i18n("OOM score adjust:"), from: -1000, to: 1000, suffix: "" },
        { key: "uclamp_min", label: i18n("uclamp min:"), from: 0, to: 1024, suffix: "" },
        { key: "uclamp_max", label: i18n("uclamp max:"), from: 0, to: 1024, suffix: "" },
        { key: "cpu_weight", label: i18n("CPUWeight (unit):"), from: 1, to: 10000, suffix: "" },
        { key: "io_weight", label: i18n("IOWeight (unit):"), from: 1, to: 10000, suffix: "" },
    ]

    Connections {
        target: page.store
        function onRevisionChanged() {
            if (page.selected === "" && page.names.length) {
                page.selected = page.names[0];
            }
        }
    }

    ColumnLayout {
        SettingsBanner {
            store: page.store
        }

        RowLayout {
            spacing: Kirigami.Units.largeSpacing

            ColumnLayout {
                Layout.alignment: Qt.AlignTop
                Layout.preferredWidth: Kirigami.Units.gridUnit * 12
                Repeater {
                    model: page.names
                    delegate: QQC2.ItemDelegate {
                        required property string modelData
                        Layout.fillWidth: true
                        highlighted: modelData === page.selected
                        text: modelData
                        onClicked: page.selected = modelData
                        QQC2.ToolTip.text: page.origin(modelData)
                        QQC2.ToolTip.visible: hovered && QQC2.ToolTip.text !== ""
                    }
                }
                RowLayout {
                    QQC2.TextField {
                        id: newName
                        Layout.fillWidth: true
                        placeholderText: i18n("New profile")
                        validator: RegularExpressionValidator { regularExpression: /[A-Za-z0-9_-]+/ }
                    }
                    QQC2.Button {
                        icon.name: "list-add"
                        enabled: newName.text !== "" && !page.names.includes(newName.text)
                        onClicked: {
                            page.store.set(["profile", newName.text], {});
                            page.selected = newName.text;
                            newName.text = "";
                        }
                        QQC2.ToolTip.text: i18n("Add a profile")
                        QQC2.ToolTip.visible: hovered
                    }
                }
            }

            Kirigami.FormLayout {
                Layout.alignment: Qt.AlignTop
                Layout.fillWidth: true
                visible: page.selected !== ""

                Kirigami.Heading {
                    Kirigami.FormData.isSection: true
                    level: 4
                    text: page.selected
                }
                QQC2.Label {
                    Kirigami.FormData.label: i18n("From:")
                    text: page.origin(page.selected) || i18n("–")
                    elide: Text.ElideMiddle
                    Layout.maximumWidth: Kirigami.Units.gridUnit * 18
                }
                ChoiceCombo {
                    Kirigami.FormData.label: i18n("Inherits:")
                    choices: [{ text: i18n("Nothing"), value: null }].concat(
                        page.names.filter(n => n !== page.selected).map(n => ({ text: n, value: n })))
                    current: page.current.inherit === undefined ? null : page.current.inherit
                    onPicked: v => page.setField("inherit", v === null ? undefined : v)
                }
                ChoiceCombo {
                    Kirigami.FormData.label: i18n("Policy:")
                    choices: {
                        const c = [{ text: i18n("Not set"), value: null }, { text: "other", value: "other" },
                                   { text: "batch", value: "batch" }, { text: "idle", value: "idle" }];
                        for (const rt of ["fifo", "rr"]) {
                            for (const prio of [1, 10, 50, 99]) {
                                c.push({ text: rt + ":" + prio, value: rt + ":" + prio });
                            }
                        }
                        const cur = page.current.sched;
                        if (cur !== undefined && !c.some(x => x.value === cur)) {
                            c.push({ text: cur, value: cur });
                        }
                        return c;
                    }
                    current: page.current.sched === undefined ? null : page.current.sched
                    onPicked: v => page.setField("sched", v === null ? undefined : v)
                }
                ChoiceCombo {
                    Kirigami.FormData.label: i18n("I/O priority:")
                    choices: {
                        const c = [{ text: i18n("Not set"), value: null }, { text: "idle", value: "idle" }];
                        for (let i = 0; i < 8; i++) {
                            c.push({ text: "be:" + i, value: "be:" + i });
                        }
                        for (let i = 0; i < 8; i++) {
                            c.push({ text: "rt:" + i, value: "rt:" + i });
                        }
                        return c;
                    }
                    current: page.current.io === undefined ? null : page.current.io
                    onPicked: v => page.setField("io", v === null ? undefined : v)
                }
                Repeater {
                    model: page.numberFields
                    delegate: OptionalSpin {
                        required property var modelData
                        Kirigami.FormData.label: modelData.label
                        from: modelData.from
                        to: modelData.to
                        suffix: modelData.suffix
                        value: page.current[modelData.key]
                        fallback: undefined
                        onEdited: v => page.setField(modelData.key, v)
                    }
                }
                RowLayout {
                    QQC2.Button {
                        text: page.shipped ? i18n("Revert to %1", page.shipped._source ? page.shipped._source.split("/").slice(-2).join("/") : i18n("file")) : i18n("Delete profile")
                        icon.name: page.shipped ? "edit-undo" : "edit-delete"
                        enabled: page.edited !== undefined
                        onClicked: {
                            page.store.set(["profile", page.selected], undefined);
                            if (!page.shipped) {
                                page.selected = "";
                            }
                        }
                    }
                }
                QQC2.Label {
                    Layout.maximumWidth: Kirigami.Units.gridUnit * 20
                    wrapMode: Text.WordWrap
                    font: Kirigami.Theme.smallFont
                    opacity: 0.7
                    text: i18n("Unset fields are left as they are, or come from the inherited profile. Unit weights apply to a process's whole systemd unit, so give them only to profiles for whole apps or services. Realtime policies can starve the rest of the system.")
                }
            }
        }
    }
}
