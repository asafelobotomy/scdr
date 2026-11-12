.pragma library
.import "logic.js" as Logic
// Profiles and what gives them out: the role → profile table ([roles], per power profile),
// the CPU-pressure thresholds and system switches, and the profiles themselves ([profile.*]).
// Same values and rules as the Plasma widget's Roles, Pressure and Profiles pages.

var ROLES = ["foreground", "audio", "game", "background"];
var POLICIES = ["other", "batch", "idle", "fifo:1", "fifo:10", "fifo:50", "fifo:99", "rr:1", "rr:10", "rr:50", "rr:99"];
var POLICY = "^(other|normal|batch|idle|(fifo|rr):([1-9]|[1-9][0-9]))$";
var IO = "^(idle|(be|rt):[0-7])$";
// The profile fields that take a whole number, with their range.
var NUMBERS = [
    { key: "nice", label: "Nice", from: -20, to: 19, suffix: "" },
    { key: "slice_us", label: "Time slice", from: 100, to: 100000, suffix: " µs" },
    { key: "oom_score_adj", label: "Out-of-memory adjustment", from: -1000, to: 1000, suffix: "" },
    { key: "uclamp_min", label: "Lowest CPU speed hint", from: 0, to: 1024, suffix: "" },
    { key: "uclamp_max", label: "Highest CPU speed hint", from: 0, to: 1024, suffix: "" },
    { key: "cpu_weight", label: "Unit CPU weight", from: 1, to: 10000, suffix: "" },
    { key: "io_weight", label: "Unit disk weight", from: 1, to: 10000, suffix: "" }
];
// [pressure]: CPU PSI "some" avg10 in percent, and the shortest time between switches.
var PRESSURE = {
    high: { from: 0, to: 100, decimals: 1, fallback: 20 },
    low: { from: 0, to: 100, decimals: 1, fallback: 5 },
    hold_secs: { from: 0, to: 3600, decimals: 0, fallback: 30 }
};

var MESSAGES = {
    BadRole: "That isn't a role this page knows.",
    BadNumber: "That number is out of range.",
    PressureOrder: "“Turn off below” has to be lower than “Turn on above”.",
    BadChoice: "Pick one of the choices in the list.",
    NoSuchProfile: "That profile is gone; refresh and try again.",
    ProfileExists: "There already is a profile with that name.",
    ProfileName: "Use lower-case letters, numbers, - and _ only, starting with a letter or number. 40 characters at most.",
    ProfileInUse: "Your rules, roles or profiles still use this profile.",
    EmptyProfile: "Keep at least one setting in a profile."
};

function fail(code) {
    return MESSAGES[code] ? { error: MESSAGES[code], code: code } : Logic.fail(code);
}

function isObject(v) {
    return v !== null && typeof v === "object" && !Array.isArray(v);
}

function goodKey(k) {
    return typeof k === "string" && new RegExp(Logic.KEY).test(k);
}

function wholeIn(v, from, to) {
    return typeof v === "number" && Math.floor(v) === v && v >= from && v <= to;
}

function profileExists(settings, name) {
    return goodKey(name) && (Logic.lookup(settings.base, ["profile", name]) !== undefined
                             || Logic.lookup(settings.layer, ["profile", name]) !== undefined);
}

// ---- Roles ----------------------------------------------------------------------------

function rolePath(role, power) {
    if (ROLES.indexOf(role) < 0 || (power !== null && !goodKey(power)))
        return null;
    return power === null ? ["roles", role] : ["roles", "power", power, role];
}

// Not set (with what applies then), off, and every profile.
function roleChoices(settings, role, power) {
    var path = rolePath(role, power);
    if (!path)
        return [];
    var base = Logic.lookup(settings.base, path);
    var shown = typeof base === "string" && base !== "" ? Logic.clip(base, 40) : "off";
    var out = [{ text: power !== null && base === undefined ? "Same as default" : "Not set (" + shown + ")", value: null },
               { text: "Off", value: "" }];
    settings.profiles.forEach(function (n) { out.push({ text: n, value: n }); });
    var cur = Logic.lookup(settings.layer, path);
    if (typeof cur === "string" && !out.some(function (c) { return c.value === cur; }))
        out.push({ text: Logic.clip(cur, 40), value: cur });
    return out;
}

function roleCurrent(settings, role, power) {
    var v = Logic.lookup(settings.layer, rolePath(role, power) || []);
    return typeof v === "string" ? v : null;
}

function setRole(layer, settings, role, power, value) {
    var path = rolePath(role, power);
    if (!path)
        return fail("BadRole");
    if (value !== null && value !== "" && !profileExists(settings, value))
        return Logic.fail("BadProfile");
    return Logic.setPath(layer, path, value);
}

// ---- Pressure and the system switches -------------------------------------------------

function pressureValue(settings, key) {
    var v = Logic.lookup(settings.layer, ["pressure", key]);
    return v === undefined ? Logic.lookup(settings.base, ["pressure", key]) : v;
}

function setPressure(layer, settings, key, value) {
    var spec = PRESSURE[key];
    if (!spec)
        return fail("BadChoice");
    if (value !== undefined && value !== null) {
        var whole = spec.decimals === 0;
        if (typeof value !== "number" || !isFinite(value) || value < spec.from || value > spec.to || (whole && Math.floor(value) !== value))
            return fail("BadNumber");
    }
    var r = Logic.setPath(layer, ["pressure", key], value);
    if (r.error)
        return r;
    var now = { layer: layer, base: settings.base };
    var high = pressureValue(now, "high"), low = pressureValue(now, "low");
    if (!((typeof low === "number" ? low : PRESSURE.low.fallback) < (typeof high === "number" ? high : PRESSURE.high.fallback)))
        return fail("PressureOrder");
    return r;
}

// iocost and autogroup: not set, on, off.
function switchChoices(settings, key) {
    var base = Logic.lookup(settings.base, ["system", key]);
    var unset = base === true ? "Not set (on)" : base === false ? "Not set (off)" : "Not set";
    if (key === "iocost")
        return [{ text: unset, value: null }, { text: "On: disk weights work without the bfq disk scheduler", value: true },
                { text: "Off", value: false }];
    return [{ text: unset, value: null }, { text: "Off: nice values apply across the whole system", value: false },
            { text: "On: the kernel groups tasks by session", value: true }];
}

function switchCurrent(settings, key) {
    var v = Logic.lookup(settings.layer, ["system", key]);
    return typeof v === "boolean" ? v : null;
}

function setSwitch(layer, key, value) {
    if ((key !== "iocost" && key !== "autogroup") || (value !== null && typeof value !== "boolean"))
        return fail("BadChoice");
    return Logic.setPath(layer, ["system", key], value);
}

// ---- Profiles -------------------------------------------------------------------------

// A profile's own fields (no "_source" and the like), from the layer if it has a copy.
function fieldsOf(settings, name) {
    var own = Logic.lookup(settings.layer, ["profile", name]);
    var src = isObject(own) ? own : Logic.lookup(settings.base, ["profile", name]);
    var out = {};
    if (isObject(src))
        Object.keys(src).forEach(function (k) {
            if (k.charAt(0) !== "_")
                out[k] = src[k];
        });
    return out;
}

// What the page shows for a profile: its fields and where it comes from.
function profileView(settings, name) {
    var edited = isObject(Logic.lookup(settings.layer, ["profile", name]));
    var shipped = Logic.lookup(settings.base, ["profile", name]);
    var source = isObject(shipped) && typeof shipped._source === "string" ? shipped._source : "";
    var origin = edited ? (isObject(shipped) ? "Changed here" : "Added here")
        : (source !== "" ? "From " + Logic.clip(source, 120) : "From scdr's files");
    return { name: name, fields: fieldsOf(settings, name), edited: edited, shipped: isObject(shipped), origin: origin };
}

function inheritChoices(settings, name) {
    var out = [{ text: "Nothing", value: null }];
    settings.profiles.forEach(function (n) {
        if (n !== name)
            out.push({ text: n, value: n });
    });
    return out;
}

function listChoices(list, current) {
    var out = [{ text: "Not set", value: null }];
    list.forEach(function (v) { out.push({ text: v, value: v }); });
    if (typeof current === "string" && list.indexOf(current) < 0)
        out.push({ text: Logic.clip(current, 20), value: current });
    return out;
}

function policyChoices(current) {
    return listChoices(POLICIES, current);
}

function ioChoices(current) {
    var list = ["idle"];
    for (var i = 0; i < 8; i++)
        list.push("be:" + i);
    for (var j = 0; j < 8; j++)
        list.push("rt:" + j);
    return listChoices(list, current);
}

function checkField(settings, name, field, value) {
    if (value === undefined || value === null)
        return "";
    if (field === "inherit")
        return value !== name && profileExists(settings, value) ? "" : "BadChoice";
    if (field === "sched")
        return typeof value === "string" && new RegExp(POLICY).test(value) ? "" : "BadChoice";
    if (field === "io")
        return typeof value === "string" && new RegExp(IO).test(value) ? "" : "BadChoice";
    for (var i = 0; i < NUMBERS.length; i++)
        if (NUMBERS[i].key === field)
            return wholeIn(value, NUMBERS[i].from, NUMBERS[i].to) ? "" : "BadNumber";
    return "BadChoice";
}

// Sets one field. A profile from scdr's files is copied whole into the layer first, since a
// profile in the layer replaces the one below it.
function setField(layer, settings, name, field, value) {
    if (!profileExists(settings, name))
        return fail("NoSuchProfile");
    var bad = checkField(settings, name, field, value);
    if (bad !== "")
        return fail(bad);
    var copy = fieldsOf({ layer: layer, base: settings.base }, name);
    if (value === undefined || value === null)
        delete copy[field];
    else
        copy[field] = value;
    if (Object.keys(copy).length === 0)
        return fail("EmptyProfile");
    return Logic.setPath(layer, ["profile", name], copy);
}

function checkNewName(settings, name) {
    if (typeof name !== "string" || !new RegExp(Logic.PROFILE).test(name))
        return MESSAGES.ProfileName;
    return profileExists(settings, name) ? MESSAGES.ProfileExists : "";
}

// A new profile inherits from `from` (the one selected), or sets nice 0 when there is none:
// an empty profile wouldn't be saved.
function addProfile(layer, settings, name, from) {
    var bad = checkNewName({ layer: layer, base: settings.base }, name);
    if (bad !== "")
        return bad === MESSAGES.ProfileExists ? fail("ProfileExists") : fail("ProfileName");
    var start = typeof from === "string" && profileExists({ layer: layer, base: settings.base }, from) ? { inherit: from } : { nice: 0 };
    return Logic.setPath(layer, ["profile", name], start);
}

function usedIn(value, name) {
    if (value === name)
        return true;
    if (Array.isArray(value))
        return value.some(function (v) { return usedIn(v, name); });
    if (isObject(value))
        return Object.keys(value).some(function (k) { return usedIn(value[k], name); });
    return false;
}

// Drops the layer's copy: a profile from scdr's files goes back to it; one added here goes,
// unless the layer's rules, roles or other profiles still use it.
function removeProfile(layer, settings, name) {
    if (!goodKey(name) || !isObject(Logic.lookup(layer, ["profile", name])))
        return fail("NoSuchProfile");
    if (Logic.lookup(settings.base, ["profile", name]) === undefined) {
        var rules = Array.isArray(layer.rule) ? layer.rule : [];
        var others = isObject(layer.profile) ? Object.keys(layer.profile).filter(function (k) { return k !== name; }) : [];
        if (rules.some(function (r) { return isObject(r) && r.profile === name; }) || usedIn(layer.roles, name)
            || others.some(function (k) { return isObject(layer.profile[k]) && layer.profile[k].inherit === name; }))
            return fail("ProfileInUse");
    }
    return Logic.setPath(layer, ["profile", name], undefined);
}
