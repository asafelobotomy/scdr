# SCDR — design

A modern desktop scheduling and priority daemon for Arch Linux. It is the successor to
pop-os/system76-scheduler and it learns from ananicy-cpp, GameMode, scx-loader and
CachyOS-Settings. It targets EEVDF and sched_ext kernels (6.12 and later), cgroup v2 and
systemd.

## 1. Why a new daemon

| Project | What to keep | What is broken or missing on a 2026 Arch system |
|---|---|---|
| system76-scheduler (Rust) | Rule profiles with inheritance; cgroup/parent conditions; foreground/background profiles; PipeWire monitor; exception list (`chrt`, `gamemoderun`, …) | CFS knobs (`latency_ns`, `min/wakeup_granularity_ns`) were removed in 6.6, so that half is silently a no-op. Spawns the Python `execsnoop-bpfcc`. Foreground only from pop-shell. Uses ionice only. |
| ananicy-cpp (C++) | Netlink proc connector; per-thread nice; `oom_score_adj`; topology-aware `cpuset` (X3D, P/E cores); the CachyOS rule set (about 15.8k entries) | `latency_nice` was never merged upstream. Fork/exec dedup race drops execs. sched and ioprio only reach the main thread. Moves processes into its own cgroups, against systemd. No autogroup handling, no foreground or audio awareness, no hot reload. |
| GameMode (C) | D-Bus/portal client protocol; pidfd registration; X3D/hybrid detection | Hardware tuning (governor, GPU, core parking) is out of scope for us. Its renice refuses to touch values it didn't set, so the two daemons fight. |
| scx-loader (Rust) | `org.scx.Loader` D-Bus: start or switch a sched_ext scheduler and mode | No policy for *when* to switch (no power or game awareness). |
| CachyOS-Settings | sysctl, zram, THP, udev I/O schedulers, `Delegate=cpu io …` on `user@.service` | Static. Nothing reacts to load or context. |

The gap: nothing combines **per-process policy** (who matters) with **system policy**
(which scheduler and settings), driven by **live context** (focused app, game, audio,
power profile, pressure), using the levers today's kernels actually have.

## 2. Kernel levers (checked against mainline and this machine's 7.1 kernel)

| Lever | Interface | Notes |
|---|---|---|
| nice | `setpriority`, per TID | Only works across sessions if autogroup is off, or if we set `/proc/PID/autogroup` nice. |
| **EEVDF custom slice** | `sched_setattr(sched_runtime)` on SCHED_NORMAL, per TID | Replaces `latency_nice`. Clamped to 0.1–100 ms (`fair.c:__setparam_fair`). A short slice means an earlier deadline, so the task wakes and preempts sooner: good for input, audio and compositors. |
| Policy | `sched_setattr` (other, batch, idle, fifo, rr) | Every thread, not just the main one. Add `SCHED_FLAG_RESET_ON_FORK` for RT. |
| **uclamp** | `sched_setattr` UTIL_CLAMP_MIN/MAX; `cpu.uclamp.*` per cgroup | Frequency hint for schedutil and EAS: raise min for the foreground app, cap max for background work. Only does something with the schedutil governor (intel_pstate passive or amd-pstate). |
| cgroup weight | `cpu.weight`, `io.weight` on systemd `app-*.scope` / `*.service` | Set through systemd (`systemctl set-property --runtime`; user units via `--machine=USER@`), so systemd enables the controllers and stays the owner, unlike ananicy's cgroup moves. The controllers are normally off inside `app.slice`/`session.slice`/`system.slice`, so there is no file to write directly. Has an effect even when autogroup is on. Weights only compete between sibling units. |
| I/O priority | `ioprio_set` | **Only takes effect under bfq.** On `none`, mq-deadline or kyber, use `io.weight` (bfq) or `io.cost` instead. Detect each disk's scheduler and pick the right lever. |
| OOM | `oom_score_adj` | Protect the compositor, audio and the focused app; offer browser tabs and builds first. |
| Affinity | `sched_setaffinity` | Only from explicit rules, using topology aliases (`p-cores`, `e-cores`, `x3d-cache`, `llc-N`). |
| Global EEVDF | debugfs `sched/base_slice_ns`, `sched/preempt` | `preempt` can be none, voluntary, full or lazy (`PREEMPT_DYNAMIC`). |
| sched_ext | `/sys/kernel/sched_ext/state`; `org.scx.Loader` | While a BPF scheduler is loaded, slice and uclamp may be ignored. The applier must check which scheduler is active. |
| Pressure | `/proc/pressure/{cpu,io,memory}` poll triggers | Event-driven, so no polling loop is needed. |
| Process events | netlink `CN_PROC` (fork, exec, comm, exit) | In-process. No bcc, no Python. Use a pidfd to avoid PID-reuse races. |

## 3. Architecture

```
                    ┌──────────────────────── scdrd (root, system service) ────────────────────────┐
 CN_PROC netlink ──▶│ Watcher ──▶ ProcTable(pid,start_time) ──▶ RuleEngine ──▶ Planner ──▶ Applier      │
 /proc initial scan │                     ▲                         ▲            │  (diff, ownership,   │
                    │                     │                         │            │   revert)            │
 PSI triggers ─────▶│ ContextBus ─────────┴─────────────────────────┘            ▼                      │
 power-profiles ───▶│  (foreground, game, audio, power, pressure, scx state)   systemd D-Bus (scopes)  │
 GameMode signals ─▶│                                                           sched_setattr/ioprio    │
 PipeWire ─────────▶│ SystemPolicy ──▶ org.scx.Loader / debugfs EEVDF / autogroup                       │
                    │ D-Bus: org.scdr.Daemon1   CLI: scdrctl                                  │
                    └──────────────────────────────────────────────────────────────────────────────────┘
        ▲ foreground pid (D-Bus, per session)
 scdr-session (user service): KWin script │ GNOME Shell ext │ Hyprland/Sway/niri IPC │ X11 _NET_ACTIVE_WINDOW
```

- **Watcher.** Events are keyed by `(pid, starttime)`, not by pid alone. This fixes
  ananicy's dedup race and the PID-reuse problem. It re-runs matching on `exec` and on
  `comm`. It delays cgroup-based matching until the process lands in its scope; the
  trigger is a cgroup event or a short retry, not system76's fixed 2 s sleep.
- **RuleEngine.** Matchers are compiled once: an Aho-Corasick set for names, globsets
  for exe, cmdline and cgroup paths. Conditions are `name`, `exe`, `cmdline`, `cgroup`,
  `unit`, `parent`/`ancestor`, `uid`, `flatpak` app id, and `!`-negation. Profiles
  inherit, as in system76. Rules hot-reload through inotify.
- **Planner.** Combines the static rule result with live context. The foreground app
  gets a boost; background gets demoted only under pressure; a game gets the game
  profile; audio gets the audio profile. The output is a desired state per process.
- **Applier.** Diffs the desired state against what is already applied. It records the
  original values and only reverts values that are still ours, which makes it safe to
  run beside GameMode and rtkit. Each action fails on its own without blocking the rest.
  Threads are handled by applying on fork for every TID of a managed TGID.
- **SystemPolicy.** A small state machine, power profile × {idle, desktop, gaming} ×
  pressure, that maps to one of:
  - a sched_ext scheduler and mode through scx-loader, e.g. `lavd --performance` when
    gaming on AC, `lavd --powersave` on battery, `bpfland` for low latency
  - or EEVDF settings (base slice, preempt mode) when sched_ext is off.

  It also controls autogroup.
- **Session agent.** Unprivileged. Reports the focused PID, the session's idle state
  and the screen lock. KDE Plasma Wayland gets a KWin script, which is the first target
  since this machine runs Plasma.

## 4. Configuration

TOML, to match scx-loader and Arch convention. Files are layered:
`/usr/share/scdr/` (shipped) < `/etc/scdr/` (admin) < `/etc/scdr/rules.d/*.toml`.

The shipped defaults, with every field documented, are in
[`data/scdr.toml`](data/scdr.toml).

**Importers:** `scdrctl import ananicy <dir>` and `import system76 <kdl>`. These
bring in the CachyOS game list (about 13.5k entries) without curating it by hand.

## 5. Interfaces

- D-Bus `org.scdr.Daemon1`:
  - `SetForeground(u)`, `Explain(u) → s`, `Status() → a{sv}`, `Reload()`
  - property `Mode`, signal `ModeChanged`
- Compatibility shims:
  - `com.system76.Scheduler.SetForegroundProcess`
  - listening for GameMode `GameRegistered`/`GameUnregistered`
- CLI:
  - `scdrctl status | explain <pid> | top | reload | mode <m> | import …`

  `explain` prints the matched rule, its inherited fields, what was applied, what failed
  and why (for example "ioprio ignored: nvme0n1 uses none").

## 6. Coexistence and safety

- If ananicy-cpp or system76-scheduler is running, refuse to start, or offer a takeover
  mode.
- Never touch kernel threads, PID 1, existing RT tasks or exception descendants (`chrt`,
  `nice`, `taskset`, `gamemoderun` …). Respect rtkit grants.
- Don't manage governor, GPU or core parking; GameMode and power-profiles-daemon own
  those.
- systemd unit with hardening:
  - capabilities `CAP_SYS_NICE`, `CAP_NET_ADMIN` (proc connector), `CAP_SYS_RESOURCE`
    (oom), plus `CAP_SYS_ADMIN` only if debugfs EEVDF tuning is enabled
  - `ProtectSystem=strict`, with `ReadWritePaths=` limited to the debugfs and proc
    paths it writes
- polkit rule so scdrd can drive `org.scx.Loader`. Its default is
  `auth_admin_keep`, but root needs no prompt.

## 7. Milestones

1. **Core.** Proc connector watcher, `/proc` scan, ProcTable, TOML rules, applier
   (nice, slice, policy, ioprio, oom), `explain`, PKGBUILD and systemd unit.
2. **cgroups** (done). systemd unit weights; uclamp, gated on schedutil; per-disk
   I/O lever detection with opt-in iocost; autogroup control (done in M1).
3. **Context** (done). Session agent (`scdrctl agent`) with a runtime-loaded KWin
   script, GameMode signals and `pw-dump` audio streams; `[roles]` with
   per-power-profile overrides from power-profiles-daemon.
4. **System policy** (done). scx_loader integration with hand-override back-off,
   EEVDF base slice / preempt per power profile, PSI-driven background role
   (sampled every 2 s, with hysteresis).
5. **Ecosystem** (done). `scdrctl import ananicy|system76`; Hyprland, niri and
   GNOME Shell foreground sources; `scdrctl bench` (wake-up latency under load);
   AUR `scdr` / `scdr-git` PKGBUILDs sharing `packaging/install.sh`; MPL-2.0.
   First benchmark: weight differences help waking tasks, shorter slices didn't.

## 7a. GUI

A Plasma 6 panel widget (`data/plasmoid`), pure QML on Plasma's D-Bus QML
module: it binds to the daemon's `State` property (a JSON summary, emitted
through PropertiesChanged when it changes, at most every 2 s) and calls
`Pause`, `Resume` and `SetRoleEnabled`. Those, and `Reload`, need root or
polkit's `org.scdr.control` (`auth_admin_keep`); pause and role switches
persist in `/var/lib/scdr/runtime.json`. A full app (Kirigami via cxx-qt,
or Slint for other desktops) can build on the same API, plus `Processes()`
and `Explain()`.

## 8. Conventions

- No source file over 400 lines (enforced by `tests/file_length.rs`).

## 9. Open questions

- Whether a session-wide boost belongs in the default config at all. Wayland
  compositors already get rtkit, so it may not be needed.
- Whether nice or a custom slice should be the main foreground lever under EEVDF. To be
  settled by the M5 benchmarks.
- Whether to ship the CachyOS rules as a dependency (`cachyos-ananicy-rules`) or as a
  one-time import.
