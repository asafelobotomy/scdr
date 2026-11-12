import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { loadLogic, loadScript } from "./load-logic.mjs";

const L = loadLogic();
const W = loadScript("words.js", L);
const S = loadScript("schedulers.js", L);
const R = loadScript("rules.js", L);
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
  assert.equal(W.pressureText(s), "CPU busy: 34 %");
  assert.equal(W.managedText(s), "12 apps managed");
  assert.equal(W.boostedLine(s.boosted[0]), "firefox — desktop");
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
  assert.deepEqual(s.layerRules[2].conditions, [{ field: "name", text: "x" }, { field: "exe", text: "/opt/x/*" }]);
  assert.deepEqual(s.layerRules[3].conditions, [{ field: "name", text: "a, b" }]);
  assert.equal(s.layerRules[1].key, JSON.stringify({ profile: "game", name: ["steam*"] }));
  assert.deepEqual(s.profiles, ["background", "desktop", "game"]);
  assert.deepEqual(s.installed.map((x) => x.value), ["bpfland", "bpfland:gaming", "lavd", "devbox"]);
  assert.equal(s.installed[1].label, "bpfland (gaming)");
  assert.deepEqual(s.cards.map((c) => [c.name, c.installed, c.modes.length]),
    [["bpfland", true, 2], ["lavd", true, 1], ["rusty", false, 1], ["devbox", true, 1]]);
  assert.equal(s.cards[2].description, "Multi-domain");
  assert.equal(s.schedExt, true);
  assert.deepEqual(s.powerProfiles, ["power-saver", "balanced", "performance"]);
  assert.equal(s.trial, null);
  assert.equal(s.currentScheduler, "bpfland");
  assert.equal(s.loaderError, "");
  assert.ok(L.parseSettings("[]").error);
  assert.ok(L.parseSettings("nope").error);
});

test("settings_scheduler_cases", () => {
  const doc = (layer, extra = {}) => JSON.stringify({ layer, base: {}, schedulers: [], ...extra });
  assert.equal(L.parseSettings(doc({})).schedExt, false);
  assert.equal(L.parseSettings(JSON.stringify({ layer: {}, base: { sched_ext: { enable: true } } })).schedExt, true);
  assert.equal(L.parseSettings(JSON.stringify({ layer: { sched_ext: { enable: false } }, base: { sched_ext: { enable: true } } })).schedExt, false);
  assert.equal(L.parseSettings(doc({}, { scx_loader_error: "no loader" })).loaderError, "no loader");
  // Odd entries in the scheduler list are skipped; a list without modes means "auto".
  const odd = plain(L.parseSettings(doc({}, { schedulers: [{ name: "a b", installed: true }, { name: "x", installed: true }, 7] })));
  assert.deepEqual(odd.installed, [{ value: "x", label: "x" }]);
  assert.deepEqual(odd.cards[0].modes, ["auto"]);
  // Without a power profile list, the usual three.
  assert.deepEqual(plain(L.parseSettings(doc({}, { power_profiles: undefined })).powerProfiles), ["power-saver", "balanced", "performance"]);
});

test("trial_parsing", () => {
  const doc = (trial) => JSON.stringify({ layer: {}, base: {}, trial });
  assert.equal(L.parseSettings(doc(null)).trial, null);
  assert.equal(L.parseSettings(doc({ was: "eevdf" })).trial, null);
  assert.equal(L.parseSettings(doc("x")).trial, null);
  const t = plain(L.parseSettings(doc({ trying: "scx_devbox:auto", was: "eevdf" })).trial);
  assert.deepEqual(t, { trying: "devbox", was: "Kernel default" });
  assert.equal(W.trialText(t), "Trying devbox (was Kernel default). It isn't saved; stop the trial to go back.");
  const m = plain(L.parseSettings(doc({ trying: "scx_bpfland:gaming", was: "scx_lavd:auto" })).trial);
  assert.deepEqual(m, { trying: "bpfland (gaming)", was: "lavd" });
  assert.equal(W.trialText(null), "");
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

test("remove_rule_checks_key", () => {
  const rules = [{ profile: "a", name: "one" }, { profile: "b", name: ["two"] }, { profile: "c", name: ["x", "y"] }];
  const layer = { rule: plain(rules) };
  const key = (i) => JSON.stringify(rules[i]);
  assert.equal(R.removeRule(layer, 0, key(1)).code, "Changed");
  assert.equal(layer.rule.length, 3);
  assert.equal(R.removeRule(layer, 7, key(1)).code, "NoSuchRule");
  assert.equal(R.removeRule({}, 0, key(0)).code, "NoSuchRule");
  assert.equal(R.removeRule(layer, 2, key(2)).ok, true);
  assert.equal(R.removeRule(layer, 0, key(0)).ok, true);
  assert.deepEqual(plain(layer.rule), [rules[1]]);
  const last = { rule: [{ profile: "a", name: "one" }], other: 1 };
  assert.equal(R.removeRule(last, 0, JSON.stringify(last.rule[0])).ok, true);
  assert.deepEqual(plain(last), { other: 1 });
});

test("messages_are_plain", () => {
  const codes = ["BadScheduler", "BadName", "BadProfile", "Duplicate", "Changed", "NoSuchRule", "Unreadable", "TooBig"];
  const texts = [
    S.setSlot({}, "default", "x y").error, L.addRule({}, "d", "").error, L.addRule({}, "D", "x").error,
    L.addRule({ rule: [{ profile: "d", name: "x" }] }, "d", "x").error, R.removeRule({ rule: [{ name: "a" }] }, 0, "{}").error,
    R.removeRule({}, 0, "{}").error, L.addRule({ rule: 1 }, "d", "x").error,
    L.applyEdit(JSON.stringify({ layer: {} }), () => ({ ok: true, index: 0 })).error,
    L.checkRuleName(""), L.checkRuleName("-x"), L.checkRuleName("x".repeat(70)), L.checkProfile("A"),
    L.writeError({ error: "AccessDenied" }), L.writeError({}), L.writeError({ error: "org.freedesktop.DBus.Error.ServiceUnknown" }),
    L.parseState("{").error, L.parseSettings("{").error,
  ];
  assert.ok(codes.length >= 8);
  for (const t of texts.filter((t) => t !== undefined && t !== "")) {
    assert.doesNotMatch(t, /serde|json|d-bus|dbus|polkit|systemctl|scdrd|sched_ext|eevdf|[{}]/i, t);
  }
  assert.equal(R.removeRule({ rule: [{ name: "a" }] }, 0, "{}").error, "Your rules changed elsewhere; refresh and try again.");
});

test("apply_edit_round_trip_and_size_limit", () => {
  const text = fixture("settings.json");
  const r = plain(L.applyEdit(text, (layer) => L.addRule(layer, "desktop", "kate")));
  assert.equal(r.index, 4);
  const doc = JSON.parse(r.doc);
  assert.equal(doc.rule.length, 5);
  assert.equal(doc.sched_ext.power.performance, "lavd");
  assert.equal(JSON.parse(text).layer.rule.length, 4);
  assert.equal(plain(L.applyEdit(text, (layer) => S.setSlot(layer, "default", "bad one"))).code, "BadScheduler");
  // The edit also gets the whole answer, for the files below the layer.
  let seen = null;
  L.applyEdit(text, (layer, doc) => { seen = doc; return { ok: true }; });
  assert.equal(seen.base.profile.desktop !== undefined, true);
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
  assert.equal(W.roleLabel("foreground"), "The app you're using");
  assert.equal(W.roleLabel("audio"), "Apps playing sound");
  assert.equal(W.roleLabel("game"), "Games");
  assert.equal(W.roleLabel("background"), "Apps in the background when the CPU is busy");
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
    "installer-install": "Install the extra CPU schedulers (scx-scheds and scx-tools)? Your package manager asks for your password.",
    "installer-update": "Update the whole system, the schedulers included? Arch doesn't update one package alone.",
    "installer-git": "Build the newest schedulers from the AUR (scx-scheds-git)? This can take several minutes.",
  });
  // The D-Bus writes that destroy or pause ask inside the page, with these texts.
  assert.deepEqual(plain(W.CONFIRMS), {
    "pause": "Pause scdr? Every app's priority goes back to normal until you resume.",
    "rule-remove": "Remove your rule for %s?",
    "profile-remove": "Delete the profile %s?",
    "profile-revert": "Undo your changes to the profile %s?",
  });
  assert.equal(W.confirmText("rule-remove", "steam*"), "Remove your rule for steam*?");
  assert.equal(W.confirmText("rule-remove", "$&"), "Remove your rule for $&?");
  assert.equal(W.confirmText("pause"), W.CONFIRMS.pause);
});

test("dbus_writes_allowlisted", () => {
  const { methods, timeouts } = dbusTable();
  const reads = ["Status", "Processes", "Settings"];
  const writes = ["Pause", "Resume", "SetRoleEnabled", "SetSettings", "TryScheduler", "EndSchedulerTrial"];
  assert.deepEqual([...methods].sort(), [...reads, ...writes].sort());
  assert.deepEqual(Object.keys(timeouts).sort(), [...writes].sort());
  for (const w of writes) assert.equal(timeouts[w], 120000, w);
  // Only the page's own commands run: reads of state, the follow-focus and start switches, and
  // the scheduler installer.
  assert.deepEqual(commands().map((c) => c.id), ["state", "agent-state", "agent-on", "agent-off", "start",
    "installer-which", "installer-install", "installer-update", "installer-git"]);
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

test("rejects_hostile_values", () => {
  for (const bad of ["-rf", "a\nb", "a/b", "x".repeat(65), ""]) assert.equal(L.addRule({}, "desktop", bad).code, "BadName");
  for (const bad of ["bpf land", "x:turbo", "eevdf:gaming", "a\nb"]) assert.equal(S.setSlot({}, "default", bad).code, "BadScheduler", bad);
});

test("long_rule_names_can_be_removed", () => {
  const name = "n".repeat(150);
  const text = JSON.stringify({ layer: { rule: [{ profile: "desktop", name }] } });
  const rule = plain(L.parseSettings(text)).layerRules[0];
  assert.equal(rule.name.length, 120);
  assert.equal(rule.rawName, name);
  assert.equal(R.removeRule(JSON.parse(text).layer, 0, rule.key).ok, true);
});
