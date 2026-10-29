# SCDR

A process scheduling and priority daemon for Arch Linux on EEVDF and
sched_ext kernels (6.12+). It's a modern successor to
[system76-scheduler](https://github.com/pop-os/system76-scheduler), drawing on
ananicy-cpp, GameMode and scx-loader. See [DESIGN.md](DESIGN.md) for the
reasoning and the roadmap.

**Status: milestone 5.** Rule-based per-process priorities, systemd unit
weights, live roles for the focused app, audio and games, pressure-driven
background demotion, sched_ext / EEVDF policy, rule importers, and a latency
benchmark. Packaged for the AUR (`packaging/aur`).

## What it does

- Watches process fork/exec/exit through the kernel's proc connector (netlink).
  No BPF toolchain or helper process. Falls back to rescans when that isn't
  available.
- Matches each process against TOML rules (name, exe, cmdline, cgroup, parent,
  uid, with `not_` negations) and applies a profile to **every thread**:
  - `nice`
  - `sched` policy
  - `slice_us`: the EEVDF per-task time slice, the modern replacement for the
    CFS latency knobs
  - `io` priority
  - `oom_score_adj`
  - `uclamp_min`/`uclamp_max`: CPU frequency hints, applied only while a CPU
    uses the schedutil governor
- Sets `cpu_weight`/`io_weight` on the process's systemd unit (service or app
  scope) through systemd itself (`systemctl set-property --runtime`, reaching
  user units via `--machine=USER@`). systemd turns the cgroup controllers on
  where needed and stays the owner of the cgroup tree. Weights a unit sets
  itself are never overridden; KDE's baloo ships with `CPUWeight=1`, for example.
- Reports per disk whether io priorities and `IOWeight=` take effect (only
  under bfq, or with iocost). `[system] iocost = true` enables iocost where it
  would help.
- Only changes values it owns. It remembers each process's original state and
  leaves alone threads that rtkit, GameMode, `renice` or `chrt` changed.
- Leaves launchers (`nice`, `chrt`, `gamemoderun`, …) and anything they start
  alone. It also spots processes started through them after they've exec'd away.
- Roles give processes no rule matches a profile for what they're doing now:
  - `foreground`: the focused window's app (its whole `app.slice` unit), from a
    KWin script loaded for the session only (nothing is written to kwinrc)
  - `audio`: processes with a *running* PipeWire audio stream
  - `game`: processes registered with GameMode

  A per-user session agent (`scdrctl agent`, run by `scdr-session.service`)
  reports these. Focus comes from a KWin script on Plasma, Hyprland's or
  niri's IPC, or the bundled GNOME Shell extension (`gnome-extensions enable
  scdr-foreground@scdr`); the Hyprland, niri and GNOME paths are tested
  against their documented event formats but not yet on those desktops; scdrd only accepts pids owned by the calling user. Roles can
  differ per power-profiles-daemon profile, e.g. no foreground boost on
  power-saver.
- Under CPU pressure (PSI above `[pressure] high`), apps that aren't focused
  get the `background` role (`CPUWeight=50`) until pressure drops below
  `low`, switching at most once every `hold_secs` (30 s). Without a session
  agent nothing is demoted, since scdr can't tell which app is focused.
  `scdrctl status` also shows memory and io pressure, which scdr doesn't act
  on: its levers share CPU and disk time, they don't free memory.
- Optional scheduler policy: `[sched_ext]` picks a sched_ext scheduler
  through scx_loader (gaming, then power profile, then default) and backs
  off if you switch by hand; `[eevdf]` sets the kernel's base slice and
  preemption mode, per power profile, while no sched_ext scheduler runs.
- Restores everything on `systemctl stop scdrd`: threads, unit weights,
  autogroup and iocost. systemd keeps an empty runtime drop-in
  (`CPUWeight=`) for each unit it touched under `/run`; it has no effect and is
  gone after a reboot.
- `scdrctl explain <pid|name>` shows:
  - the matched rule and its file
  - the profile and what it inherits
  - original, target and current values
  - per-thread results
  - caveats, e.g. that io priority is ignored on non-bfq disks

## Build and install

```sh
cd packaging/arch && makepkg -si
systemctl enable --now scdrd
systemctl --user enable --now scdr-session   # roles: focus, audio, games
```

Or for development:

```sh
cargo build
cargo test
# See what the shipped config would do, without changing anything:
cargo run --bin scdrctl -- list --share-dir data
cargo run --bin scdrd -- --dry-run --once --share-dir data
```

## Panel widget (KDE Plasma 6)

The package installs a Plasma widget: right-click the panel, *Add or Manage
Widgets*, search for "SCDR". It shows the active scheduler, CPU pressure and
power profile, switches each role on or off, lists the apps boosted right now
(grouped by app), and pauses or resumes scdr. It's plain QML talking to scdrd
over the system bus and follows the daemon's `State` property, so it doesn't
poll. Changing anything needs root or polkit's `org.scdr.control`, which asks
for your password and remembers it for a few minutes.

## Using it

```sh
scdrctl status            # daemon state, managed counts, autogroup/sched_ext/io facts
scdrctl explain plasmashell
scdrctl explain 1234 --offline   # evaluate the config without the daemon
scdrctl list [--all]      # which profile each process would get (offline)
scdrctl ps                # what scdrd is applying right now, and why
scdrctl pause | resume    # restore everything / start again (polkit)
scdrctl role background off   # switch a role off or on (polkit)
scdrctl check             # validate config files
systemctl reload scdrd    # or: scdrctl reload (root)
```

## Configuration

The defaults live in `/usr/share/scdr/scdr.toml` and are documented inline.
Override them in `/etc/scdr/scdr.toml` or `/etc/scdr/rules.d/*.toml`; rules
from `/etc` are tried first, and the first match wins.

```toml
[profile.my-renderer]
inherit = "build"
nice = 10

[[rule]]
profile = "my-renderer"
name = ["blender"]
cmdline = ["* -b *"]   # background renders only
```

## Importing ananicy-cpp and system76-scheduler rules

```sh
# The CachyOS/ananicy game list (~13.5k names) as one rule:
scdrctl import ananicy /etc/ananicy.d --only Game -o /etc/scdr/rules.d/50-ananicy.toml
# system76-scheduler config and extra assignments:
scdrctl import system76 /etc/system76-scheduler/config.kdl \
    /etc/system76-scheduler/process-scheduler/*.kdl -o /etc/scdr/rules.d/50-system76.toml
systemctl reload scdrd
```

Imported profiles are prefixed (`ananicy-`, `s76-`). rules.d files are tried
before the shipped config, so the ananicy importer skips names your rules
already match (`--keep-overlaps` to include them), and `--only` limits it to
the types you want. Importing everything also brings CachyOS's opinions with
it: NetworkManager as a background service, or `bash` at nice -4. The
output is checked with scdr's own config loader before it's written.

## Benchmark

`scdrctl bench --repeat 5` measures how late an interactive-style thread
(1 ms sleeps) wakes while twice as many busy threads as CPUs run, under
scdr's levers. Run it with scdrd stopped. One run on an i5-8300H (8 CPUs,
EEVDF, kernel 7.1), medians of 5 rounds:

| scenario | p99 | max | load throughput |
|---|---|---|---|
| baseline | 794 µs | 8.3 ms | 100% |
| probe slice 2000 / 1000 / 500 µs | 1.1–1.3 ms | 4.8–14 ms | ~100% |
| load at nice 5 (background) | 468 µs | 2.8 ms | ~90% |
| slice 1000 µs + load nice 5 | 706 µs | 2.9 ms | ~90% |

Lowering the competing load's priority (nice, `CPUWeight`) is what helps a
waking task. A shorter EEVDF slice made no measurable difference for this
sleep/wake pattern, so the default `slice_us` values are not yet justified by
data. Measure on your hardware before relying on them.

## Contributing

- **No source file over 400 lines**, tests included. `tests/file_length.rs`
  enforces this in `cargo test`. When a file grows past that, split it into a
  module directory.
- Run `cargo clippy --all-targets` and `cargo test` before committing.
