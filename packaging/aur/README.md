# AUR packages

- `scdr-git/` builds the latest `main` from GitHub.
- `scdr/` builds a tagged release (`v$pkgver` on GitHub).

`scdr-git` is a split package that also builds `scx-devbox-git`, the
sched_ext scheduler in `scx_devbox/` (needs a sched_ext kernel to build).
`scdr/` stays daemon-only until a release tag contains `scx_devbox/`; then
split it the same way as `packaging/arch/PKGBUILD`. Copy `scx-devbox.install`
along with `scdr.install` when publishing.

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

Expected `namcap` warnings: the empty `rules.d` directories (they're for
drop-ins), and "dbus/systemd may not be needed" (scdr talks to them over
D-Bus and `systemctl` at runtime rather than linking them).
