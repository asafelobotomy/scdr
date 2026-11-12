#!/bin/sh
# The package installs exactly the page's files, readable and not executable, and nothing else
# under usr/share/uq. Run from anywhere: sh tests/uq/install_test.sh
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cd "$root"

mkdir "$tmp/target"
: >"$tmp/target/scdrd"
: >"$tmp/target/scdrctl"
packaging/install.sh "$tmp/root" "$tmp/target" >/dev/null

fail() {
  echo "FAIL: $1"
  exit 1
}

want=$(cd uq/pages/scdr && find . -type f | sort)
[ -n "$want" ] || fail "uq/pages/scdr has no files"
dir="$tmp/root/usr/share/uq/settings/pages/scdr"
[ -d "$dir" ] || fail "the page folder was not installed"
got=$(cd "$dir" && find . -type f | sort)
[ "$got" = "$want" ] || fail "installed files differ from uq/pages/scdr
want:
$want
got:
$got"

for f in $got; do
  mode=$(stat -c %a "$dir/$f")
  [ "$mode" = 644 ] || fail "$f has mode $mode, not 644"
done

others=$(cd "$tmp/root/usr/share/uq" && find . -type f | sort | grep -v '^\./settings/pages/scdr/' || true)
[ -z "$others" ] || fail "other files under usr/share/uq: $others"

echo PASS
