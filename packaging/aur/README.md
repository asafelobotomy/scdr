# AUR packages

- `scdr-git/` builds the latest `main` from GitHub.
- `scdr/` builds a tagged release (`v$pkgver` on GitHub).

Both install through `packaging/install.sh` from the source tree, the same as
the local `packaging/arch/PKGBUILD`.

## Publishing

1. Push this repository to `https://github.com/asafelobotomy/scdr`.
2. For a release, tag it (`git tag v0.1.0 && git push --tags`), then in
   `scdr/` run `updpkgsums` to replace the `SKIP` checksum.
3. Test in a clean chroot: `extra-x86_64-build` (devtools), or at least
   `makepkg -f && namcap PKGBUILD *.pkg.tar.zst`.
4. Regenerate metadata: `makepkg --printsrcinfo > .SRCINFO`.
5. Push to the AUR (needs an AUR account with your SSH key):

   ```sh
   git clone ssh://aur@aur.archlinux.org/scdr-git.git ~/aur/scdr-git
   cp packaging/aur/scdr-git/{PKGBUILD,.SRCINFO,scdr.install} ~/aur/scdr-git/
   cd ~/aur/scdr-git && git add -A && git commit -m "Initial upload" && git push
   ```

Expected `namcap` warnings: the empty `rules.d` directories (they're for
drop-ins), and "dbus/systemd may not be needed" (scdr talks to them over
D-Bus and `systemctl` at runtime rather than linking them).
