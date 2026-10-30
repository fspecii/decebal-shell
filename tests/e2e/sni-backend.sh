#!/usr/bin/env bash
# Exercises the actual Rust Host, never the user's session bus.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd)
REPO=$(cd -- "$HERE/../.." && pwd)
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
for MODE in external owned; do
 ROOT=$(mktemp -d /tmp/decebal-sni-backend.XXXXXX)
 cleanup() { set +e; [[ -n ${FIXTURE_PID:-} ]] && kill "$FIXTURE_PID" 2>/dev/null; [[ -n ${BUS_PID:-} ]] && kill "$BUS_PID" 2>/dev/null; wait 2>/dev/null; rm -rf "$ROOT"; }
 trap cleanup EXIT
 export HOME="$ROOT/home" XDG_CONFIG_HOME="$ROOT/config" XDG_CACHE_HOME="$ROOT/cache" XDG_STATE_HOME="$ROOT/state" XDG_DATA_HOME="$ROOT/data" XDG_RUNTIME_DIR="$ROOT/runtime" DECEBAL_SNI_TEST_ROOT="$ROOT"
 mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
 chmod 700 "$XDG_RUNTIME_DIR"
 unset DISPLAY WAYLAND_DISPLAY DBUS_SESSION_BUS_PID
 export DBUS_SESSION_BUS_ADDRESS="unix:path=$ROOT/bus"
 dbus-daemon --session --nofork --address="$DBUS_SESSION_BUS_ADDRESS" >"$ROOT/bus.log" 2>&1 & BUS_PID=$!
 for _ in {1..100}; do [[ -S "$ROOT/bus" ]] && break; sleep .02; done
 ARGS=(); [[ $MODE == external ]] && ARGS+=(--watcher)
 /usr/bin/python3 "$HERE/sni-fixture.py" --root "$ROOT" "${ARGS[@]}" >"$ROOT/fixture.log" 2>&1 & FIXTURE_PID=$!
 for _ in {1..100}; do [[ -f "$ROOT/ready" ]] && break; kill -0 "$FIXTURE_PID" 2>/dev/null || { cat "$ROOT/fixture.log"; exit 1; }; sleep .02; done
 export DECEBAL_SNI_WATCHER_MODE="$MODE"
 echo "Testing real backend with $MODE watcher"
 (cd "$REPO"; CARGO_BUILD_JOBS=4 cargo test --lib sni::private_tests::private_bus_backend -- --ignored --exact --test-threads=1) || { cat "$ROOT/fixture.log"; cat "$ROOT/events.jsonl"; exit 1; }
 cleanup
 unset FIXTURE_PID BUS_PID
 trap - EXIT
done
