# Tests for the uQ Settings page (`uq/pages/scdr/`)

- `logic.test.mjs`, `load-logic.mjs`, `fixtures/`: Node tests of `logic.js` and the manifest.
  Run: `node --test 'tests/uq/*.test.mjs'` (Node 22; the quotes matter, a bare directory fails).
- `tst_page.qml` with `FakeContext.qml` and `stub/`: the page offscreen, no uQ needed.
  Run: `QT_QPA_PLATFORM=offscreen qmltestrunner -import tests/uq/stub -input tests/uq`.
  CI runs it (the `uq` job); `tst_components.qml` checks the stub itself.
- `install_test.sh`: the package installs exactly the page's files.

`FakeContext.qml`, `stub/` and `tst_components.qml` are copied from uQ's
`templates/settings-page-tests/` at v0.3.0-rc.1. Refresh them when moving to a newer uQ.
Nothing here is installed.

---

# Settings page test kit

Copy this folder into your app's repository as `tests/uq/` to test your page offscreen, without
uQ Settings or Kirigami:

```
tests/uq/stub/org/uq/settings/   stand-ins for the components pages may use (UqLabel, …)
tests/uq/FakeContext.qml         a stand-in `ctx`: canned command and D-Bus answers, a call log
tests/uq/tst_page.qml            an example test: loads the page, gives it the fake ctx
tests/uq/tst_components.qml      checks the stub itself (keep or drop)
```

1. In `tst_page.qml`, point `pageUrl` at your page: `Qt.resolvedUrl("../../uq/pages/<id>/Page.qml")`.
2. Give `FakeContext` the answers your page expects: `commands` (command id → result, or a
   function of the arguments), `dbus` (method or property → `{value}` or `{error}`), and
   `confirmed`/`confirms` for commands with `confirm`. Every request is logged in `calls`.
   `fake.change(id)` stands for a change to a watched command's files or signals: each
   `ctx.command.watch` of it runs again.
3. Run it:

   ```
   QT_QPA_PLATFORM=offscreen qmltestrunner -import tests/uq/stub -input tests/uq
   ```

   (`/usr/lib/qt6/bin/qmltestrunner` on Arch.) Pure logic in a `logic.js` can also be tested
   with Node's built-in runner.

The stub matches the components as documented for `uq_api = 1` in uQ's
`docs/settings-pages.md` ("Components pages may use"); uQ's own tests check that every property
and signal of the stub exists on the real component. Refresh your copy when you move to a newer
uQ.
