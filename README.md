# SCDR

A process scheduling and priority daemon for Arch Linux on EEVDF and
sched_ext kernels (6.12+). It's a modern successor to
[system76-scheduler](https://github.com/pop-os/system76-scheduler), drawing on
ananicy-cpp, GameMode and scx-loader. See [DESIGN.md](DESIGN.md) for the
reasoning and the roadmap.

**Status: milestone 2 (cgroups).** Rule-based per-process priorities plus
systemd unit weights. Not yet included: foreground boosting, GameMode/PipeWire
awareness, sched_ext switching.

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
```

Or for development:

```sh
cargo build
cargo test
# See what the shipped config would do, without changing anything:
cargo run --bin scdrctl -- list --share-dir data
cargo run --bin scdrd -- --dry-run --once --share-dir data
```

## Using it

```sh
scdrctl status            # daemon state, managed counts, autogroup/sched_ext/io facts
scdrctl explain plasmashell
scdrctl explain 1234 --offline   # evaluate the config without the daemon
scdrctl list [--all]      # which profile each process would get
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

## Contributing

- **No source file over 400 lines**, tests included. `tests/file_length.rs`
  enforces this in `cargo test`. When a file grows past that, split it into a
  module directory.
- Run `cargo clippy --all-targets` and `cargo test` before committing.
