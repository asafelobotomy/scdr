// Node tests of the bar widget's pure logic (uq/widgets/scdr/logic.js) and its manifest.
//   node --test tests/uq/widget.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import vm from "node:vm";

const dir = new URL("../../uq/widgets/scdr/", import.meta.url);
const source = readFileSync(new URL("logic.js", dir), "utf8")
  .split("\n")
  .filter((line) => !/^\s*\.(pragma|import)\b/.test(line))
  .join("\n");
const L = vm.createContext({});
vm.runInContext(source, L, { filename: "logic.js" });
const plain = (v) => JSON.parse(JSON.stringify(v));

const sample = (seq, over = {}) => ({
  seq, time: seq, interactive: 10 * seq, batch: seq, focus: 0, damped: 1, preempt: 2, rescue: 0,
  perf_pct: 60, wait_max_ms: seq / 10, wait_avg_ms: 0.02, ...over,
});
const stateText = (over = {}) => JSON.stringify({
  paused: false, managed: 3, power_profile: "balanced", cpu_pressure: 7.4, pressure_active: false,
  scheduler: ["sched_ext: devbox (started outside scdr; left alone)"],
  roles: [["game", "game", true], ["background", "background", false]], agents: 1,
  boosted: [], memory_pressure: 1, io_pressure: 2, devbox: [], ...over,
});

test("parseState normalises State and refuses junk", () => {
  const s = L.parseState(stateText({ boosted: [{ pid: 4, name: "a", profile: "p", role: null, why: "w", app: null }] }));
  assert.equal(s.paused, false);
  assert.equal(s.powerProfile, "balanced");
  assert.equal(s.cpuPressure, 7.4);
  assert.deepEqual(plain(s.roles[1]), { role: "background", profile: "background", enabled: false });
  assert.deepEqual(plain(s.boosted[0]), { pid: 4, name: "a", profile: "p", role: "", why: "w", app: "" });
  assert.equal(L.parseState(stateText({ cpu_pressure: null })).cpuPressure, null);
  assert.equal(L.parseState("{"), null);
  assert.equal(L.parseState("[]"), null);
  assert.equal(L.parseState(JSON.stringify({ managed: 1 })), null);
});

test("schedulerInfo names the running scheduler", () => {
  const name = (lines) => L.schedulerInfo(lines).name;
  assert.deepEqual(plain(L.schedulerInfo(["sched_ext: devbox (started outside scdr; left alone)", "eevdf: x"])),
    { name: "scx_devbox", note: "started outside scdr; left alone" });
  assert.equal(name(["sched_ext: bpfland:auto (by scdr)"]), "bpfland");
  assert.equal(name(["sched_ext: lavd:gaming (chosen by hand; scdr would pick bpfland:auto)"]), "lavd (gaming)");
  assert.equal(name(["sched_ext: bpfland_1.0.14_g123 (started outside scdr; left alone)"]), "bpfland");
  assert.equal(name(["sched_ext: eevdf (by scdr)", "eevdf: base_slice=3000us preempt=full"]), "Kernel default (EEVDF)");
  assert.equal(name(["eevdf: base_slice=3000us preempt=full"]), "Kernel default (EEVDF)");
  assert.equal(name(["sched_ext: scx_loader unavailable (no such name); install scx-tools"]), "Kernel default (EEVDF)");
  assert.equal(name([]), "Kernel default (EEVDF)");
  assert.equal(name(undefined), "Kernel default (EEVDF)");
});

test("record keeps 180 samples, skips seen ones and follows a loader restart", () => {
  const h = L.newHistory();
  assert.equal(h.length, 180);
  for (const k of L.KEYS) assert.equal(h.data[k].length, 180);
  const s1 = L.parseState(stateText({ devbox: [sample(1), sample(2)] }));
  assert.equal(L.record(h, s1, 1000), 2);
  assert.equal(h.lastSeq, 2);
  assert.ok(h.devbox);
  assert.deepEqual(plain(h.data.interactive.slice(-2)), [10, 20]);
  assert.deepEqual(plain(h.data.cpu.slice(-2)), [7.4, 7.4]);
  // The same samples again: nothing new.
  assert.equal(L.record(h, s1, 1500), 0);
  assert.equal(L.record(h, L.parseState(stateText({ devbox: [sample(2), sample(3)] })), 2000), 1);
  assert.equal(L.last(h, "wait_max"), 0.3);
  // Restarted: the sequence starts again.
  assert.equal(L.record(h, L.parseState(stateText({ devbox: [sample(1, { interactive: 99 })] })), 3000), 1);
  assert.equal(L.last(h, "interactive"), 99);
  for (const k of L.KEYS) assert.equal(h.data[k].length, 180);
  for (let i = 0; i < 400; i++) L.record(h, L.parseState(stateText({ devbox: [sample(10 + i)] })), 4000 + i);
  for (const k of L.KEYS) assert.equal(h.data[k].length, 180);
});

test("tick records pressure only while devbox is silent", () => {
  const h = L.newHistory(5);
  const s = L.parseState(stateText({ cpu_pressure: 40 }));
  assert.equal(L.tick(h, null, 10000), false);
  assert.equal(L.tick(h, s, 10000), true);
  assert.equal(L.last(h, "cpu"), 40);
  assert.equal(L.last(h, "interactive"), 0);
  L.record(h, L.parseState(stateText({ devbox: [sample(1)] })), 20000);
  assert.equal(L.tick(h, s, 21000), false);
  assert.equal(L.tick(h, s, 23000), true);
  assert.equal(h.data.cpu.length, 5);
});

test("scales", () => {
  const h = L.newHistory(10);
  assert.equal(L.waitScale(h), 20);
  L.push(h, "wait_max", 30);
  assert.equal(L.waitScale(h), 35);
  assert.equal(L.niceMax(0, 10), 10);
  assert.equal(L.niceMax(11, 10), 20);
  assert.equal(L.niceMax(230, 10), 500);
  assert.equal(L.niceMax(1000, 10), 1000);
  L.push(h, "interactive", 140);
  assert.equal(L.schedScale(h), 200);
  assert.deepEqual(plain(L.tail(h, "interactive", 2)), [0, 140]);
});

test("call outcomes", () => {
  assert.equal(L.callOutcome({ value: null }).status, "ok");
  assert.equal(L.callOutcome({ error: "no answer within 2 s" }).status, "waiting");
  assert.match(L.callOutcome({ error: "org.freedesktop.DBus.Error.AccessDenied: not authorized" }).message, /^Not allowed/);
  assert.equal(L.callOutcome({ error: "org.scdr.Daemon1 is not running" }).message, "scdrd isn't running.");
  assert.match(L.callOutcome({ error: "boom" }).message, /boom$/);
});

test("settled and labels", () => {
  const s = L.parseState(stateText());
  assert.equal(L.settled({ kind: "pause" }, s), false);
  assert.equal(L.settled({ kind: "resume" }, s), true);
  assert.equal(L.settled({ kind: "role", role: "background", enabled: true }, s), false);
  assert.equal(L.settled({ kind: "role", role: "game", enabled: true }, s), true);
  assert.equal(L.statusLabel(null, null), "Connecting…");
  assert.equal(L.statusLabel(s, null), "Running");
  assert.equal(L.statusLabel(s, { kind: "pause" }), "Waiting for your password…");
  assert.equal(L.pressureLine(L.parseState(stateText({ cpu_pressure: 61.6, pressure_active: true }))), "62% (demoting background apps)");
  assert.equal(L.pressureLine(L.parseState(stateText({ cpu_pressure: null }))), "–");
  const rows = L.grouped([
    { pid: 1, name: "web", app: "firefox", profile: "foreground", role: "foreground", why: "" },
    { pid: 2, name: "web2", app: "firefox", profile: "foreground", role: "foreground", why: "" },
    { pid: 3, name: "pipewire", app: "", profile: "audio", role: "audio", why: "" },
  ]);
  assert.deepEqual(plain(rows.map(L.boostedLine)), ["firefox ×2 — foreground", "pipewire — audio"]);
});

test("the manifest lists only what the widget uses", () => {
  const toml = readFileSync(new URL("widget.toml", dir), "utf8");
  assert.match(toml, /^id = "scdr"$/m);
  assert.match(toml, /^slot = "bar"$/m);
  assert.match(toml, /^bus = "system"$/m);
  assert.match(toml, /^bus_name = "org.scdr.Daemon1"$/m);
  assert.match(toml, /^methods = \["Pause", "Resume", "SetRoleEnabled"\]$/m);
  assert.match(toml, /^properties = \["State"\]$/m);
  const qml = readdirSync(dir).filter((f) => /\.(qml|js)$/.test(f));
  for (const f of qml) {
    const text = readFileSync(new URL(f, dir), "utf8");
    assert.doesNotMatch(text, /^import (Quickshell|org\.kde|QtQuick\.Controls)/m, f);
    for (const m of text.matchAll(/(?:dbus\.call|write)\("(\w+)"/g)) assert.match(toml, new RegExp(`"${m[1]}"`), f);
    assert.ok(text.split("\n").length <= 400, `${f} is over 400 lines`);
  }
});
