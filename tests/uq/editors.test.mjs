// Node tests of the page's editing logic: the path setter, scheduler choices (devbox included),
// roles, pressure, profiles and rules. Run: node --test 'tests/uq/*.test.mjs'
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { loadLogic, loadScript, pageDir } from "./load-logic.mjs";

const L = loadLogic();
const S = loadScript("schedulers.js", L);
const P = loadScript("profiles.js", L);
const R = loadScript("rules.js", L);
const W = loadScript("words.js", L);
const plain = (v) => JSON.parse(JSON.stringify(v));
const fixture = readFileSync(new URL("./fixtures/settings.json", import.meta.url), "utf8");
const settings = () => L.parseSettings(fixture);
const raw = () => JSON.parse(fixture);
// Runs `edit(layer, doc)` on a copy of the fixture's layer, as the page does; returns the layer sent.
function edited(edit, text = fixture) {
  const r = plain(L.applyEdit(text, edit));
  return r.error ? r : JSON.parse(r.doc);
}

test("set_path_sets_and_prunes", () => {
  const layer = { a: { b: { c: 1 } }, keep: 1 };
  assert.equal(L.setPath(layer, ["a", "b", "d"], 2).ok, true);
  assert.deepEqual(plain(layer.a.b), { c: 1, d: 2 });
  L.setPath(layer, ["a", "b", "c"], undefined);
  L.setPath(layer, ["a", "b", "d"], null);
  assert.deepEqual(plain(layer), { keep: 1 });
  // Missing or odd tables on the way: created when setting, left alone when removing.
  const odd = { x: [1], y: 5 };
  assert.equal(L.setPath(odd, ["x", "z"], undefined).ok, true);
  assert.deepEqual(plain(odd), { x: [1], y: 5 });
  L.setPath(odd, ["y", "z"], false);
  assert.deepEqual(plain(odd.y), { z: false });
  // A table with something else in it stays.
  const both = { t: { u: { v: 1 }, w: 2 } };
  L.setPath(both, ["t", "u", "v"], undefined);
  assert.deepEqual(plain(both), { t: { w: 2 } });
  for (const bad of [[], "a", ["__proto__", "x"], ["a b"], ["a", 1], ["x".repeat(65)], Array(9).fill("a")]) {
    assert.equal(L.setPath({}, bad, 1).code, "Unreadable", JSON.stringify(bad));
  }
  assert.equal(L.lookup({ a: { b: 0 } }, ["a", "b"]), 0);
  assert.equal(L.lookup({ a: 1 }, ["a", "b"]), undefined);
  assert.equal(L.lookup({}, ["constructor"]), undefined);
});

test("scheduler_slot_choices_include_devbox", () => {
  const s = settings();
  const def = plain(S.slotChoices(s, "default"));
  assert.deepEqual(def.map((c) => c.value), [null, "eevdf", "bpfland", "bpfland:gaming", "lavd", "devbox"]);
  assert.deepEqual(def.map((c) => c.text), ["Not set (Kernel default)", "Kernel default", "bpfland", "bpfland (gaming)", "lavd", "devbox"]);
  assert.equal(S.slotCurrent(s, "default"), "bpfland");
  assert.equal(S.slotChoices(s, "gaming")[0].text, "Not set (lavd (gaming))");
  assert.equal(S.slotChoices(s, "balanced")[0].text, "Same as default");
  assert.equal(S.slotCurrent(s, "performance"), "lavd");
  assert.equal(S.slotCurrent(s, "gaming"), null);
  // A choice in the layer that isn't installed any more stays, marked.
  const gone = L.parseSettings(JSON.stringify({ ...raw(), layer: { sched_ext: { gaming: "scx_rusty:auto" } } }));
  assert.equal(S.slotCurrent(gone, "gaming"), "rusty");
  assert.deepEqual(plain(S.slotChoices(gone, "gaming").at(-1)), { text: "rusty (not installed)", value: "rusty" });
  assert.deepEqual(plain(S.slotChoices(s, "bad slot")), []);
});

test("scheduler_slots_set_and_enable", () => {
  let layer = edited((l) => S.setSlot(l, "gaming", "devbox"));
  assert.deepEqual(layer.sched_ext, { enable: true, default: "bpfland", power: { performance: "lavd" }, gaming: "devbox" });
  layer = edited((l) => S.setSlot(l, "performance", null));
  assert.deepEqual(layer.sched_ext, { enable: true, default: "bpfland" });
  const off = JSON.stringify({ layer: { sched_ext: { enable: false } } });
  assert.deepEqual(edited((l) => S.setSlot(l, "default", "eevdf"), off).sched_ext, { enable: false, default: "eevdf" });
  assert.deepEqual(edited((l) => S.setSlot(l, "power-saver", "lavd:powersave"), off).sched_ext,
    { enable: true, power: { "power-saver": "lavd:powersave" } });
  assert.deepEqual(edited((l) => S.setEnabled(l, false), off).sched_ext, { enable: false });
  assert.equal(S.setSlot({}, "enable", "lavd").code, "BadSlot");
  for (const bad of ["x:turbo", "a b", "eevdf:auto", 7]) assert.equal(S.setSlot({}, "default", bad).code, "BadScheduler");
});

test("kernel_knobs", () => {
  const s = settings();
  assert.deepEqual(plain(S.preemptChoices(s, null)).map((c) => c.text), ["Not set (full)", "none", "voluntary", "full", "lazy"]);
  assert.equal(S.preemptChoices(s, "balanced")[0].text, "Same as default");
  assert.deepEqual(edited((l) => S.setPreempt(l, "balanced", "lazy")).eevdf, { power: { balanced: { preempt: "lazy" } } });
  assert.deepEqual(edited((l) => S.setSlice(l, null, 4000)).eevdf, { base_slice_us: 4000 });
  assert.equal(S.setPreempt({}, null, "always").code, "BadPreempt");
  for (const bad of [99, 100001, 1.5, "3000"]) assert.equal(S.setSlice({}, null, bad).code, "BadSlice", String(bad));
  const layer = { eevdf: { base_slice_us: 3000 } };
  assert.equal(S.setSlice(layer, null, undefined).ok, true);
  assert.deepEqual(plain(layer), {});
});

test("try_args", () => {
  assert.equal(S.tryArg("devbox", "auto"), "devbox:auto");
  assert.equal(S.tryArg("bpfland", "gaming"), "bpfland:gaming");
  assert.equal(S.tryArg("lavd", ""), "lavd:auto");
  for (const [n, m] of [["a b", "auto"], ["-x", "auto"], ["lavd", "turbo"], ["eevdf", "auto"], ["x".repeat(40), "auto"]]) {
    assert.equal(S.tryArg(n, m), "", `${n}:${m}`);
  }
});

test("installer_and_loader_words", () => {
  assert.equal(W.installerOf({ code: 0, output: "pamac\n" }), "pamac");
  assert.equal(W.installerOf({ code: 0, output: "something\n" }), "");
  assert.equal(W.installerOf({ code: 1, error: "exit 1: Error: no package installer found" }), "");
  assert.equal(W.installResult({ code: 0, output: null }), "Done.");
  assert.equal(W.installResult({ error: "Cancelled", cancelled: true }), "");
  assert.equal(W.installResult({ code: 1, error: "exit 1: Error: no AUR helper\nmore" }), "exit 1: Error: no AUR helper");
  const s = settings();
  assert.equal(W.loaderText(s), "");
  const only = L.parseSettings(JSON.stringify({ ...raw(), scx_loader_error: "no loader",
    schedulers: [{ name: "devbox", modes: ["auto"], installed: true }] }));
  assert.match(W.loaderText(only), /^Only the schedulers scdr runs itself/);
  const none = L.parseSettings(JSON.stringify({ ...raw(), scx_loader_error: "no loader", schedulers: [] }));
  assert.equal(W.loaderText(none), "No extra schedulers are installed.");
});

test("roles_grid", () => {
  const s = settings();
  assert.deepEqual(plain(P.roleChoices(s, "foreground", null)).slice(0, 2),
    [{ text: "Not set (desktop)", value: null }, { text: "Off", value: "" }]);
  assert.equal(P.roleChoices(s, "audio", null)[0].text, "Not set (off)");
  assert.equal(P.roleChoices(s, "foreground", "power-saver")[0].text, "Not set (off)");
  assert.equal(P.roleChoices(s, "foreground", "balanced")[0].text, "Same as default");
  assert.deepEqual(edited((l, d) => P.setRole(l, d, "game", "performance", "background")).roles, { power: { performance: { game: "background" } } });
  assert.deepEqual(edited((l, d) => P.setRole(l, d, "audio", null, "")).roles, { audio: "" });
  assert.equal(P.setRole({}, raw(), "audio", null, "nope").code, "BadProfile");
  assert.equal(P.setRole({}, raw(), "nap", null, "").code, "BadRole");
});

test("pressure_and_switches", () => {
  assert.deepEqual(edited((l, d) => P.setPressure(l, d, "high", 30.5)).pressure, { high: 30.5 });
  assert.deepEqual(edited((l, d) => P.setPressure(l, d, "hold_secs", 60)).pressure, { hold_secs: 60 });
  assert.equal(edited((l, d) => P.setPressure(l, d, "low", 20)).code, "PressureOrder");
  assert.equal(edited((l, d) => P.setPressure(l, d, "hold_secs", 1.5)).code, "BadNumber");
  assert.equal(edited((l, d) => P.setPressure(l, d, "high", 101)).code, "BadNumber");
  const set = JSON.stringify({ layer: { pressure: { high: 50 } }, base: {} });
  assert.deepEqual(edited((l, d) => P.setPressure(l, d, "high", undefined), set), {});
  const s = settings();
  assert.equal(P.switchChoices(s, "iocost")[0].text, "Not set (on)");
  assert.equal(P.switchChoices(s, "autogroup")[0].text, "Not set");
  assert.deepEqual(edited((l) => P.setSwitch(l, "autogroup", false)).system, { autogroup: false });
  assert.equal(P.setSwitch({}, "swap", true).code, "BadChoice");
});

test("profile_edits", () => {
  const s = settings();
  assert.deepEqual(plain(P.profileView(s, "game")), {
    name: "game", fields: { inherit: "desktop", nice: -5 }, edited: false, shipped: true, origin: "From /usr/share/scdr/scdr.toml",
  });
  // Editing a shipped profile copies it whole, without its _source.
  assert.deepEqual(edited((l, d) => P.setField(l, d, "game", "io", "rt:1")).profile, { game: { inherit: "desktop", nice: -5, io: "rt:1" } });
  assert.deepEqual(edited((l, d) => P.setField(l, d, "game", "nice", undefined)).profile, { game: { inherit: "desktop" } });
  assert.equal(edited((l, d) => P.setField(l, d, "background", "nice", undefined)).code, "EmptyProfile");
  assert.equal(edited((l, d) => P.setField(l, d, "game", "nice", 20)).code, "BadNumber");
  assert.equal(edited((l, d) => P.setField(l, d, "game", "sched", "fifo:100")).code, "BadChoice");
  assert.equal(edited((l, d) => P.setField(l, d, "game", "inherit", "game")).code, "BadChoice");
  assert.equal(edited((l, d) => P.setField(l, d, "game", "color", 1)).code, "BadChoice");
  assert.equal(edited((l, d) => P.setField(l, d, "nope", "nice", 1)).code, "NoSuchProfile");
  assert.deepEqual(plain(P.policyChoices("fifo:42")).at(-1), { text: "fifo:42", value: "fifo:42" });
  assert.equal(P.ioChoices(undefined).length, 18);
  assert.deepEqual(plain(P.inheritChoices(s, "game")).map((c) => c.value), [null, "background", "desktop"]);
});

test("profile_add_and_remove", () => {
  assert.deepEqual(edited((l, d) => P.addProfile(l, d, "quiet", "background")).profile, { quiet: { inherit: "background" } });
  assert.deepEqual(edited((l, d) => P.addProfile(l, d, "quiet", "")).profile, { quiet: { nice: 0 } });
  assert.equal(edited((l, d) => P.addProfile(l, d, "game", "")).code, "ProfileExists");
  assert.equal(edited((l, d) => P.addProfile(l, d, "Bad Name", "")).code, "ProfileName");
  assert.equal(P.checkNewName(settings(), "game"), "There already is a profile with that name.");
  const doc = raw();
  doc.layer.profile = { mine: { nice: 3 }, game: { nice: 1 } };
  const text = JSON.stringify(doc);
  assert.equal(L.parseSettings(text).profiles.includes("mine"), true);
  assert.deepEqual(edited((l, d) => P.removeProfile(l, d, "mine"), text).profile, { game: { nice: 1 } });
  // Undo for a shipped profile drops only the copy.
  assert.deepEqual(edited((l, d) => P.removeProfile(l, d, "game"), text).profile, { mine: { nice: 3 } });
  assert.equal(edited((l, d) => P.removeProfile(l, d, "desktop"), text).code, "NoSuchProfile");
  for (const use of [{ rule: [{ profile: "mine", name: "x" }] }, { roles: { power: { balanced: { audio: "mine" } } } },
    { profile: { other: { inherit: "mine" } } }]) {
    const d = raw();
    d.layer = { ...use, profile: { ...(use.profile || {}), mine: { nice: 3 } } };
    assert.equal(edited((l, dd) => P.removeProfile(l, dd, "mine"), JSON.stringify(d)).code, "ProfileInUse", JSON.stringify(use));
  }
});

test("rule_values", () => {
  assert.deepEqual(plain(R.parseValue("name", " firefox ")), { value: "firefox" });
  assert.deepEqual(plain(R.parseValue("exe", "/a/*, /b ,")), { value: ["/a/*", "/b"] });
  assert.deepEqual(plain(R.parseValue("uid", "1000, 0")), { value: [1000, 0] });
  assert.equal(R.parseValue("uid", "me").code, "BadUid");
  assert.equal(R.parseValue("uid", "99999999999").code, "BadUid");
  assert.equal(R.parseValue("name", " , ").code, "NoValue");
  assert.equal(R.parseValue("name", "a\tb").code, "BadValue");
  assert.equal(R.parseValue("name", "x".repeat(257)).code, "BadValue");
  assert.equal(R.parseValue("name", Array(33).fill("a").join(",")).code, "TooMany");
});

test("rule_editing", () => {
  const s = settings();
  const k = (i) => s.layerRules.find((r) => r.index === i).key;
  assert.deepEqual(edited((l) => R.moveRule(l, 1, k(1), -1)).rule.map((r) => r.name), [["steam*"], "firefox", "x", ["a", "b"]]);
  assert.equal(edited((l) => R.moveRule(l, 0, k(0), -1)).code, "NoSuchRule");
  assert.equal(edited((l) => R.moveRule(l, 3, k(3), 1)).code, "NoSuchRule");
  assert.equal(edited((l) => R.moveRule(l, 1, k(0), 1)).code, "Changed");
  assert.deepEqual(edited((l, d) => R.setProfile(l, d, 0, k(0), "game")).rule[0], { profile: "game", name: "firefox" });
  assert.equal(edited((l, d) => R.setProfile(l, d, 0, k(0), "nope")).code, "BadProfile");
  assert.deepEqual(edited((l) => R.setCondition(l, 2, k(2), "uid", "1000")).rule[2], { profile: "background", exe: "/opt/x/*", name: "x", uid: [1000] });
  assert.equal(edited((l) => R.setCondition(l, 2, k(2), "color", "x")).code, "BadField");
  assert.deepEqual(edited((l) => R.renameCondition(l, 0, k(0), "name", "cmdline")).rule[0], { profile: "desktop", cmdline: "firefox" });
  assert.equal(edited((l) => R.renameCondition(l, 2, k(2), "name", "exe")).code, "Taken");
  assert.equal(edited((l) => R.renameCondition(l, 0, k(0), "name", "uid")).code, "BadUid");
  assert.equal(edited((l) => R.renameCondition(l, 0, k(0), "name", "not_name")).code, "NeedsMatch");
  assert.deepEqual(edited((l) => R.removeCondition(l, 2, k(2), "exe")).rule[2], { profile: "background", name: "x" });
  assert.equal(edited((l) => R.removeCondition(l, 0, k(0), "name")).code, "NeedsMatch");
  assert.deepEqual(edited((l) => R.addCondition(l, 0, k(0))).rule[0], { profile: "desktop", name: "firefox", exe: "*" });
  const all = { profile: "d" };
  for (const f of R.FIELDS) all[f] = f === "uid" ? [1] : "*";
  delete all.uid;
  const text = JSON.stringify({ layer: { rule: [all] } });
  assert.deepEqual(edited((l) => R.addCondition(l, 0, JSON.stringify(all)), text).rule[0].uid, [1000]);
  assert.deepEqual(plain(R.fieldChoices([{ field: "name" }, { field: "exe" }], "exe")).map((c) => c.value),
    plain(R.FIELDS).filter((f) => f !== "name"));
});

test("file_rules_for_reading", () => {
  const b = plain(R.baseRules(raw().base));
  assert.deepEqual(b, { rows: [{ profile: "background", summary: "name = baloo*", source: "/usr/share/scdr/scdr.toml" }], total: 1 });
  const many = { rule: Array.from({ length: 400 }, (_, i) => ({ profile: "p", name: "n" + i })) };
  const m = R.baseRules(many);
  assert.equal(m.rows.length, 300);
  assert.equal(m.total, 400);
  assert.deepEqual(plain(R.baseRules(null)), { rows: [], total: 0 });
});

test("page_scripts_stay_small_and_plain", () => {
  const scripts = readdirSync(pageDir).filter((f) => f.endsWith(".js"));
  assert.deepEqual(scripts.sort(), ["logic.js", "profiles.js", "rules.js", "schedulers.js", "words.js"]);
  for (const f of scripts) {
    const text = readFileSync(`${pageDir}${f}`, "utf8");
    assert.ok(text.split("\n").length <= 400, `${f} is over 400 lines`);
    assert.match(text, /^\.pragma library\n/, f);
    assert.doesNotMatch(text, /\bQt\./, f);
  }
  // Every message the editors can give is plain words.
  const words = /serde|json|d-bus|dbus|polkit|systemctl|scdrd|sched_ext|eevdf|[{}]/i;
  for (const m of [S.MESSAGES, P.MESSAGES, R.MESSAGES]) {
    for (const t of Object.values(m)) assert.doesNotMatch(t, words, t);
  }
});
