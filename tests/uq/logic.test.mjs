import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { loadLogic } from "./load-logic.mjs";

const L = loadLogic();
const dir = new URL("../../uq/pages/scdr/", import.meta.url);
const fixture = (name) => readFileSync(new URL(`./fixtures/${name}`, import.meta.url), "utf8");
const manifest = readFileSync(new URL("page.toml", dir), "utf8");
// The logic runs in its own realm; compare plain copies.
const plain = (v) => JSON.parse(JSON.stringify(v));

// A minimal reader for the parts of page.toml these tests check.
function commands() {
  return manifest
    .split(/^\[\[command\]\]/m)
    .slice(1)
    .map((block) => {
      const body = block.split(/^\[/m)[0];
      const str = (key) => body.match(new RegExp(`^${key} = "((?:[^"\\\\]|\\\\.)*)"`, "m"))?.[1];
      return {
        id: str("id"),
        confirm: str("confirm")?.replace(/\\"/g, '"'),
        hasArgs: /^args = /m.test(body),
        patterns: [...body.matchAll(/pattern = "((?:[^"\\]|\\.)*)"/g)].map((m) => m[1]),
      };
    });
}

function dbusTable() {
  const body = manifest.split(/^\[dbus\]/m)[1].split(/^\[/m)[0];
  const list = body.match(/^methods = \[(.*)\]/m)[1].match(/"([^"]+)"/g).map((s) => s.slice(1, -1));
  const timeouts = Object.fromEntries(
    [...body.match(/^timeout_ms = \{(.*)\}/m)[1].matchAll(/(\w+) = (\d+)/g)].map((m) => [m[1], Number(m[2])]),
  );
  return { methods: list, timeouts };
}

test("parses_state_fixture", () => {
  const s = plain(L.parseState(fixture("state.json")));
  assert.equal(s.paused, false);
  assert.equal(s.managed, 12);
  assert.equal(s.powerProfile, "balanced");
  assert.equal(s.cpuPressure, 34.2);
  assert.equal(s.agents, 1);
  assert.deepEqual(s.roles[3], { role: "background", profile: "background", enabled: false });
  assert.equal(s.roles.length, 4);
  assert.deepEqual(s.boosted.map((b) => b.name), ["firefox", "<b>x</b>"]);
  assert.equal(L.pressureText(s), "CPU busy: 34 %");
  assert.equal(L.managedText(s), "12 apps managed");
  assert.equal(L.boostedLine(s.boosted[0]), "firefox — desktop");
});

test("reads_busctl_wrapper_and_failures", () => {
  const ok = plain(L.readState({ code: 0, output: { type: "s", data: fixture("state.json") } }));
  assert.equal(ok.status, "ready");
  assert.equal(ok.state.managed, 12);
  const down = plain(L.readState({ code: 1, error: "exit 1: Failed to get property State: The name org.scdr.Daemon1 was not provided by any .service files" }));
  assert.equal(down.status, "not-running");
  const odd = plain(L.readState({ code: 1, error: "exit 1: Access denied" }));
  assert.equal(odd.status, "error");
  assert.equal(plain(L.readState({ code: 0, output: { type: "s", data: "{" } })).status, "error");
  assert.equal(plain(L.readState({ code: 0, output: null })).status, "error");
});

test("bad_state_is_an_error", () => {
  for (const bad of ["{", "null", "[]", "42", '"x"', "{}"]) assert.ok(L.parseState(bad).error, bad);
  const roles = (r) => JSON.stringify({ paused: false, roles: r });
  assert.ok(L.parseState(roles([["audio", "realtime"]])).error);
  assert.ok(L.parseState(roles([["audio", "realtime", true, 1]])).error);
  assert.ok(L.parseState(roles([["audio", 1, true]])).error);
  assert.ok(!L.parseState(roles([])).error);
});

test("state_is_capped_and_clipped", () => {
  const boosted = Array.from({ length: 900 }, (_, i) => ({ pid: i, name: "x".repeat(500), profile: "p", why: "" }));
  const s = plain(L.parseState(JSON.stringify({ paused: true, boosted })));
  assert.equal(s.boosted.length, 500);
  assert.equal(s.boosted[0].name.length, 200);
});

test("settings_fixture", () => {
  const s = plain(L.parseSettings(fixture("settings.json")));
  assert.deepEqual(s.layerRules.map((r) => [r.index, r.name, r.profile, r.simple]), [
    [0, "firefox", "desktop", true],
    [1, "steam*", "game", true],
    [2, "x", "background", false],
    [3, "name = a, b", "desktop", false],
  ]);
  assert.deepEqual(s.profiles, ["background", "desktop", "game"]);
  assert.deepEqual(s.schedulers.map((x) => x.value), ["eevdf", "bpfland", "bpfland:gaming", "lavd"]);
  assert.deepEqual(s.schedulers[0], { value: "eevdf", label: "Kernel default" });
  assert.equal(s.schedulers[2].label, "bpfland (gaming)");
  assert.equal(s.choice, "bpfland");
  assert.equal(s.currentScheduler, "bpfland");
  assert.equal(s.loaderError, "");
  assert.ok(L.parseSettings("[]").error);
  assert.ok(L.parseSettings("nope").error);
});

test("settings_choice_cases", () => {
  const doc = (layer, extra = {}) => JSON.stringify({ layer, base: {}, schedulers: [], ...extra });
  assert.equal(L.parseSettings(doc({})).choice, "eevdf");
  assert.equal(L.parseSettings(doc({ sched_ext: { enable: false, default: "bpfland" } })).choice, "eevdf");
  const gone = plain(L.parseSettings(doc({ sched_ext: { enable: true, default: "scx_rusty:auto" } })));
  assert.equal(gone.choice, "rusty");
  assert.equal(gone.schedulers.at(-1).label, "rusty (not installed)");
  assert.equal(L.parseSettings(doc({}, { scx_loader_error: "no loader" })).loaderError, "no loader");
});

test("scheduler_eevdf_disables", () => {
  const layer = { sched_ext: { enable: true, default: "lavd" } };
  assert.equal(L.setScheduler(layer, "eevdf").ok, true);
  assert.deepEqual(plain(layer.sched_ext), { enable: false, default: "eevdf" });
});

test("scheduler_named_enables_and_keeps_power_keys", () => {
  const layer = { sched_ext: { power: { performance: "lavd" }, gaming: "lavd:gaming" } };
  assert.equal(L.setScheduler(layer, "bpfland:gaming").ok, true);
  assert.equal(layer.sched_ext.enable, true);
  assert.equal(layer.sched_ext.default, "bpfland:gaming");
  assert.equal(layer.sched_ext.power.performance, "lavd");
  assert.equal(layer.sched_ext.gaming, "lavd:gaming");
  const empty = {};
  L.setScheduler(empty, "lavd");
  assert.deepEqual(plain(empty), { sched_ext: { enable: true, default: "lavd" } });
});

test("scheduler_rejects_bad_choice", () => {
  for (const bad of ["bpf land", "x:turbo", "", "a/b", "x".repeat(40), "-x", "a\nb", null]) {
    const layer = {};
    const r = L.setScheduler(layer, bad);
    assert.equal(r.code, "BadScheduler", String(bad));
    assert.deepEqual(plain(layer), {});
  }
});

test("add_rule_appends_and_creates", () => {
  const layer = {};
  assert.equal(L.addRule(layer, "desktop", "firefox").index, 0);
  assert.deepEqual(plain(layer.rule), [{ profile: "desktop", name: "firefox" }]);
  assert.equal(L.addRule(layer, "game", "steam*").index, 1);
  assert.equal(layer.rule.length, 2);
});

test("add_rule_refuses", () => {
  const layer = { rule: [{ profile: "desktop", name: ["firefox"] }] };
  assert.equal(L.addRule(layer, "desktop", "").code, "BadName");
  assert.equal(L.addRule(layer, "desktop", "a/b").code, "BadName");
  assert.equal(L.addRule(layer, "desktop", "x".repeat(65)).code, "BadName");
  assert.equal(L.addRule(layer, "desktop", "-rf").code, "BadName");
  assert.equal(L.addRule(layer, "desktop", "firefox").code, "Duplicate");
  assert.equal(L.addRule(layer, "Bad Profile", "x").code, "BadProfile");
  assert.equal(L.addRule({ rule: "x" }, "desktop", "x").code, "Unreadable");
  assert.equal(layer.rule.length, 1);
  assert.equal(L.addRule(layer, "game", "firefox").index, 1);
});

test("remove_rule_checks_name", () => {
  const layer = { rule: [{ profile: "a", name: "one" }, { profile: "b", name: ["two"] }, { profile: "c", name: ["x", "y"] }] };
  assert.equal(L.removeRule(layer, "wrong", 0).code, "Changed");
  assert.equal(layer.rule.length, 3);
  assert.equal(L.removeRule(layer, "two", 2).code, "Changed");
  assert.equal(L.removeRule(layer, "x", 2).code, "Changed");
  assert.equal(L.removeRule(layer, "two", 7).code, "NoSuchRule");
  assert.equal(L.removeRule({}, "two", 0).code, "NoSuchRule");
  assert.equal(L.removeRule(layer, "two", 1).ok, true);
  assert.equal(L.removeRule(layer, "one", 0).ok, true);
  assert.equal(layer.rule.length, 1);
  const last = { rule: [{ profile: "a", name: "one" }], other: 1 };
  assert.equal(L.removeRule(last, "one", 0).ok, true);
  assert.deepEqual(plain(last), { other: 1 });
});

test("messages_are_plain", () => {
  const codes = ["BadScheduler", "BadName", "BadProfile", "Duplicate", "Changed", "NoSuchRule", "Unreadable", "TooBig"];
  const texts = [
    L.setScheduler({}, "x y").error, L.addRule({}, "d", "").error, L.addRule({}, "D", "x").error,
    L.addRule({ rule: [{ profile: "d", name: "x" }] }, "d", "x").error, L.removeRule({ rule: [{ name: "a" }] }, "b", 0).error,
    L.removeRule({}, "b", 0).error, L.addRule({ rule: 1 }, "d", "x").error,
    L.applyEdit(JSON.stringify({ layer: {} }), () => ({ ok: true, index: 0 })).error,
    L.checkRuleName(""), L.checkRuleName("-x"), L.checkRuleName("x".repeat(70)), L.checkProfile("A"),
    L.writeError({ error: "AccessDenied" }), L.writeError({}), L.writeError({ error: "org.freedesktop.DBus.Error.ServiceUnknown" }),
    L.parseState("{").error, L.parseSettings("{").error,
  ];
  assert.ok(codes.length >= 8);
  for (const t of texts.filter((t) => t !== undefined && t !== "")) {
    assert.doesNotMatch(t, /serde|json|d-bus|dbus|polkit|systemctl|scdrd|sched_ext|eevdf|[{}]/i, t);
  }
  assert.equal(L.removeRule({ rule: [{ name: "a" }] }, "b", 0).error, "Your rules changed elsewhere; refresh and try again.");
});

test("apply_edit_round_trip_and_size_limit", () => {
  const text = fixture("settings.json");
  const r = plain(L.applyEdit(text, (layer) => L.addRule(layer, "desktop", "kate")));
  assert.equal(r.index, 4);
  const doc = JSON.parse(r.doc);
  assert.equal(doc.rule.length, 5);
  assert.equal(doc.sched_ext.power.performance, "lavd");
  assert.equal(JSON.parse(text).layer.rule.length, 4);
  assert.equal(plain(L.applyEdit(text, () => L.setScheduler({}, "bad one"))).code, "BadScheduler");
  assert.ok(L.applyEdit("{", () => ({ ok: true })).error);
  // Quotes double when the document is sent as a string argument, so the argument limit bites first.
  const rules = (n) => JSON.stringify({ layer: { rule: Array.from({ length: n }, (_, i) => ({ profile: "desktop", name: "app" + i })) } });
  const argsLimit = plain(L.applyEdit(rules(360), () => ({ ok: true, index: 0 })));
  assert.equal(argsLimit.code, "TooBig");
  assert.ok(JSON.stringify(JSON.parse(rules(360)).layer).length < 15 * 1024);
  assert.ok(L.applyEdit(rules(100), () => ({ ok: true })).doc);
  const tooBig = plain(L.applyEdit(rules(500), () => ({ ok: true, index: 0 })));
  assert.equal(tooBig.code, "TooBig");
  assert.equal(tooBig.error, "Too many settings to change here; use scdr's own settings.");
  const huge = JSON.stringify({ layer: { note: "x".repeat(16 * 1024) } });
  assert.equal(plain(L.applyEdit(huge, () => ({ ok: true }))).code, "TooBig");
  const small = JSON.stringify({ layer: { note: "x".repeat(2000) } });
  assert.ok(L.applyEdit(small, () => ({ ok: true })).doc);
});

test("role_labels", () => {
  assert.equal(L.roleLabel("foreground"), "The app you're using");
  assert.equal(L.roleLabel("audio"), "Apps playing sound");
  assert.equal(L.roleLabel("game"), "Games");
  assert.equal(L.roleLabel("background"), "Apps in the background when the CPU is busy");
});

test("agent_state_cases", () => {
  assert.equal(L.agentEnabled({ code: 0, output: "enabled\n" }), true);
  assert.equal(L.agentEnabled({ code: 1, error: "exit 1", output: "disabled\n" }), false);
  assert.equal(L.agentEnabled({ code: 1, error: "exit 1", output: "masked\n" }), null);
  assert.equal(L.agentEnabled({ code: 0, output: "static\n" }), null);
  assert.equal(L.agentEnabled({ error: "timed out" }), null);
  assert.equal(L.agentEnabled(undefined), null);
});

test("not_running_cases", () => {
  for (const e of [
    "org.freedesktop.DBus.Error.ServiceUnknown: The name is not known",
    "NameHasNoOwner",
    "The name org.scdr.Daemon1 is not activatable",
    "org.scdr.Daemon1 is not running",
  ]) assert.equal(L.notRunning(e), true, e);
  for (const e of ["AccessDenied", "timed out", "", undefined]) assert.equal(L.notRunning(e), false);
});

test("write_error_cases", () => {
  const pw = "You need to enter your password to change this.";
  assert.equal(L.writeError({ error: "org.freedesktop.DBus.Error.AccessDenied: not authorized" }), pw);
  assert.equal(L.writeError({ error: "polkit said no" }), pw);
  assert.equal(L.writeError({ error: "first line\nsecond" }), "first line");
  assert.equal(L.writeError({ error: "y".repeat(500) }).length, 200);
  assert.equal(L.writeError({}), "Something went wrong.");
});

test("patterns_match_manifest", () => {
  // The only commands that take arguments would carry these; today none does, so any pattern
  // in page.toml must still be one of the logic's own.
  const known = [L.RULE_NAME, L.PROFILE, L.CHOICE];
  for (const c of commands()) {
    for (const p of c.patterns) assert.ok(known.includes(p), `${c.id}: ${p}`);
  }
  assert.deepEqual(commands().filter((c) => c.hasArgs), []);
});

test("rejects_hostile_args", () => {
  const hostile = ["-rf", "--force", "a\nb", "x".repeat(5000), "../x", "a/b", "", "x\n", " ", "$(id)", "a;b"];
  for (const h of hostile) {
    assert.notEqual(L.checkRuleName(h), "", JSON.stringify(h).slice(0, 20));
    assert.equal(new RegExp(L.RULE_NAME).test(h), false);
    assert.equal(new RegExp(L.CHOICE).test(h), false);
    assert.equal(new RegExp(L.PROFILE).test(h), false);
  }
  for (const ok of ["firefox", "steam*", "Code - OSS", "my_app.v2", "a@b+c"]) assert.equal(L.checkRuleName(ok), "", ok);
  for (const ok of ["eevdf", "bpfland", "scx_lavd:gaming", "lavd:powersave"]) assert.equal(new RegExp(L.CHOICE).test(ok), true, ok);
  for (const bad of ["x:turbo", "a:b:c", "eevdf:auto2"]) assert.equal(new RegExp(L.CHOICE).test(bad), false, bad);
});

test("confirms_match_plan", () => {
  const confirms = Object.fromEntries(commands().filter((c) => c.confirm).map((c) => [c.id, c.confirm]));
  assert.deepEqual(confirms, {
    "agent-off": "Stop following the focused window? The app you're using no longer gets a boost.",
  });
  // The D-Bus writes that destroy or pause ask inside the page, with these texts.
  assert.deepEqual(plain(L.CONFIRMS), {
    "pause": "Pause scdr? Every app's priority goes back to normal until you resume.",
    "rule-remove": "Remove your rule for %s?",
  });
  assert.equal(L.confirmText("rule-remove", "steam*"), "Remove your rule for steam*?");
  assert.equal(L.confirmText("rule-remove", "$&"), "Remove your rule for $&?");
  assert.equal(L.confirmText("pause"), L.CONFIRMS.pause);
});

test("dbus_writes_allowlisted", () => {
  const { methods, timeouts } = dbusTable();
  const reads = ["Status", "Processes", "Settings"];
  const writes = ["Pause", "Resume", "SetRoleEnabled", "SetSettings"];
  assert.deepEqual([...methods].sort(), [...reads, ...writes].sort());
  assert.deepEqual(Object.keys(timeouts).sort(), [...writes].sort());
  for (const w of writes) assert.equal(timeouts[w], 120000, w);
  // Only the page's own commands run: reads of state, and the follow-focus and start switches.
  assert.deepEqual(commands().map((c) => c.id), ["state", "agent-state", "agent-on", "agent-off", "start"]);
});

test("manifest_text_is_plain", () => {
  const words = /D-Bus|polkit|JSON|systemctl|scdrd|sched_ext|EEVDF|sysctl|org\.scdr/i;
  const shown = manifest.replace(/^\[dbus\][\s\S]*?(?=^\[)/m, ""); // the bus name is not shown to anyone
  const fields = [...shown.matchAll(/^(?:name|label|confirm) = "((?:[^"\\]|\\.)*)"/gm)].map((m) => m[1]);
  assert.ok(fields.length >= 7);
  for (const f of fields) assert.doesNotMatch(f, words, f);
});

const qmlFiles = () => readdirSync(dir).filter((f) => f.endsWith(".qml"));

test("page_strings_are_plain", () => {
  const words = /D-Bus|polkit|JSON|systemctl|scdrd|xzramd|notifd|tailscaled|sched_ext|EEVDF|zram-generator|sysctl|org\.scdr|busctl/i;
  let checked = 0;
  for (const file of qmlFiles()) {
    const lines = readFileSync(new URL(file, dir), "utf8").split("\n");
    for (const line of lines) {
      if (/^\s*\/\//.test(line) || !/\b(text|placeholderText|title|label|detail|message)\s*:|confirm/.test(line)) continue;
      for (const m of line.matchAll(/"((?:[^"\\]|\\.)*)"/g)) {
        checked++;
        assert.doesNotMatch(m[1], words, `${file}: ${m[1]}`);
      }
    }
  }
  assert.ok(checked > 30, `checked ${checked} strings`);
});

test("page_files_stay_small_and_unstyled", () => {
  const files = qmlFiles();
  assert.ok(files.includes("Page.qml"));
  for (const f of files) {
    const text = readFileSync(new URL(f, dir), "utf8");
    assert.ok(text.split("\n").length <= 400, `${f} is over 400 lines`);
    assert.doesNotMatch(text, /import org\.kde|Kirigami/, f);
  }
});
