# scdr

Process scheduling and priority daemon (`scdrd`) and control tool (`scdrctl`) for Arch Linux on EEVDF/sched_ext kernels. Rust, single crate. See `DESIGN.md` for rationale and roadmap.

## Commands
- Build: `cargo build`
- Test: `cargo test --locked` (includes `tests/file_length.rs`).
- Lint: `cargo clippy --locked --all-targets -- -D warnings` (what CI runs).
- Dry run against the shipped config, changing nothing: `cargo run --bin scdrctl -- list --share-dir data` and `cargo run --bin scdrd -- --dry-run --once --share-dir data`.
- Package for Arch: `cd packaging/arch && makepkg -si`; then `systemctl enable --now scdrd` and `systemctl --user enable --now scdr-session`.

## Layout
- `src/bin/scdrd.rs` (daemon) and `src/bin/scdrctl/` (CLI); library in `src/lib.rs` with modules such as `agent`, `apply`, `config`, `import`, `rules`, `service`, `watcher`, `policy`, `schedulers`, `procfs`, `dbus`.
- `tests/file_length.rs`: the only integration test (line-limit rule).
- `data/`: shipped config and units (`scdr.toml`, `scdrd.service`, `scdr-session.service`, `user@-io.conf` drop-in delegating io to user managers, D-Bus and polkit files), plus `kwin/`, `gnome-shell/`, `plasmoid/` (Plasma widget).
- `packaging/`: `arch/` (PKGBUILD), `aur/` (`scdr`, `scdr-git`), `install.sh`.

## Conventions
- No source file over 400 lines, tests included; `tests/file_length.rs` enforces it. Split into a module directory (`foo/mod.rs` + `foo/bar.rs`).
- Run `cargo clippy --all-targets` and `cargo test` before committing.
- Rust edition 2024, MSRV 1.85, MPL-2.0. Work on `main`. CI (`ci.yml`) runs clippy and tests. Pushing a `vX.Y.Z` tag runs `release.yml`: it uploads a source archive, builds the AUR package from it and keeps the AUR files (real checksum, `.SRCINFO`) as the `aur` artifact. Tagging, releasing and pushing to the AUR are the user's steps, never an agent's.
- Do not edit or commit build output in `packaging/arch` (`pkg/`, `src/`, `*.pkg.tar.zst` are git-ignored).

## Gotchas
- The daemon acts on live processes and systemd units; prefer `--dry-run` while developing.
- Hyprland, niri and GNOME focus paths are tested against documented event formats only, not on those desktops (per README).
- scdrd exits 3 (no restart) while ananicy-cpp or system76-scheduler runs (`src/rivals.rs`); `--dry-run` skips the check.
- The session agent follows logind's `LockedHint` (`src/agent/lock.rs`) and sends `SetForeground(0)` while locked, so nothing stays boosted behind the lock screen.

- With scx_devbox running and announcing `/run/scx_devbox/api`, scdrd writes focus hints to `/run/scdr/devbox-hints` and its foreground/background roles step aside (`src/service/devbox.rs`, `scx_devbox/src/hints.h`, `devbox.focus.h`).

- `[split]` (`src/config/split.rs`, `src/service/split.rs`): for listed apps, processes running a different binary from the app move into `app-<app>-tools-<pid>.scope` via busctl `StartTransientUnit` (run as the user) in `app-<app>\x2dtools.slice`, at scan or the 1.5 s exec re-check. The slice gets `memory_high`/`io_weight`/`cpu_weight` (undone on stop) and is sent to scx_devbox as a `neutral` hint. Test it against a dummy app scope with a dev `scdrd --no-dbus --share-dir <dir with only [split]>`, not on Claude itself.

## uQ integration
- uQ launches apps as `app-uq-<id>-<random hex>.scope` in `app.slice`, but Claude Desktop's launcher (`/usr/bin/claude-desktop`) re-wraps itself in `app-com.anthropic.Claude-<pid>.scope`, so `[split]`, the tools slice and the devbox hints see the same shape on uQ as on Plasma; `uq-com_anthropic_Claude` in `[split] apps` covers `CLAUDE_DISABLE_SYSTEMD_SCOPE=1`.
- uQ's units run in `session.slice`, and its `base.lua` restarts `scdr-session` with the session's environment. bindle's `scdr` module imports ananicy rules before removing ananicy, then enables `scdrd`; `scdr-sched-ext` turns on `[sched_ext]` through `/etc/scdr/rules.d/40-bindle-sched-ext.toml`.
