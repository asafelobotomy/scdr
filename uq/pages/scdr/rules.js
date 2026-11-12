.pragma library
.import "logic.js" as Logic
// Editing the person's own rules in scdr's settings layer, as the Plasma widget's Rules page:
// each rule's profile, its conditions (globs, several separated by commas; uid takes user ids),
// order, removal. Every change names the rule by its place and its exact text, so a rule changed
// elsewhere in the meantime is left alone. No Qt calls; Node tests it.

var FIELDS = Logic.CONDITIONS;
// A rule needs one of these; the not_ fields alone match nothing.
var POSITIVE = ["name", "exe", "cmdline", "cgroup", "parent", "ancestor", "uid"];
var MAX_VALUES = 32;
var MAX_VALUE = 256;
var MAX_BASE_ROWS = 300;

var MESSAGES = {
    NoValue: "Type at least one value, or remove the condition.",
    BadValue: "Values can't hold line breaks and are 256 characters at most.",
    BadUid: "Use user ids (whole numbers), separated by commas.",
    TooMany: "Use 32 values at most.",
    BadField: "Pick one of the conditions in the list.",
    Taken: "The rule already has that condition.",
    NeedsMatch: "A rule needs at least one of name, exe, cmdline, cgroup, parent, ancestor or uid.",
    NoneFree: "The rule already has every condition."
};

function fail(code) {
    return MESSAGES[code] ? { error: MESSAGES[code], code: code } : Logic.fail(code);
}

function isObject(v) {
    return v !== null && typeof v === "object" && !Array.isArray(v);
}

// `text` as a condition's value: one glob, a list of globs, or a list of user ids.
function parseValue(field, text) {
    var parts = String(text === undefined || text === null ? "" : text).split(",")
        .map(function (s) { return s.trim(); }).filter(function (s) { return s !== ""; });
    if (parts.length === 0)
        return fail("NoValue");
    if (parts.length > MAX_VALUES)
        return fail("TooMany");
    if (field === "uid") {
        var ids = [];
        for (var i = 0; i < parts.length; i++) {
            if (!/^[0-9]{1,10}$/.test(parts[i]) || Number(parts[i]) > 4294967295)
                return fail("BadUid");
            ids.push(Number(parts[i]));
        }
        return { value: ids };
    }
    for (var j = 0; j < parts.length; j++)
        if (parts[j].length > MAX_VALUE || /[\u0000-\u001f\u007f]/.test(parts[j]))
            return fail("BadValue");
    return { value: parts.length === 1 ? parts[0] : parts };
}

function show(value) {
    return Array.isArray(value) ? value.join(", ") : String(value);
}

function hasMatch(rule) {
    return POSITIVE.some(function (f) { return rule[f] !== undefined; });
}

// Runs `fn(rule, rules)` on the layer's rule at `index` if it is still the one shown (`key`).
function onRule(layer, index, key, fn) {
    if (!Array.isArray(layer.rule) || typeof index !== "number" || index < 0 || index >= layer.rule.length)
        return fail("NoSuchRule");
    if (JSON.stringify(layer.rule[index]) !== key || !isObject(layer.rule[index]))
        return fail("Changed");
    return fn(layer.rule[index], layer.rule);
}

function setProfile(layer, settings, index, key, profile) {
    var known = typeof profile === "string" && (Logic.lookup(settings.base, ["profile", profile]) !== undefined
                                                || Logic.lookup(settings.layer, ["profile", profile]) !== undefined);
    if (!known)
        return Logic.fail("BadProfile");
    return onRule(layer, index, key, function (rule) {
        rule.profile = profile;
        return { ok: true };
    });
}

// Moves a rule one place earlier (-1) or later (+1); the first match wins.
function moveRule(layer, index, key, delta) {
    return onRule(layer, index, key, function (rule, rules) {
        var to = index + delta;
        if ((delta !== -1 && delta !== 1) || to < 0 || to >= rules.length)
            return fail("NoSuchRule");
        rules.splice(index, 1);
        rules.splice(to, 0, rule);
        return { ok: true };
    });
}

function removeRule(layer, index, key) {
    return onRule(layer, index, key, function (rule, rules) {
        rules.splice(index, 1);
        if (rules.length === 0)
            delete layer.rule;
        return { ok: true };
    });
}

function setCondition(layer, index, key, field, text) {
    if (FIELDS.indexOf(field) < 0)
        return fail("BadField");
    var parsed = parseValue(field, text);
    if (parsed.error)
        return parsed;
    return onRule(layer, index, key, function (rule) {
        rule[field] = parsed.value;
        return { ok: true };
    });
}

// Turns one condition into another (name into exe, say), keeping its values.
function renameCondition(layer, index, key, from, to) {
    if (FIELDS.indexOf(from) < 0 || FIELDS.indexOf(to) < 0)
        return fail("BadField");
    return onRule(layer, index, key, function (rule) {
        if (rule[from] === undefined)
            return fail("Changed");
        if (rule[to] !== undefined)
            return fail("Taken");
        var parsed = parseValue(to, show(rule[from]));
        if (parsed.error)
            return parsed;
        delete rule[from];
        rule[to] = parsed.value;
        return hasMatch(rule) ? { ok: true } : fail("NeedsMatch");
    });
}

function removeCondition(layer, index, key, field) {
    return onRule(layer, index, key, function (rule) {
        if (rule[field] === undefined)
            return fail("Changed");
        delete rule[field];
        return hasMatch(rule) ? { ok: true } : fail("NeedsMatch");
    });
}

// Adds the first condition the rule doesn't have yet, matching anything ("*", or uid 1000)
// until the person types its value.
function addCondition(layer, index, key) {
    return onRule(layer, index, key, function (rule) {
        var free = FIELDS.filter(function (f) { return rule[f] === undefined; });
        if (free.length === 0)
            return fail("NoneFree");
        rule[free[0]] = free[0] === "uid" ? [1000] : "*";
        return { ok: true, field: free[0] };
    });
}

// The fields a condition can be turned into: its own and the ones the rule doesn't have.
function fieldChoices(conditions, field) {
    var used = {};
    conditions.forEach(function (c) { used[c.field] = true; });
    return FIELDS.filter(function (f) { return f === field || !used[f]; }).map(function (f) {
        return { text: f, value: f };
    });
}

// The rules from scdr's files, for reading: {profile, summary, source}, and how many in all.
function baseRules(base) {
    var raw = isObject(base) && Array.isArray(base.rule) ? base.rule : [];
    var rows = [];
    for (var i = 0; i < raw.length && rows.length < MAX_BASE_ROWS; i++) {
        if (!isObject(raw[i]))
            continue;
        rows.push({
            profile: Logic.clip(raw[i].profile, 40),
            summary: Logic.describeRule(raw[i]),
            source: typeof raw[i]._source === "string" ? Logic.clip(raw[i]._source, 160) : ""
        });
    }
    return { rows: rows, total: raw.length };
}
