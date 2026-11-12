.pragma library
.import "logic.js" as Logic
// The scheduler parts of scdr's settings layer: which scheduler runs by default, while a game
// runs and per power profile ([sched_ext]), the kernel scheduler's own knobs ([eevdf]), and a
// trial run. Same values as the Plasma widget's Schedulers page. No Qt calls; Node tests it.

var PREEMPT = ["none", "voluntary", "full", "lazy"];
var MODES = ["auto", "gaming", "powersave", "lowlatency", "server"];
var SLICE_MIN = 100;
var SLICE_MAX = 100000;

var MESSAGES = {
    BadSlot: "That isn't a setting this page knows.",
    BadPreempt: "Pick one of the choices in the list.",
    BadSlice: "Use a whole number from 100 to 100000."
};

function fail(code) {
    return MESSAGES[code] ? { error: MESSAGES[code], code: code } : Logic.fail(code);
}

function goodKey(k) {
    return typeof k === "string" && new RegExp(Logic.KEY).test(k);
}

// "default", "gaming" or a power profile's name, as a path under [sched_ext]; null if bad.
function slotPath(slot) {
    if (slot === "default" || slot === "gaming")
        return ["sched_ext", slot];
    return goodKey(slot) && slot !== "enable" ? ["sched_ext", "power", slot] : null;
}

// The choice a slot has in the layer ("lavd:gaming", "eevdf"), or null when it isn't set.
function slotCurrent(settings, slot) {
    var path = slotPath(slot);
    var v = path ? Logic.normChoice(Logic.lookup(settings.layer, path)) : "";
    return v === "" ? null : v;
}

// The choices for a slot: not set (with what applies then), the kernel's own, every installed
// scheduler in each mode, and the layer's choice if it isn't installed.
function slotChoices(settings, slot) {
    var path = slotPath(slot);
    if (!path)
        return [];
    var fallback = Logic.normChoice(Logic.lookup(settings.base, path));
    var unset = fallback !== "" ? "Not set (" + Logic.choiceLabel(fallback) + ")"
        : (slot === "default" ? "Not set (Kernel default)" : "Same as default");
    var out = [{ text: unset, value: null }, { text: "Kernel default", value: "eevdf" }];
    settings.installed.forEach(function (c) {
        out.push({ text: c.label, value: Logic.normChoice(c.value) });
    });
    var cur = slotCurrent(settings, slot);
    if (cur !== null && !out.some(function (c) { return c.value === cur; }))
        out.push({ text: Logic.choiceLabel(cur) + " (not installed)", value: cur });
    return out;
}

// Sets a slot; a scheduler other than the kernel's also turns scdr's choosing on, so picking
// one makes it run.
function setSlot(layer, slot, choice) {
    var path = slotPath(slot);
    if (!path)
        return fail("BadSlot");
    if (choice !== null && (typeof choice !== "string" || !new RegExp(Logic.CHOICE).test(choice)))
        return fail("BadScheduler");
    var r = Logic.setPath(layer, path, choice);
    if (r.error || choice === null || choice === "eevdf")
        return r;
    return Logic.setPath(layer, ["sched_ext", "enable"], true);
}

function setEnabled(layer, on) {
    return Logic.setPath(layer, ["sched_ext", "enable"], on === true);
}

// ---- The kernel scheduler's knobs ([eevdf], [eevdf.power.<profile>]) ------------------

function knobPath(power, knob) {
    if (power !== null && !goodKey(power))
        return null;
    return power === null ? ["eevdf", knob] : ["eevdf", "power", power, knob];
}

function preemptChoices(settings, power) {
    var base = Logic.lookup(settings.base, knobPath(power, "preempt") || []);
    var unset = typeof base === "string" ? "Not set (" + Logic.clip(base, 20) + ")"
        : (power === null ? "Not set" : "Same as default");
    var out = [{ text: unset, value: null }];
    PREEMPT.forEach(function (p) { out.push({ text: p, value: p }); });
    var cur = Logic.lookup(settings.layer, knobPath(power, "preempt") || []);
    if (typeof cur === "string" && PREEMPT.indexOf(cur) < 0)
        out.push({ text: Logic.clip(cur, 20), value: cur });
    return out;
}

function setPreempt(layer, power, value) {
    var path = knobPath(power, "preempt");
    if (!path)
        return fail("BadSlot");
    if (value !== null && PREEMPT.indexOf(value) < 0)
        return fail("BadPreempt");
    return Logic.setPath(layer, path, value);
}

function setSlice(layer, power, value) {
    var path = knobPath(power, "base_slice_us");
    if (!path)
        return fail("BadSlot");
    if (value !== undefined && value !== null
        && (typeof value !== "number" || Math.floor(value) !== value || value < SLICE_MIN || value > SLICE_MAX))
        return fail("BadSlice");
    return Logic.setPath(layer, path, value);
}

// ---- Trying a scheduler ---------------------------------------------------------------

// The argument for TryScheduler ("bpfland:gaming"), or "" if the name or mode is odd.
function tryArg(name, mode) {
    var m = typeof mode === "string" && mode !== "" ? mode : "auto";
    if (!goodKey(name) || MODES.indexOf(m) < 0)
        return "";
    var arg = name + ":" + m;
    return new RegExp(Logic.CHOICE).test(arg) ? arg : "";
}
