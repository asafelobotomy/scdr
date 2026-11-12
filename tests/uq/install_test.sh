#!/bin/sh
# The package installs exactly the page's and the bar widget's files, readable and not
# executable, and nothing else under usr/share/uq. Run from anywhere: sh tests/uq/install_test.sh
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

# Each uQ folder installs exactly its files, mode 644: "<source> <installed>".
check_folder() {
  src=$1
  dir="$tmp/root/usr/share/uq/$2"
  want=$(cd "$src" && find . -type f | sort)
  [ -n "$want" ] || fail "$src has no files"
  [ -d "$dir" ] || fail "$src was not installed"
  got=$(cd "$dir" && find . -type f | sort)
  [ "$got" = "$want" ] || fail "installed files differ from $src
want:
$want
got:
$got"
  for f in $got; do
    mode=$(stat -c %a "$dir/$f")
    [ "$mode" = 644 ] || fail "$src/$f has mode $mode, not 644"
  done
}

check_folder uq/pages/scdr settings/pages/scdr
check_folder uq/widgets/scdr shell/widgets/scdr

others=$(cd "$tmp/root/usr/share/uq" && find . -type f | sort | grep -v -e '^\./settings/pages/scdr/' -e '^\./shell/widgets/scdr/' || true)
[ -z "$others" ] || fail "other files under usr/share/uq: $others"

echo PASS
