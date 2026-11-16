# uQ integration

This folder adds one page to [uQ](https://github.com/asafelobotomy/uq) Settings. It does nothing
without uQ: no program here reads it, and scdr works the same with or without it.

## What it adds

- `pages/scdr/`: the **Scheduling** page, under **Apps** in uQ Settings, laid out like Settings'
  own pages (`uq_api = 3`): six tabs of cards.
  - **Overview**: running or paused (Pause shows what would be put back first), CPU pressure as a
    bar, the boosted apps with their icons, and what scdr sees (governor, disks, power profile).
  - **Roles**: the boosts, the profile each one gives, "follow the focused window" and the CPU
    pressure thresholds.
  - **Scheduler**: the CPU scheduler by default, in games and per power profile, trials, the
    installer for more schedulers (each shows the command it would run), and the kernel scheduler's knobs.
  - **Rules**: your own rules, profiles, the rules from scdr's files, Explain for a process, and
    the running apps with the rule each gets.
  - **Tools**: check that scdr's files load, make scdr read them again, the latency benchmark
    with progress, and a preview of importing ananicy-cpp or system76-scheduler rules.
  - **Preferences**: the other settings (system switches, agent tools, programs left alone),
    followed live from scdr's files with the reason when scdr refuses a value, and "Reset my
    changes", which lists what goes first.

Profile, rule and scheduler changes are made the way the Plasma widget makes them: over scdr's
D-Bus interface, so polkit's `org.scdr.control` asks for your password. Everything else is a
command of `scdrctl`, declared in `page.toml`, which uQ confirms with a preview:

- `scdrctl config show|set|unset|reset|path [--dry-run] [--json]`: the Preferences tab.
- `scdrctl pause --dry-run --json`, `scdrctl schedulers <action> --dry-run --json`,
  `scdrctl import <from> --dry-run --json`: the plan `{summary, changes[], would_refuse, reason}`
  that uQ shows before it asks.
- `scdrctl check --json`, `scdrctl list [--all] --json`, `scdrctl bench --check --json` and
  `scdrctl bench --ndjson` (one progress line per step).

Writing imported rules needs root (they go to `/etc/scdr/rules.d`), so that tab shows the command
for a terminal instead of running it.

## Where it installs

`packaging/install.sh` copies the whole `pages/scdr/` folder to
`/usr/share/uq/settings/pages/scdr/`. The package lists `uq` as an optional dependency.

## How it was tested

- `node --test 'tests/uq/*.test.mjs'`: the page's logic (`logic.js`, `plans.js`, …) and its manifest.
- `cargo test`: `tests/cli.rs` runs `scdrctl`'s JSON against a throwaway config tree.
- `QT_QPA_PLATFORM=offscreen qmltestrunner -import tests/uq/stub -input tests/uq`: the page
  itself, against uQ's test kit. This runs in CI (`.github/workflows/ci.yml`, job `uq`).
- `sh tests/uq/install_test.sh`: the package installs exactly this folder's files.
- `uqctl integrations check uq/`: uQ's own check of the manifest and the page.

To try it without installing: `uq-settings --plugin-dir uq/pages`.
