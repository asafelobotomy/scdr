.pragma library
.import "logic.js" as Logic
// What the commands in page.toml answer, read for the page: `scdrctl config show`, the previews
// (`--dry-run --json`), `check`, `list`, the benchmark's progress lines, and D-Bus Status and
// Explain. No Qt calls, so Node tests it. Every text is clipped; the page shows it as plain text.

var KEY = /^[a-z_]+(\.[A-Za-z0-9_-]+){1,3}$/;
var TYPES = ["bool", "uint", "float", "text", "list"];
var MAX_PLAN_ROWS = 500;

function isObject(v) {
    return Logic.isObject(v);
}

function clip(v, n) {
    return Logic.clip(v, n || 200);
}

// {ok, output, error} for a command that prints one JSON document, whatever its exit code: an
// `{ok: false, error}` envelope on a failure, or {error} when it didn't run.
function envelope(result) {
    if (!result)
        return { ok: false, output: null, error: "Nothing came back." };
    if (result.cancelled)
        return { ok: false, output: null, error: "", cancelled: true };
    var out = result.output;
    if (isObject(out) && out.ok === false)
        return { ok: false, output: out, error: clip(out.error || "scdr refused that.", 400) };
    if (result.error && !isObject(out))
        return { ok: false, output: null, error: Logic.writeError(result) };
    if (result.error)
        return { ok: false, output: out, error: Logic.writeError(result) };
    return { ok: true, output: out, error: "" };
}

function isCancelled(result) {
    return !!(result && result.cancelled);
}

// ---- config show ---------------------------------------------------------------------

// The `config` read as {ready, error, file, rows, layer}. `rows` are the settings, in order.
function configView(result) {
    var env = envelope(result);
    if (!env.ok || !isObject(env.output) || !Array.isArray(env.output.settings))
        return { ready: false, error: env.error || "scdr's settings couldn't be read.", file: "", rows: [], layer: { rules: 0, profiles: 0, empty: true } };
    var rows = [];
    env.output.settings.slice(0, 64).forEach(function (s) {
        if (!isObject(s) || typeof s.key !== "string" || !KEY.test(s.key) || TYPES.indexOf(s.type) < 0)
            return;
        var row = {
            key: s.key,
            type: s.type,
            group: clip(s.group, 60),
            label: clip(s.label, 80),
            help: clip(s.help, 300),
            value: s.value === undefined ? null : s.value,
            source: s.source === "you" || s.source === "files" ? s.source : "default",
            fallback: s.default === undefined ? null : s.default,
            nullable: s.nullable === true,
            unit: typeof s.unit === "string" ? clip(s.unit, 8) : "",
            from: typeof s.min === "number" ? s.min : 0,
            to: typeof s.max === "number" ? s.max : 1000000,
            choices: Array.isArray(s.choices) ? s.choices.filter(function (c) { return typeof c === "string"; }).slice(0, 12) : [],
            inherited: Array.isArray(s.inherited) ? s.inherited.map(function (x) { return clip(x, 80); }).slice(0, 100) : []
        };
        if (row.type === "list")
            row.value = Array.isArray(s.value) ? s.value.map(function (x) { return clip(x, 80); }).slice(0, 100) : [];
        rows.push(row);
    });
    var l = isObject(env.output.layer) ? env.output.layer : {};
    return {
        ready: true,
        error: "",
        file: typeof env.output.layer_file === "string" ? clip(env.output.layer_file, 200) : "",
        rows: rows,
        layer: { rules: +l.rules || 0, profiles: +l.profiles || 0, empty: l.empty !== false }
    };
}

// A row to show while the real one isn't there (the settings were read again without it).
function emptyRow(key) {
    return { key: key, type: "text", group: "", label: key, help: "", value: null, source: "default", fallback: null, nullable: false, unit: "", from: 0, to: 1000000, choices: [], inherited: [] };
}

function rowsIn(view, groups) {
    return view.rows.filter(function (r) { return groups.indexOf(r.group) >= 0; });
}

// A list as one line of text, and back.
function listText(list) {
    return list.join(", ");
}

// What `config set` takes for `value`: lists as comma-separated text, an empty list as "[]".
function setArg(row, value) {
    if (row.type === "list") {
        var text = Array.isArray(value) ? listText(value) : String(value || "");
        return text.trim() === "" ? "[]" : text;
    }
    if (typeof value === "boolean")
        return value ? "on" : "off";
    return String(value);
}

// The one-line problem with arguments for `config-set`, or "": the same rules page.toml has.
function setProblem(key, value) {
    if (!KEY.test(key))
        return "That setting isn't known.";
    if (typeof value !== "string" || value === "" || value.length > 400)
        return "Type a value of up to 400 characters.";
    if (value.charAt(0) === "-" || /[\x00-\x1f\x7f]/.test(value))
        return "Use letters, numbers, spaces and , . % only.";
    return "";
}

function sourceText(row) {
    if (row.source === "you")
        return "Set here";
    if (row.source === "files")
        return "From scdr's files";
    return "Default";
}

function valueText(row) {
    var v = row.value;
    var unit = row.unit !== "" ? " " + row.unit : "";
    if (row.type === "list")
        return v.length === 0 ? "none" : listText(v);
    if (v === null || v === undefined)
        return row.fallback !== null && row.fallback !== undefined ? (typeof row.fallback === "boolean" ? (row.fallback ? "On" : "Off") : String(row.fallback) + unit) : "not set";
    if (typeof v === "boolean")
        return v ? "On" : "Off";
    return String(v) + unit;
}

// ---- Previews: {summary, changes[], would_refuse, reason} ---------------------------------

// `result` of a preview command: the plan, or {problem} when it didn't answer one.
function planOf(result) {
    var env = envelope(result);
    var p = env.output;
    if (!isObject(p) || typeof p.summary !== "string" || !Array.isArray(p.changes))
        return { problem: env.error || "scdr couldn't say what this would do." };
    var changes = [];
    p.changes.slice(0, MAX_PLAN_ROWS).forEach(function (c) {
        if (!isObject(c) || typeof c.label !== "string")
            return;
        changes.push({
            group: typeof c.group === "string" && c.group !== "" ? clip(c.group, 60) : "Changes",
            label: clip(c.label, 160),
            detail: typeof c.detail === "string" ? clip(c.detail, 200) : "",
            reversible: c.reversible === true ? true : (c.reversible === false ? false : null)
        });
    });
    return { summary: clip(p.summary, 200), changes: changes, more: Math.max(0, p.changes.length - MAX_PLAN_ROWS),
             refuses: p.would_refuse === true, reason: clip(p.reason, 300), problem: "" };
}

// The changes of a plan grouped in order of first appearance: [{title, items[]}].
function planGroups(plan) {
    var groups = [];
    var at = {};
    (plan.changes || []).forEach(function (c) {
        if (at[c.group] === undefined) {
            at[c.group] = groups.length;
            groups.push({ title: c.group, items: [] });
        }
        groups[at[c.group]].items.push(c);
    });
    return groups;
}

function changeText(c) {
    var how = c.reversible === true ? " (can be undone)" : (c.reversible === false ? " (can't be undone)" : "");
    return c.label + (c.detail !== "" ? ": " + c.detail : "") + how;
}

// What removing one of the person's rules does, as a plan the page's own question shows.
function removeRulePlan(rule) {
    return {
        summary: "Remove your rule for " + rule.name,
        changes: [{ group: "Removed", label: rule.name, detail: "no longer sent to the " + rule.profile + " profile", reversible: false }],
        more: 0, refuses: false, reason: "", problem: ""
    };
}

// What dropping a profile does: the rules and roles that use it, or what comes back.
function removeProfilePlan(settings, name, shipped) {
    var layer = settings ? settings.layer : {};
    var rules = (settings ? settings.layerRules : []).filter(function (r) { return r.profile === name; });
    var changes = [];
    if (shipped)
        changes.push({ group: "Back to scdr's files", label: name, detail: "your changes to this profile are dropped", reversible: false });
    else
        changes.push({ group: "Removed", label: name, detail: "your profile is deleted", reversible: false });
    rules.forEach(function (r) {
        changes.push({ group: "Still using it", label: r.name, detail: shipped ? "keeps using the profile from scdr's files" : "needs another profile first", reversible: null });
    });
    var roles = layer && isObject(layer.roles) ? Object.keys(layer.roles).filter(function (k) { return layer.roles[k] === name; }) : [];
    roles.forEach(function (r) {
        changes.push({ group: "Still using it", label: "Role: " + r, detail: "", reversible: null });
    });
    return { summary: shipped ? "Undo your changes to " + name : "Delete the profile " + name, changes: changes, more: 0, refuses: false, reason: "", problem: "" };
}

// ---- check, list, import ---------------------------------------------------------------

function checkView(result) {
    var env = envelope(result);
    if (!env.ok)
        return { ok: false, text: env.error || "The configuration doesn't load.", warnings: [], files: [] };
    var o = isObject(env.output) ? env.output : {};
    return {
        ok: true,
        text: (+o.profiles || 0) + " profiles and " + (+o.rules || 0) + " rules load.",
        warnings: Array.isArray(o.warnings) ? o.warnings.slice(0, 50).map(function (w) { return clip(w, 300); }) : [],
        files: Array.isArray(o.files) ? o.files.slice(0, 100).map(function (f) { return clip(f, 200); }) : []
    };
}

// `list --json`: [{pid, name, profile, rule, excepted}], at most `max`, filtered by `text`.
function processesView(result, text, max) {
    var env = envelope(result);
    if (!env.ok || !isObject(env.output) || !Array.isArray(env.output.processes))
        return { ok: false, problem: env.error || "The process list couldn't be read.", rows: [], total: 0 };
    var q = String(text || "").trim().toLowerCase();
    var all = env.output.processes.filter(function (p) {
        return isObject(p) && typeof p.name === "string" && (q === "" || p.name.toLowerCase().indexOf(q) >= 0 || String(p.profile).toLowerCase().indexOf(q) >= 0);
    });
    var rows = all.slice(0, max || 100).map(function (p) {
        return { pid: +p.pid || 0, name: clip(p.name, 40), profile: clip(p.profile, 40), rule: clip(p.rule, 80), excepted: p.excepted === true };
    });
    return { ok: true, problem: "", rows: rows, total: all.length };
}

var PATH = /^\/[^\x00-\x1f\x7f]{1,400}$/;

function importProblem(path) {
    if (typeof path !== "string" || path.trim() === "")
        return "Type the folder or file to read.";
    if (!PATH.test(path.trim()))
        return "Use a full path starting with /.";
    return "";
}

// The command line to run as an administrator to write the rules (the page can't: /etc is root's).
function importCommand(kind, path, only) {
    var args = ["scdrctl", "import", kind];
    if (kind === "ananicy" && only)
        args.push("--only", only);
    args.push(path, "--output", "/etc/scdr/rules.d/50-" + kind + ".toml");
    return "sudo " + args.join(" ");
}

// ---- Benchmark progress ----------------------------------------------------------------

function startBench() {
    return { percent: 0, line: "Starting…", done: false, results: [] };
}

// Each ndjson line {event: "scenario", name, round, rounds, percent} or {event: "done", results}.
function benchReduce(state, ev) {
    if (!isObject(ev))
        return state;
    if (ev.event === "scenario")
        return { percent: Math.max(0, Math.min(100, +ev.percent || 0)), line: clip(ev.name, 80) + (+ev.rounds > 1 ? " (round " + (+ev.round || 1) + " of " + (+ev.rounds) + ")" : ""), done: false, results: [] };
    if (ev.event === "done")
        return { percent: 100, line: "Done.", done: true, results: benchRows(ev.results) };
    return state;
}

function benchRows(results) {
    if (!Array.isArray(results))
        return [];
    return results.slice(0, 20).filter(isObject).map(function (r) {
        var name = isObject(r.scenario) ? r.scenario.name : "";
        var num = function (v) { return typeof v === "number" ? Math.round(v) + " µs" : "-"; };
        return { name: clip(name, 60), p50: num(r.p50_us), p99: num(r.p99_us), max: num(r.max_us) };
    });
}

// What `bench --check --json` said: {can, reason, seconds}.
function benchCheck(result) {
    var env = envelope(result);
    if (!env.ok || !isObject(env.output))
        return { can: false, reason: env.error || "scdr couldn't check.", seconds: 0 };
    return { can: env.output.can_run === true, reason: clip(env.output.reason, 300), seconds: +env.output.seconds || 0 };
}

// ---- D-Bus Status and Explain ----------------------------------------------------------

function parseJsonValue(result) {
    if (!result || result.error || typeof result.value !== "string")
        return null;
    var p = Logic.parseJson(result.value);
    return p.error || !isObject(p.value) ? null : p.value;
}

// Facts for the Overview: [{label, text}].
function statusFacts(result) {
    var s = parseJsonValue(result);
    if (s === null)
        return [];
    var facts = [];
    var add = function (label, text) {
        if (text !== "" && text !== null && text !== undefined)
            facts.push({ label: label, text: clip(text, 160) });
    };
    add("scdr", typeof s.version === "string" ? s.version + (s.dry_run === true ? " (dry run)" : "") : "");
    add("Running for", typeof s.uptime_secs === "number" ? uptimeText(s.uptime_secs) : "");
    add("Power profile", s.power_profile);
    add("Processes", typeof s.tracked === "number" ? s.tracked + " seen, " + (+s.managed || 0) + " managed, " + (+s.excepted || 0) + " left alone" : "");
    var gov = Array.isArray(s.governors) ? s.governors : [];
    add("CPU governor", gov.join(", "));
    var drv = Array.isArray(s.cpufreq_driver) ? s.cpufreq_driver : [];
    add("CPU frequency driver", typeof drv[0] === "string" ? drv[0] + (typeof drv[1] === "string" ? " (" + drv[1] + ")" : "") : "");
    add("Memory pressure", typeof s.memory_pressure === "number" ? Math.round(s.memory_pressure) + " %" : "");
    add("Disk pressure", typeof s.io_pressure === "number" ? Math.round(s.io_pressure) + " %" : "");
    (Array.isArray(s.disks) ? s.disks.slice(0, 12) : []).forEach(function (d) {
        if (isObject(d))
            add("Disk " + clip(d.name || "", 20), clip(d.scheduler || "", 60) + (d.iocost === true ? ", iocost on" : ""));
    });
    (Array.isArray(s.notes) ? s.notes.slice(0, 10) : []).forEach(function (n) {
        if (Array.isArray(n) && n.length === 2)
            add(clip(n[0], 40), n[1]);
    });
    return facts;
}

function uptimeText(secs) {
    if (secs < 90)
        return secs + " s";
    if (secs < 5400)
        return Math.round(secs / 60) + " min";
    if (secs < 172800)
        return Math.round(secs / 3600) + " h";
    return Math.round(secs / 86400) + " days";
}

// `Explain(pid)`: [{label, text}] for the fields that have something to say, or {problem}.
function explainView(result) {
    var e = parseJsonValue(result);
    if (e === null)
        return { ok: false, problem: result && result.error ? Logic.writeError(result) : "scdr couldn't explain that process.", rows: [], notes: [] };
    if (e.name === null || e.name === undefined)
        return { ok: false, problem: "No process has that number.", rows: [], notes: [] };
    var rows = [];
    var add = function (label, text) {
        if (typeof text === "string" && text !== "")
            rows.push({ label: label, text: clip(text, 240) });
    };
    add("Name", e.name);
    add("State", e.state);
    add("Why", e.reason);
    add("Profile", e.profile);
    if (Array.isArray(e.profile_chain) && e.profile_chain.length > 1)
        add("Inherits from", e.profile_chain.slice(1).join(" → "));
    if (isObject(e.rule))
        add("Rule", [typeof e.rule.source === "string" ? e.rule.source.split("/").pop() : "", e.rule.index !== undefined ? "#" + e.rule.index : "", typeof e.rule.conditions === "string" ? "(" + e.rule.conditions + ")" : ""].filter(function (x) { return x; }).join(" "));
    add("Settings", e.settings);
    add("Original", e.original);
    add("Now", e.current);
    add("Unit", e.unit);
    add("Program", e.exe);
    var notes = [].concat(Array.isArray(e.notes) ? e.notes : [], Array.isArray(e.errors) ? e.errors : []).slice(0, 12).map(function (n) { return clip(n, 300); });
    return { ok: true, problem: "", rows: rows, notes: notes };
}
