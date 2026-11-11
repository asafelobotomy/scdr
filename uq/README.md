# uQ integration

This folder adds one page to [uQ](https://github.com/asafelobotomy/uq) Settings. It does nothing
without uQ: no program here reads it, and scdr works the same with or without it.

## What it adds

- `pages/scdr/`: the **Scheduling** page, under **Apps** in uQ Settings. It shows whether scdr is
  running, pauses and resumes it, switches the boosts (the app you use, audio, games, apps in the
  background) on and off, picks the CPU scheduler, lists and edits your own app rules, and turns
  "follow the focused window" on and off.

Changes are made the way the Plasma widget makes them: over scdr's D-Bus interface, so polkit's
`org.scdr.control` asks for your password. Pausing and removing a rule ask first.

## Where it installs

`packaging/install.sh` copies the whole `pages/scdr/` folder to
`/usr/share/uq/settings/pages/scdr/`. The package lists `uq` as an optional dependency.

## How it was tested

- `node --test 'tests/uq/*.test.mjs'`: the page's logic (`logic.js`) and its manifest.
- `QT_QPA_PLATFORM=offscreen qmltestrunner -import tests/uq/stub -input tests/uq`: the page
  itself, against uQ's test kit. This runs in CI (`.github/workflows/ci.yml`, job `uq`).
- `sh tests/uq/install_test.sh`: the package installs exactly this folder's files.
- `uqctl integrations check uq/`: uQ's own check of the manifest and the page.

To try it without installing: `uq-settings --plugin-dir uq/pages`.
