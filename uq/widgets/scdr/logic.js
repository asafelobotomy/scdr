.pragma library
// The bar widget's pure logic: reading scdr's State property, the live graphs' history (ported
// from the Plasma widget's main.qml) and the plain-words labels. No Qt calls, so Node tests it
// (tests/uq/widget.test.mjs).

var HISTORY_LENGTH = 180;
// scx_devbox's per-second samples, and the pressure recorded alongside them.
var DEVBOX_KEYS = ["wait_max", "wait_avg", "interactive", "batch", "damped", "preempt", "perf"];
var KEYS = DEVBOX_KEYS.concat(["cpu", "memory", "io"]);
// Without a devbox sample for this long, the clock records pressure instead.
var SAMPLE_GAP_MS = 3000;
var FRAME_MS = 1000 / 60;
var MAX_ROWS = 500;
var MAX_TEXT = 200;
var KERNEL_DEFAULT = "Kernel default (EEVDF)";

function clip(value, max) {
    var s = typeof value === "string" ? value : (value === null || value === undefined ? "" : String(value));
    return s.length > (max || MAX_TEXT) ? s.slice(0, max || MAX_TEXT) : s;
}

function isObject(v) {
    return v !== null && typeof v === "object" && !Array.isArray(v);
}

function num(v) {
    var n = Number(v);
    return isFinite(n) ? n : 0;
}

function optNum(v) {
    return typeof v === "number" && isFinite(v) ? v : null;
}

// ---- State ---------------------------------------------------------------------------------

// `text` is the State property: a JSON document (WidgetState in scdr's service/control.rs).
// Returns null when it can't be read; otherwise a normalised copy with only what the widget uses.
function parseState(text) {
    var v;
    try {
        v = JSON.parse(text);
    } catch (e) {
        return null;
    }
    if (!isObject(v) || typeof v.paused !== "boolean")
        return null;
    var roles = [];
    (Array.isArray(v.roles) ? v.roles : []).forEach(function (r) {
        if (Array.isArray(r) && r.length === 3 && typeof r[0] === "string" && typeof r[2] === "boolean" && roles.length < MAX_ROWS)
            roles.push({ role: clip(r[0], 40), profile: clip(r[1], 40), enabled: r[2] });
    });
    var boosted = [];
    (Array.isArray(v.boosted) ? v.boosted : []).forEach(function (p) {
        if (!isObject(p) || boosted.length >= MAX_ROWS)
            return;
        boosted.push({
            pid: typeof p.pid === "number" ? p.pid : 0,
            name: clip(p.name, 80),
            profile: clip(p.profile, 40),
            role: typeof p.role === "string" ? clip(p.role, 40) : "",
            why: clip(p.why),
            app: typeof p.app === "string" ? clip(p.app, 80) : ""
        });
    });
    var devbox = (Array.isArray(v.devbox) ? v.devbox : []).filter(function (x) {
        return isObject(x) && typeof x.seq === "number";
    });
    var scheduler = Array.isArray(v.scheduler)
        ? v.scheduler.slice(0, 20).filter(function (s) { return typeof s === "string"; }).map(function (s) { return clip(s); })
        : [];
    return {
        paused: v.paused,
        managed: typeof v.managed === "number" ? v.managed : 0,
        powerProfile: typeof v.power_profile === "string" ? clip(v.power_profile, 40) : "",
        cpuPressure: optNum(v.cpu_pressure),
        memoryPressure: optNum(v.memory_pressure),
        ioPressure: optNum(v.io_pressure),
        pressureActive: v.pressure_active === true,
        scheduler: scheduler,
        roles: roles,
        agents: typeof v.agents === "number" ? v.agents : 0,
        boosted: boosted,
        devbox: devbox
    };
}

// The running scheduler from scdr's status lines ("sched_ext: devbox (started outside scdr; left
// alone)", "sched_ext: bpfland:gaming (by scdr)", "eevdf: base_slice=…"): {name, note}.
function schedulerInfo(lines) {
    var list = Array.isArray(lines) ? lines : [];
    for (var i = 0; i < list.length; i++) {
        var line = String(list[i]);
        if (line.indexOf("sched_ext: ") !== 0)
            continue;
        var rest = line.slice(11);
        if (rest.indexOf("scx_loader unavailable") === 0)
            return { name: KERNEL_DEFAULT, note: "scx_loader isn't available" };
        var m = rest.match(/^([A-Za-z0-9_.-]+)(?::([a-z]+))?/);
        if (!m)
            continue;
        var name = m[1].replace(/^scx_/, "").replace(/_\d[\w.-]*$/, "");
        var paren = rest.match(/\(([^)]*)\)\s*$/);
        var note = paren ? clip(paren[1], 120) : "";
        if (name === "eevdf")
            return { name: KERNEL_DEFAULT, note: note };
        var label = name === "devbox" ? "scx_devbox" : name;
        if (m[2] && m[2] !== "auto")
            label += " (" + m[2] + ")";
        return { name: clip(label, 60), note: note };
    }
    return { name: KERNEL_DEFAULT, note: "" };
}

function statusLabel(state, pending) {
    if (pending)
        return "Waiting for your password…";
    if (!state)
        return "Connecting…";
    return state.paused ? "Paused" : "Running";
}

function roleLabel(role) {
    return { game: "Games", audio: "Audio", foreground: "Focused app", background: "Background demotion" }[role] || role;
}

// One row per app (its app.slice unit), or per process name outside apps, as in Plasma.
function grouped(boosted) {
    var rows = {}, order = [];
    (boosted || []).forEach(function (p) {
        var name = p.app || p.name;
        var key = name + "\u0000" + p.profile;
        if (!rows[key]) {
            rows[key] = { name: name, profile: p.profile, role: p.role, why: p.why, pids: [] };
            order.push(key);
        }
        rows[key].pids.push(p.pid);
    });
    return order.map(function (k) { return rows[k]; }).sort(function (a, b) {
        return a.name < b.name ? -1 : a.name > b.name ? 1 : 0;
    });
}

function boostedLine(row) {
    return row.pids.length > 1 ? row.name + " ×" + row.pids.length + " — " + row.profile
                               : row.name + " — " + row.profile;
}

function pressureLine(state) {
    if (!state || state.cpuPressure === null)
        return "–";
    return Math.round(state.cpuPressure) + "%" + (state.pressureActive ? " (demoting background apps)" : "");
}

// ---- Writes ----------------------------------------------------------------------------------

// What a Pause/Resume/SetRoleEnabled answer means. The bridge gives up after 2 s, but scdrd's
// polkit check may still be waiting for the password: the change lands later, through State.
function callOutcome(result) {
    if (!result || !result.error)
        return { status: "ok", message: "" };
    var e = String(result.error);
    if (/within 2 s|deadline|timed? ?out/i.test(e))
        return { status: "waiting", message: "" };
    if (/not ?authori[sz]ed|polkit|AccessDenied|dismissed/i.test(e))
        return { status: "error", message: "Not allowed: scdr's changes need the org.scdr.control password." };
    if (/not running|ServiceUnknown|NameHasNoOwner/i.test(e))
        return { status: "error", message: "scdrd isn't running." };
    return { status: "error", message: "scdr couldn't make that change: " + clip(e, 160) };
}

// Whether a change still waiting for the password has shown up in `state`.
// pending: {kind: "pause"|"resume"} or {kind: "role", role, enabled}.
function settled(pending, state) {
    if (!pending || !state)
        return false;
    if (pending.kind === "pause")
        return state.paused === true;
    if (pending.kind === "resume")
        return state.paused === false;
    if (pending.kind === "role")
        return state.roles.some(function (r) { return r.role === pending.role && r.enabled === pending.enabled; });
    return true;
}

// ---- History ---------------------------------------------------------------------------------

function newHistory(length) {
    var h = { length: length || HISTORY_LENGTH, lastSeq: 0, lastSampleAt: 0, devbox: false, data: {} };
    KEYS.forEach(function (k) {
        var a = [];
        for (var i = 0; i < h.length; i++)
            a.push(0);
        h.data[k] = a;
    });
    return h;
}

function push(h, key, value) {
    var a = h.data[key];
    a.push(num(value));
    while (a.length > h.length)
        a.shift();
}

function pushPressure(h, state) {
    push(h, "cpu", state.cpuPressure);
    push(h, "memory", state.memoryPressure);
    push(h, "io", state.ioPressure);
}

// Each State update carries scdrd's last few scx_devbox samples (one a second; those already
// recorded are skipped by sequence number) and the current pressure. Returns how many samples
// were added.
function record(h, state, now) {
    var samples = state && Array.isArray(state.devbox) ? state.devbox : [];
    // The loader restarted: its sequence starts again.
    if (samples.length && samples[samples.length - 1].seq < h.lastSeq)
        h.lastSeq = 0;
    var fresh = samples.filter(function (x) { return x.seq > h.lastSeq; });
    fresh.forEach(function (x) {
        push(h, "wait_max", x.wait_max_ms);
        push(h, "wait_avg", x.wait_avg_ms);
        push(h, "interactive", x.interactive);
        push(h, "batch", x.batch);
        push(h, "damped", x.damped);
        push(h, "preempt", x.preempt);
        push(h, "perf", x.perf_pct);
        h.lastSeq = x.seq;
        // Pressure once per sample, so all graphs share one time scale.
        pushPressure(h, state);
    });
    if (fresh.length) {
        h.devbox = true;
        h.lastSampleAt = now;
    }
    return fresh.length;
}

// The one-second clock: without devbox samples State only changes when a value does, so record
// pressure here and keep devbox's graphs moving with zeros. Returns whether it recorded.
function tick(h, state, now) {
    if (!state || now - h.lastSampleAt < SAMPLE_GAP_MS)
        return false;
    DEVBOX_KEYS.forEach(function (k) { push(h, k, 0); });
    pushPressure(h, state);
    return true;
}

function last(h, key) {
    var a = h.data[key];
    return a && a.length ? a[a.length - 1] : 0;
}

function peak(h, key) {
    var a = h.data[key];
    return a && a.length ? Math.max.apply(null, a) : 0;
}

// The wait graph's scale: at least a frame and a bit, more for spikes.
function waitScale(h) {
    return Math.max(20, Math.ceil(peak(h, "wait_max") * 1.15));
}

// A round top for a graph whose peak is `v`: 1, 2 or 5 × 10^n, at least `min`.
function niceMax(v, min) {
    var top = Math.max(num(v), min || 10);
    var p = Math.pow(10, Math.floor(Math.log(top) / Math.LN10));
    var steps = [1, 2, 5, 10];
    for (var i = 0; i < steps.length; i++)
        if (steps[i] * p >= top)
            return steps[i] * p;
    return 10 * p;
}

function schedScale(h) {
    return niceMax(Math.max(peak(h, "interactive"), peak(h, "batch"), peak(h, "damped")), 10);
}

// The last `n` values of `key`, for the bar glyph.
function tail(h, key, n) {
    var a = h.data[key] || [];
    return a.slice(Math.max(0, a.length - n));
}

function fixed(v, digits) {
    return num(v).toFixed(digits);
}
