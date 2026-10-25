#!/bin/sh
# Installs a built SCDR into a package root. Run from the source tree:
#   packaging/install.sh DESTDIR [TARGET_DIR]
# Shared by packaging/arch and the AUR PKGBUILDs so they can't drift apart.
set -eu
dest=$1
target=${2:-target/release}
ext=scdr-foreground@scdr

install -Dm755 "$target/scdrd" "$dest/usr/bin/scdrd"
install -Dm755 "$target/scdrctl" "$dest/usr/bin/scdrctl"
install -Dm644 data/scdr.toml "$dest/usr/share/scdr/scdr.toml"
install -dm755 "$dest/usr/share/scdr/rules.d" "$dest/etc/scdr/rules.d"
install -Dm644 data/scdrd.service "$dest/usr/lib/systemd/system/scdrd.service"
install -Dm644 data/scdr-session.service "$dest/usr/lib/systemd/user/scdr-session.service"
install -Dm644 data/org.scdr.Daemon1.conf "$dest/usr/share/dbus-1/system.d/org.scdr.Daemon1.conf"
install -Dm644 -t "$dest/usr/share/gnome-shell/extensions/$ext" \
  "data/gnome-shell/$ext/metadata.json" "data/gnome-shell/$ext/extension.js"
install -Dm644 README.md "$dest/usr/share/doc/scdr/README.md"
install -Dm644 DESIGN.md "$dest/usr/share/doc/scdr/DESIGN.md"
install -Dm644 LICENSE "$dest/usr/share/licenses/scdr/LICENSE"
