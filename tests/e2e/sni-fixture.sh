#!/usr/bin/env bash
# Protocol-only self-test: no desktop, user bus, X server or compiled binaries.
# For manual private native testing: start a private daemon with --address
# unix:path=$ROOT/bus, then run sni-fixture.py --root "$ROOT" [--watcher].
set -euo pipefail
ROOT=$(mktemp -d /tmp/decebal-sni.XXXXXX)
cleanup() { set +e; [[ -n ${FIXTURE_PID:-} ]] && kill "$FIXTURE_PID" 2>/dev/null; [[ -n ${BUS_PID:-} ]] && kill "$BUS_PID" 2>/dev/null; wait 2>/dev/null; rm -rf "$ROOT"; }
trap cleanup EXIT
export HOME="$ROOT/home" XDG_CONFIG_HOME="$ROOT/config" XDG_CACHE_HOME="$ROOT/cache" XDG_STATE_HOME="$ROOT/state" XDG_DATA_HOME="$ROOT/data" XDG_RUNTIME_DIR="$ROOT/runtime"
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
unset DISPLAY WAYLAND_DISPLAY DBUS_SESSION_BUS_PID
export DBUS_SESSION_BUS_ADDRESS="unix:path=$ROOT/bus"
dbus-daemon --session --nofork --address="$DBUS_SESSION_BUS_ADDRESS" >"$ROOT/bus.log" 2>&1 &
BUS_PID=$!
for _ in {1..100}; do [[ -S "$ROOT/bus" ]] && break; sleep .02; done
HERE=$(cd -- "$(dirname -- "$0")" && pwd)
/usr/bin/python3 "$HERE/sni-fixture.py" --root "$ROOT" --watcher >"$ROOT/fixture.log" 2>&1 &
FIXTURE_PID=$!
for _ in {1..100}; do [[ -f "$ROOT/ready" ]] && break; kill -0 "$FIXTURE_PID" 2>/dev/null || { cat "$ROOT/fixture.log"; exit 1; }; sleep .02; done
/usr/bin/python3 "$HERE/sni-protocol.py" "$ROOT" || { cat "$ROOT/fixture.log"; exit 1; }
