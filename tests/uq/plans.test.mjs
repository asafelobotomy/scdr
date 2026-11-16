// Node tests of plans.js: what the page makes of `scdrctl config show`, the previews, `check`,
// `list`, the benchmark's lines, and the daemon's Status and Explain. Run: node --test 'tests/uq/*.test.mjs'
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { loadLogic, loadScript, pageDir } from "./load-logic.mjs";

const L = loadLogic();
const P = loadScript("plans.js", L);
const plain = (v) => JSON.parse(JSON.stringify(v));

const row = (over) => ({ key: "system.rescan_secs", type: "uint", group: "System", label: "Full rescan every", help: "", value: 60, source: "files", default: 60, min: 0, max: 86400, unit: "s", ...over });
const ok = (output) => ({ code: 0, output });

test("config_view_reads_rows_and_drops_odd_ones", () => {
  const v = plain(P.configView(ok({ ok: true, layer_file: "/var/lib/scdr/settings.toml", layer: { rules: 2, profiles: 1, empty: false },
    settings: [row(), row({ key: "evil key", type: "uint" }), row({ key: "a.b", type: "weird" }), row({ key: "split.apps", type: "list", value: ["a", 3, "b"] }), "x"] })));
  assert.equal(v.ready, true);
  assert.deepEqual(v.rows.map((r) => r.key), ["system.rescan_secs", "split.apps"]);
  assert.equal(v.rows[0].source, "files");
  assert.equal(v.rows[0].to, 86400);
  assert.deepEqual(v.rows[1].value, ["a", "3", "b"]);
  assert.deepEqual(v.layer, { rules: 2, profiles: 1, empty: false });
  assert.equal(v.file, "/var/lib/scdr/settings.toml");
});

test("config_view_failures_give_a_reason", () => {
  assert.equal(P.configView(null).ready, false);
  const refused = plain(P.configView({ code: 1, error: "exit 1", output: { ok: false, error: "/etc/scdr/scdr.toml: bad key" } }));
  assert.equal(refused.ready, false);
  assert.equal(refused.error, "/etc/scdr/scdr.toml: bad key");
  assert.match(P.configView({ error: "org.freedesktop.DBus.Error.AccessDenied: x" }).error, /password/);
  assert.equal(P.configView(ok({ ok: true })).ready, false);
});

test("set_arguments", () => {
  assert.equal(P.setArg({ type: "list" }, "a, b"), "a, b");
  assert.equal(P.setArg({ type: "list" }, ["a", "b"]), "a, b");
  assert.equal(P.setArg({ type: "list" }, "  "), "[]");
  assert.equal(P.setArg({ type: "bool" }, true), "on");
  assert.equal(P.setArg({ type: "uint" }, 90), "90");
  assert.equal(P.setProblem("system.iocost", "on"), "");
  assert.notEqual(P.setProblem("system.iocost", "-x"), "");
  assert.notEqual(P.setProblem("system.iocost", ""), "");
  assert.notEqual(P.setProblem("system.iocost", "a\nb"), "");
  assert.notEqual(P.setProblem("Bad Key", "on"), "");
  assert.notEqual(P.setProblem("system.x", "x".repeat(401)), "");
});

test("value_and_source_texts", () => {
  const r = plain(P.configView(ok({ ok: true, settings: [row(), row({ key: "system.autogroup", type: "bool", value: null, source: "default", default: null, nullable: true, unit: "" }),
    row({ key: "pressure.high", type: "float", value: 40, source: "you", unit: "%" }), row({ key: "split.apps", type: "list", value: [], source: "default" })] })));
  assert.equal(P.valueText(r.rows[0]), "60 s");
  assert.equal(P.valueText(r.rows[1]), "not set");
  assert.equal(P.valueText(r.rows[2]), "40 %");
  assert.equal(P.valueText(r.rows[3]), "none");
  assert.deepEqual(r.rows.map(P.sourceText), ["From scdr's files", "Default", "Set here", "Default"]);
});

const plan = { summary: "3 changes", changes: [
  { group: "Caches", label: "a", detail: "x", reversible: true },
  { label: "b" },
  { group: "Caches", label: "c", reversible: false },
  { label: 5 },
], would_refuse: false, reason: "" };

test("plans_are_grouped_in_order_of_first_appearance", () => {
  const p = plain(P.planOf(ok(plan)));
  assert.equal(p.summary, "3 changes");
  assert.equal(p.problem, "");
  const groups = plain(P.planGroups(p));
  assert.deepEqual(groups.map((g) => [g.title, g.items.map((i) => i.label)]), [["Caches", ["a", "c"]], ["Changes", ["b"]]]);
  assert.equal(P.changeText(p.changes[0]), "a: x (can be undone)");
  assert.equal(P.changeText(p.changes[2]), "c (can't be undone)");
});

test("plans_that_refuse_or_fail", () => {
  const refused = plain(P.planOf(ok({ summary: "x", changes: [], would_refuse: true, reason: "already exists" })));
  assert.equal(refused.refuses, true);
  assert.equal(refused.reason, "already exists");
  assert.match(P.planOf({ error: "exit 1" }).problem, /./);
  assert.match(P.planOf(ok("junk")).problem, /couldn't say/);
  const big = { summary: "x", changes: Array.from({ length: 600 }, (_, i) => ({ label: "l" + i })) };
  const b = plain(P.planOf(ok(big)));
  assert.equal(b.changes.length, 500);
  assert.equal(b.more, 100);
});

test("the_pages_own_plans", () => {
  const rule = P.removeRulePlan({ name: "steam*", profile: "game" });
  assert.equal(rule.changes[0].detail, "no longer sent to the game profile");
  const settings = plain(L.parseSettings(JSON.stringify({ layer: { rule: [{ profile: "mine", name: "a" }], roles: { game: "mine" }, profile: { mine: {} } }, base: {} })));
  const p = plain(P.removeProfilePlan(settings, "mine", false));
  assert.deepEqual(p.changes.map((c) => c.group), ["Removed", "Still using it", "Still using it"]);
  assert.equal(P.removeProfilePlan(settings, "mine", true).changes[0].group, "Back to scdr's files");
});

test("check_and_list", () => {
  assert.deepEqual(plain(P.checkView(ok({ ok: true, profiles: 13, rules: 11, files: ["/a"], warnings: ["w"] }))), { ok: true, text: "13 profiles and 11 rules load.", warnings: ["w"], files: ["/a"] });
  const bad = plain(P.checkView({ code: 1, error: "exit 1", output: { ok: false, error: "x.toml: unknown field" } }));
  assert.equal(bad.ok, false);
  assert.equal(bad.text, "x.toml: unknown field");
  const list = ok({ ok: true, processes: [{ pid: 1, name: "firefox", profile: "desktop", rule: "r #1" }, { pid: 2, name: "code", profile: "dev", rule: "" }, { name: 5 }] });
  assert.deepEqual(plain(P.processesView(list, "", 10).rows.map((r) => r.name)), ["firefox", "code"]);
  assert.deepEqual(plain(P.processesView(list, "DEV", 10).rows.map((r) => r.name)), ["code"]);
  const cut = plain(P.processesView(list, "", 1));
  assert.equal(cut.rows.length, 1);
  assert.equal(cut.total, 2);
  assert.equal(P.processesView(ok({ ok: false }), "", 1).ok, false);
});

test("import_paths_and_the_command_to_run", () => {
  assert.equal(P.importProblem("/etc/ananicy.d"), "");
  assert.notEqual(P.importProblem("etc/ananicy.d"), "");
  assert.notEqual(P.importProblem(""), "");
  assert.notEqual(P.importProblem("/a\nb"), "");
  assert.equal(P.importCommand("ananicy", "/etc/ananicy.d", ""), "sudo scdrctl import ananicy /etc/ananicy.d --output /etc/scdr/rules.d/50-ananicy.toml");
  assert.equal(P.importCommand("system76", "/x.kdl", ""), "sudo scdrctl import system76 /x.kdl --output /etc/scdr/rules.d/50-system76.toml");
});

test("benchmark_progress", () => {
  let s = P.startBench();
  s = P.benchReduce(s, { event: "scenario", name: "baseline", round: 1, rounds: 3, percent: 12 });
  assert.deepEqual(plain(s), { percent: 12, line: "baseline (round 1 of 3)", done: false, results: [] });
  s = P.benchReduce(s, { event: "scenario", name: "slice", round: 1, rounds: 1, percent: 500 });
  assert.equal(s.percent, 100);
  assert.equal(P.benchReduce(s, "junk"), s);
  assert.equal(P.benchReduce(s, { event: "other" }), s);
  const d = plain(P.benchReduce(s, { event: "done", results: [{ scenario: { name: "baseline" }, p50_us: 80.4, p99_us: 900, max_us: null }, 4] }));
  assert.equal(d.done, true);
  assert.deepEqual(d.results, [{ name: "baseline", p50: "80 µs", p99: "900 µs", max: "-" }]);
  assert.deepEqual(plain(P.benchCheck(ok({ ok: true, can_run: false, reason: "scdrd is running", seconds: 40 }))), { can: false, reason: "scdrd is running", seconds: 40 });
  assert.equal(P.benchCheck({ error: "x" }).can, false);
});

test("status_facts_and_explain", () => {
  const status = { value: JSON.stringify({ version: "0.4.2", dry_run: true, uptime_secs: 7200, power_profile: "balanced", tracked: 300, managed: 12, excepted: 4,
    governors: ["powersave"], cpufreq_driver: ["amd-pstate-epp", "active"], memory_pressure: 1.4, disks: [{ name: "nvme0n1", scheduler: "none", iocost: true }], notes: [["Rival", "ananicy-cpp runs"], 7] }) };
  const facts = plain(P.statusFacts(status));
  const by = Object.fromEntries(facts.map((f) => [f.label, f.text]));
  assert.equal(by["scdr"], "0.4.2 (dry run)");
  assert.equal(by["Running for"], "2 h");
  assert.equal(by["Processes"], "300 seen, 12 managed, 4 left alone");
  assert.equal(by["CPU frequency driver"], "amd-pstate-epp (active)");
  assert.equal(by["Disk nvme0n1"], "none, iocost on");
  assert.equal(by["Rival"], "ananicy-cpp runs");
  assert.deepEqual(plain(P.statusFacts({ error: "x" })), []);
  assert.equal(P.uptimeText(30), "30 s");
  assert.equal(P.uptimeText(600), "10 min");
  assert.equal(P.uptimeText(400000), "5 days");

  const ex = { value: JSON.stringify({ pid: 5, name: "firefox", state: "managed", reason: null, profile: "desktop", profile_chain: ["desktop", "base"], rule: { source: "/usr/share/scdr/scdr.toml", index: 3, conditions: "name = firefox" }, notes: ["n"], errors: ["e"] }) };
  const e = plain(P.explainView(ex));
  assert.equal(e.ok, true);
  assert.deepEqual(e.rows.map((r) => r.label), ["Name", "State", "Profile", "Inherits from", "Rule"]);
  assert.equal(e.rows[4].text, "scdr.toml #3 (name = firefox)");
  assert.deepEqual(e.notes, ["n", "e"]);
  assert.equal(P.explainView({ value: JSON.stringify({ pid: 1, name: null }) }).problem, "No process has that number.");
  assert.match(P.explainView({ error: "org.freedesktop.DBus.Error.ServiceUnknown: x" }).problem, /isn't running/);
});

test("every_setting_of_the_manifest_has_a_row", () => {
  const manifest = readFileSync(`${pageDir}page.toml`, "utf8");
  const ids = [...manifest.matchAll(/^\[\[setting\]\]\nid = "([^"]+)"/gm)].map((m) => m[1]);
  assert.ok(ids.length >= 20);
  const qml = readdirSync(pageDir).filter((f) => f.endsWith(".qml")).map((f) => readFileSync(`${pageDir}${f}`, "utf8")).join("\n");
  for (const id of ids) {
    const used = qml.includes(`settingId: "${id}"`) || (id === "system" || id === "split" || id === "exceptions") && qml.includes(`id: "${id}"`);
    assert.ok(used, `no row has the settingId ${id}`);
  }
  // And every row's id is in the manifest, or search could not jump to it.
  for (const m of qml.matchAll(/settingId: "([^"]+)"/g)) assert.ok(ids.includes(m[1]), `${m[1]} is not in page.toml`);
});
