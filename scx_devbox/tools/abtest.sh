#!/bin/sh
# A/B latbench across schedulers, alternating rounds to spread out
# background noise. Each argument is a scheduler: "eevdf", or a path to a
# scx_devbox binary. Needs run0 (or sudo via SUDO=sudo) to switch.
#   tools/abtest.sh eevdf /usr/bin/scx_devbox ./build/scx_devbox
# LAUNCH=service (default) runs the bench as a transient user service: nice
# 0, default slice. LAUNCH=scope inherits the caller's nice and slice, e.g.
# scdr's foreground boost when run from the focused app's terminal: the case
# of an agent spawning CPU hogs.
set -eu
SUDO=${SUDO:-run0}
ROUNDS=${ROUNDS:-2}
SECS=${SECS:-6}
case ${LAUNCH:-service} in
service) launch="systemd-run --user --pipe --wait -q" ;;
scope) launch="systemd-run --user --scope -q" ;;
*) echo "LAUNCH must be service or scope" >&2; exit 2 ;;
esac
bench=$(dirname "$0")/../build/latbench
pid=
pidfile=$(mktemp)

stop_sched() {
	[ -n "$pid" ] && $SUDO kill -INT "$pid" 2>/dev/null || true
	pid=
	for _ in 1 2 3 4 5 6 7 8 9 10; do
		[ "$(cat /sys/kernel/sched_ext/state)" = disabled ] && return
		sleep 0.5
	done
}

start_sched() {
	[ "$1" = eevdf ] && return
	$SUDO sh -c "'$1' >/dev/null 2>&1 & echo \$!" > "$pidfile"
	pid=$(cat "$pidfile")
	for _ in 1 2 3 4 5 6 7 8 9 10; do
		[ "$(cat /sys/kernel/sched_ext/state)" = enabled ] && sleep 1 && return
		sleep 0.5
	done
	echo "scheduler $1 didn't start" >&2
	exit 1
}

trap 'stop_sched; rm -f "$pidfile"' EXIT
for r in $(seq 1 "$ROUNDS"); do
	for s in "$@"; do
		start_sched "$s"
		for mode in sleep spawn; do
			printf '%s round%s ' "$s" "$r"
			$launch "$(realpath "$bench")" "$SECS" 16 "$mode"
		done
		stop_sched
	done
done
