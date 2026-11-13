# AUR packages

- `scdr-git/` builds the latest `main` from GitHub.
- `scdr/` builds a tagged release (`v$pkgver` on GitHub).

Both are split packages that also build scx-devbox (`scx-devbox` from `scdr`,
`scx-devbox-git` from `scdr-git`), the sched_ext scheduler in `scx_devbox/`;
it needs a sched_ext kernel to build. Copy `scx-devbox.install` along with
`scdr.install` when publishing. bindle's `scdr-devbox` module installs
`scx-devbox`, so it needs the next release of `scdr` on the AUR.

Both install through `packaging/install.sh` from the source tree, the same as
the local `packaging/arch/PKGBUILD`.

## Publishing

1. Push this repository to `https://github.com/asafelobotomy/scdr`.
2. For a release, set `version` in `Cargo.toml` and `pkgver` in
   `scdr/PKGBUILD`, then tag it (`git tag v$pkgver && git push origin v$pkgver`).
   The release workflow uploads `scdr-$pkgver.tar.gz` and keeps the AUR files
   with its real checksum as the `aur` artifact; use those instead of
   `updpkgsums`.
3. Test in a clean chroot: `extra-x86_64-build` (devtools), or at least
   `makepkg -f && namcap PKGBUILD *.pkg.tar.zst`.
4. Regenerate metadata: `makepkg --printsrcinfo > .SRCINFO`.
5. Push to the AUR (needs an AUR account with your SSH key):

   ```sh
   git clone ssh://aur@aur.archlinux.org/scdr-git.git ~/aur/scdr-git
   cp packaging/aur/scdr-git/{PKGBUILD,.SRCINFO,scdr.install,scx-devbox.install} ~/aur/scdr-git/
   cd ~/aur/scdr-git && git add -A && git commit -m "Initial upload" && git push
   ```

Expected `namcap` warnings, and no errors: the empty `rules.d` directories
(they're for drop-ins); "dbus/systemd may not be needed" (scdr talks to them
over D-Bus and `systemctl` at runtime rather than linking them); "unused
shared library ld-linux-x86-64.so.2" (Rust binaries); the Plasma and uQ QML
modules as uninstalled or optional dependencies (the panel widget, Settings
page and bar widget are optional, hence `plasma-workspace`, `kquickcharts`,
`kcmutils` and `uq` in `optdepends`).
