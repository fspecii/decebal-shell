#!/bin/sh
# Runs a pinned rdwm inside a private fixture and executes commands against it. Nothing live is touched.
# usage: [EXTRA_ENV="K=V ..."] [RDWM_CONFIG_FILE=FILE] private-rdwm.sh BIN_DIR WORK_DIR DISPLAY_NUM CMD...
#   BIN_DIR   directory holding rdwm and rdwmctl (read-only; e.g. a borrowed immutable release build)
#   WORK_DIR  must not exist; created private (HOME, XDG_*, the private socket, logs). Nothing is deleted.
#   DISPLAY_NUM  a reserved high display number (>= 700); see private-xvfb.sh for the ownership proof
#   CMD...    each argument is run with `sh -c` in the private environment (DISPLAY, RDWM_SOCK, RDWMCTL, HOME ... exported) after the WM answers
# rdwm creates its socket in $XDG_RUNTIME_DIR (it ignores RDWM_SOCK and otherwise falls back to the shared /tmp/rdwm-<uid>/), so XDG_RUNTIME_DIR is a
# private 0700 directory inside WORK_DIR and RDWM_SOCK points at the same file for the clients: the user's running rdwm cannot be reached.
set -eu
BIN=$1; W=$2; N=$3; shift 3
HERE=$(cd "$(dirname "$0")" && pwd)
[ ! -e "$W" ] || { echo "refusing to reuse existing work directory: $W" >&2; exit 2; }
. "$HERE/private-xvfb.sh"
mkdir -p "$W/home/.config/rdwm" "$W/tmp" "$W/state" "$W/run"; chmod 700 "$W/run"
if [ -n "${RDWM_CONFIG_FILE:-}" ]; then cp "$RDWM_CONFIG_FILE" "$W/home/.config/rdwm/config.toml"; fi
start_private_xvfb "$N" 1280x720 "$W/xvfb.log" || exit $?
trap 'stop_private_xvfb' EXIT
PRIV="HOME=$W/home XDG_CONFIG_HOME=$W/home/.config XDG_DATA_HOME=$W/home/.local/share XDG_CACHE_HOME=$W/home/.cache XDG_STATE_HOME=$W/state XDG_RUNTIME_DIR=$W/run TMPDIR=$W/tmp DISPLAY=$DISPLAY RDWM_SOCK=$W/run/rdwm.sock RDWMCTL=$BIN/rdwmctl ${EXTRA_ENV:-}"
env -i PATH="$PATH" $PRIV "$BIN/rdwm" >"$W/rdwm.log" 2>&1 &
WPID=$!
trap 'kill $WPID 2>/dev/null || true; wait $WPID 2>/dev/null || true; stop_private_xvfb' EXIT
i=0; while [ ! -S "$W/run/rdwm.sock" ] && [ $i -lt 40 ]; do sleep 0.25; i=$((i+1)); done
[ -S "$W/run/rdwm.sock" ] || { echo "rdwm did not create its private socket; see $W/rdwm.log" >&2; exit 4; }
for c in "$@"; do env -i PATH="$PATH" $PRIV sh -c "$c"; done
