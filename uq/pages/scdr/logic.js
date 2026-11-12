.pragma library
// The Scheduling page's pure logic: parsing what scdr reports, editing its settings layer,
// checking what the person types, and the plain-words messages. No Qt calls, so Node tests it.

var RULE_NAME = "^[A-Za-z0-9][A-Za-z0-9._+*?@ -]{0,63}$";
var PROFILE = "^[a-z0-9][a-z0-9_-]{0,39}$";
var CHOICE = "^(eevdf|(?!eevdf:)[A-Za-z0-9_]{1,32}(:(auto|gaming|powersave|lowlatency|server))?)$";

// uQ refuses D-Bus arguments over 16 KiB of JSON; scdr itself accepts far more (F19).
var MAX_DOCUMENT = 15 * 1024;
var MAX_ARGS_JSON = 16 * 1024;
var MAX_ROWS = 500;
var MAX_TEXT = 200;

var MESSAGES = {
    BadScheduler: "That isn't a scheduler scdr knows.",
    BadName: "Use letters, numbers, spaces and . _ + * ? @ - only, starting with a letter or number. 64 characters at most.",
    BadProfile: "Pick one of the profiles in the list.",
    Duplicate: "You already have a rule for that app and profile.",
    Changed: "Your rules changed elsewhere; refresh and try again.",
    NoSuchRule: "That rule is gone; refresh and try again.",
    Unreadable: "scdr's settings look unusual here; change them in scdr's own settings.",
    TooBig: "Too many settings to change here; use scdr's own settings."
};

function fail(code) {
    return { error: MESSAGES[code], code: code };
}

function clip(value, max) {
    var s = (typeof value === "string") ? value : (value === null || value === undefined ? "" : String(value));
    return s.length > (max || MAX_TEXT) ? s.slice(0, max || MAX_TEXT) : s;
}

function isObject(v) {
    return v !== null && typeof v === "object" && !Array.isArray(v);
}

function parseJson(text) {
    try {
        return { value: JSON.parse(text) };
    } catch (e) {
        return { error: "scdr's answer couldn't be read." };
    }
}

// ---- What scdr reports ----------------------------------------------------------------

// `text` is the State property: a JSON document (WidgetState in scdr's service/control.rs).
function parseState(text) {
    var p = parseJson(text);
    if (p.error || !isObject(p.value) || typeof p.value.paused !== "boolean")
        return { error: "scdr's state couldn't be read." };
    var v = p.value;
    var roles = [];
    var raw = Array.isArray(v.roles) ? v.roles : [];
    for (var i = 0; i < raw.length && roles.length < MAX_ROWS; i++) {
        var r = raw[i];
        if (!Array.isArray(r) || r.length !== 3 || typeof r[0] !== "string" || typeof r[1] !== "string" || typeof r[2] !== "boolean")
            return { error: "scdr's state couldn't be read." };
        roles.push({ role: clip(r[0]), profile: clip(r[1]), enabled: r[2] });
    }
    var boosted = [];
    var b = Array.isArray(v.boosted) ? v.boosted : [];
    for (var j = 0; j < b.length && boosted.length < MAX_ROWS; j++) {
        if (!isObject(b[j]))
            continue;
        boosted.push({ pid: typeof b[j].pid === "number" ? b[j].pid : 0, name: clip(b[j].name), profile: clip(b[j].profile), why: clip(b[j].why) });
    }
    return {
        paused: v.paused,
        managed: typeof v.managed === "number" ? v.managed : 0,
        powerProfile: typeof v.power_profile === "string" ? clip(v.power_profile) : "",
        cpuPressure: typeof v.cpu_pressure === "number" ? v.cpu_pressure : null,
        pressureActive: v.pressure_active === true,
        scheduler: Array.isArray(v.scheduler) ? v.scheduler.slice(0, 20).map(function (s) { return clip(s); }) : [],
        roles: roles,
        agents: typeof v.agents === "number" ? v.agents : 0,
        boosted: boosted
    };
}

// The result of the watched read of State (busctl's JSON: {type, data}), as a view for the page:
// {status: "ready", state} | {status: "not-running", detail} | {status: "error", message, detail}.
function readState(result) {
    if (!result)
        return { status: "error", message: "scdr's state couldn't be read.", detail: "" };
    if (result.error) {
        if (notRunning(result.error))
            return { status: "not-running", detail: clip(result.error, 400) };
        return { status: "error", message: "scdr's state couldn't be read.", detail: clip(result.error, 400) };
    }
    var out = result.output;
    var s = isObject(out) && typeof out.data === "string" ? parseState(out.data) : { error: "scdr's state couldn't be read." };
    if (s.error)
        return { status: "error", message: s.error, detail: "" };
    return { status: "ready", state: s };
}

function normChoice(v) {
    if (typeof v !== "string" || v === "")
        return "";
    var s = v.indexOf("scx_") === 0 ? v.slice(4) : v;
    return s.length > 5 && s.slice(-5) === ":auto" ? s.slice(0, -5) : s;
}

function choiceLabel(v) {
    var s = normChoice(v);
    if (s === "" || s === "eevdf")
        return "Kernel default";
    var i = s.indexOf(":");
    return i < 0 ? s : s.slice(0, i) + " (" + s.slice(i + 1) + ")";
}

function ruleNameOf(rule) {
    if (!isObject(rule))
        return null;
    var n = rule.name;
    if (typeof n === "string")
        return n;
    if (Array.isArray(n) && n.length === 1 && typeof n[0] === "string")
        return n[0];
    return null;
}

var CONDITIONS = ["name", "exe", "cmdline", "cgroup", "parent", "ancestor", "uid",
    "not_name", "not_exe", "not_cmdline", "not_cgroup", "not_parent", "not_ancestor"];

function describeRule(rule) {
    var parts = [];
    for (var i = 0; i < CONDITIONS.length; i++) {
        var v = rule[CONDITIONS[i]];
        if (v !== undefined)
            parts.push(CONDITIONS[i] + " = " + (Array.isArray(v) ? v.join(", ") : String(v)));
    }
    return clip(parts.join("; ") || "everything", 120);
}

var POWER_PROFILES = ["power-saver", "balanced", "performance"];
var KEY = "^[A-Za-z0-9_-]{1,64}$";

function conditionsOf(rule) {
    var out = [];
    for (var i = 0; i < CONDITIONS.length; i++) {
        var v = rule[CONDITIONS[i]];
        if (v !== undefined)
            out.push({ field: CONDITIONS[i], text: clip(Array.isArray(v) ? v.join(", ") : String(v), 1000) });
    }
    return out;
}

function trialOf(t) {
    if (!isObject(t) || typeof t.trying !== "string")
        return null;
    return { trying: choiceLabel(t.trying), was: choiceLabel(typeof t.was === "string" ? t.was : "eevdf") };
}

function parseSettings(text) {
    var p = parseJson(text);
    if (p.error || !isObject(p.value))
        return { error: "scdr's settings couldn't be read." };
    var s = p.value;
    var layer = isObject(s.layer) ? s.layer : {};
    var base = isObject(s.base) ? s.base : {};
    var rules = [];
    var raw = Array.isArray(layer.rule) ? layer.rule : [];
    for (var i = 0; i < raw.length && rules.length < MAX_ROWS; i++) {
        var r = raw[i];
        if (!isObject(r))
            continue;
        var name = ruleNameOf(r);
        var simple = name !== null && Object.keys(r).length === 2 && typeof r.profile === "string";
        rules.push({
            index: i,
            key: JSON.stringify(r),
            name: clip(name !== null ? name : describeRule(r), 120),
            rawName: name !== null ? name : "",
            profile: clip(r.profile),
            simple: simple,
            conditions: conditionsOf(r)
        });
    }
    var names = {};
    [base.profile, layer.profile].forEach(function (t) {
        if (isObject(t))
            Object.keys(t).forEach(function (k) { names[k] = true; });
    });
    var installed = [];
    var cards = [];
    (Array.isArray(s.schedulers) ? s.schedulers : []).slice(0, 64).forEach(function (sc) {
        if (!isObject(sc) || typeof sc.name !== "string" || !new RegExp(KEY).test(sc.name))
            return;
        var modes = (Array.isArray(sc.modes) ? sc.modes : []).filter(function (m) {
            return typeof m === "string" && new RegExp(KEY).test(m);
        });
        if (modes.length === 0)
            modes = ["auto"];
        cards.push({ name: sc.name, installed: sc.installed === true, modes: modes,
                     description: clip(typeof sc.description === "string" ? sc.description : "", 400) });
        if (sc.installed !== true)
            return;
        modes.forEach(function (m) {
            var value = m === "auto" ? sc.name : sc.name + ":" + m;
            installed.push({ value: clip(value, 80), label: choiceLabel(value) });
        });
    });
    var power = (Array.isArray(s.power_profiles) ? s.power_profiles : POWER_PROFILES).filter(function (x) {
        return typeof x === "string" && new RegExp(KEY).test(x);
    }).slice(0, 8);
    var le = isObject(layer.sched_ext) ? layer.sched_ext : {};
    var be = isObject(base.sched_ext) ? base.sched_ext : {};
    return {
        layer: layer,
        base: base,
        layerRules: rules,
        profiles: Object.keys(names).sort(),
        installed: installed,
        cards: cards,
        powerProfiles: power,
        schedExt: le.enable !== undefined ? le.enable === true : be.enable === true,
        currentScheduler: typeof s.current_scheduler === "string" ? choiceLabel(s.current_scheduler) : "",
        trial: trialOf(s.trial),
        loaderError: typeof s.scx_loader_error === "string" ? clip(s.scx_loader_error, 400) : ""
    };
}

// ---- Values in the settings, by path (as the Plasma widget's SettingsStore) ------------

// The value at `path` (a list of keys) in `obj`, or undefined.
function lookup(obj, path) {
    var v = obj;
    for (var i = 0; i < path.length; i++) {
        if (!isObject(v) || !Object.prototype.hasOwnProperty.call(v, path[i]))
            return undefined;
        v = v[path[i]];
    }
    return v;
}

function badPath(path) {
    if (!Array.isArray(path) || path.length === 0 || path.length > 8)
        return true;
    return path.some(function (k) {
        return typeof k !== "string" || !new RegExp(KEY).test(k) || k === "__proto__" || k === "constructor" || k === "prototype";
    });
}

// Sets `value` at `path` in the layer. undefined (or null) removes it, so the files below
// apply again, along with any table it leaves empty.
function setPath(layer, path, value) {
    if (badPath(path) || !isObject(layer))
        return fail("Unreadable");
    var remove = value === undefined || value === null;
    var obj = layer;
    var parents = [];
    for (var i = 0; i < path.length - 1; i++) {
        var k = path[i];
        if (!isObject(obj[k])) {
            if (remove)
                return { ok: true };
            obj[k] = {};
        }
        parents.push([obj, k]);
        obj = obj[k];
    }
    var last = path[path.length - 1];
    if (!remove) {
        obj[last] = value;
        return { ok: true };
    }
    delete obj[last];
    for (var j = parents.length - 1; j >= 0; j--) {
        var parent = parents[j][0], key = parents[j][1];
        if (Object.keys(parent[key]).length !== 0)
            break;
        delete parent[key];
    }
    return { ok: true };
}

// ---- Edits to the settings layer (same rules as scdr's own settings) ------------------

function addRule(layer, profile, name) {
    if (checkRuleName(name) !== "")
        return fail("BadName");
    if (checkProfile(profile) !== "")
        return fail("BadProfile");
    if (layer.rule !== undefined && !Array.isArray(layer.rule))
        return fail("Unreadable");
    var rules = layer.rule || [];
    for (var i = 0; i < rules.length; i++)
        if (isObject(rules[i]) && rules[i].profile === profile && ruleNameOf(rules[i]) === name)
            return fail("Duplicate");
    rules.push({ profile: profile, name: name });
    layer.rule = rules;
    return { ok: true, index: rules.length - 1 };
}

function utf8Length(s) {
    try {
        return unescape(encodeURIComponent(s)).length;
    } catch (e) {
        return Infinity;
    }
}

// Re-reads the `Settings` answer, lets `edit(layer, settings)` change a copy of the layer
// (`settings` is the whole answer, for the files below it), and returns
// {doc} (the text to send to SetSettings) or {error}. Refuses a document uQ couldn't pass on.
function applyEdit(settingsText, edit) {
    var p = parseJson(settingsText);
    if (p.error || !isObject(p.value))
        return { error: "scdr's settings couldn't be read." };
    var layer = isObject(p.value.layer) ? JSON.parse(JSON.stringify(p.value.layer)) : {};
    var res = edit(layer, p.value);
    if (res.error)
        return res;
    var doc = JSON.stringify(layer);
    if (utf8Length(doc) > MAX_DOCUMENT || utf8Length(JSON.stringify([doc])) > MAX_ARGS_JSON)
        return fail("TooBig");
    return { doc: doc, index: res.index };
}

// ---- What the person types ------------------------------------------------------------

function checkRuleName(s) {
    if (typeof s !== "string" || s === "")
        return "Type the app's name.";
    if (s.length > 64)
        return "Use 64 characters or fewer.";
    return new RegExp(RULE_NAME).test(s) ? "" : MESSAGES.BadName;
}

function checkProfile(s) {
    return typeof s === "string" && new RegExp(PROFILE).test(s) ? "" : MESSAGES.BadProfile;
}

// `systemctl --user is-enabled` as {code, output}: true, false, or null when unknown
// (masked, static, or the read failed).
function agentEnabled(result) {
    if (!result || result.error && typeof result.output !== "string")
        return null;
    var word = String(result.output).trim();
    if (word === "enabled")
        return true;
    if (word === "disabled")
        return false;
    return null;
}

function notRunning(error) {
    return /ServiceUnknown|NameHasNoOwner|not activatable|not provided by any|is not running|no owner|name .* not exist/i.test(String(error || ""));
}

function writeError(result) {
    var e = result && result.error ? String(result.error) : "";
    if (/AccessDenied|polkit|not authori[sz]ed|authori[sz]ation/i.test(e))
        return "You need to enter your password to change this.";
    if (notRunning(e))
        return "scdr isn't running.";
    var first = e.split("\n")[0].trim();
    return first === "" ? "Something went wrong." : clip(first, MAX_TEXT);
}
