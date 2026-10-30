#!/usr/bin/env bash
# Exercises the actual Rust Host, never the user's session bus.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd)
REPO=$(cd -- "$HERE/../.." && pwd)
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
for MODE in external owned lifecycle; do
 ROOT=$(mktemp -d /tmp/decebal-sni-backend.XXXXXX)
 cleanup() { set +e; [[ -n ${TEST_PID:-} ]] && kill "$TEST_PID" 2>/dev/null; [[ -n ${FIXTURE_PID:-} ]] && kill "$FIXTURE_PID" 2>/dev/null; [[ -n ${BUS_PID:-} ]] && kill "$BUS_PID" 2>/dev/null; wait 2>/dev/null; rm -rf "$ROOT"; }
 trap cleanup EXIT
 export HOME="$ROOT/home" XDG_CONFIG_HOME="$ROOT/config" XDG_CACHE_HOME="$ROOT/cache" XDG_STATE_HOME="$ROOT/state" XDG_DATA_HOME="$ROOT/data" XDG_RUNTIME_DIR="$ROOT/runtime" DECEBAL_SNI_TEST_ROOT="$ROOT"
 mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
 chmod 700 "$XDG_RUNTIME_DIR"
 unset DISPLAY WAYLAND_DISPLAY DBUS_SESSION_BUS_PID
 export DBUS_SESSION_BUS_ADDRESS="unix:path=$ROOT/bus" DECEBAL_SNI_WATCHER_MODE="$MODE"
 start_services() {
  dbus-daemon --session --nofork --address="$DBUS_SESSION_BUS_ADDRESS" >>"$ROOT/bus.log" 2>&1 & BUS_PID=$!
  for _ in {1..100}; do [[ -S "$ROOT/bus" ]] && break; sleep .02; done
  ARGS=(); [[ $MODE == external ]] && ARGS+=(--watcher)
  /usr/bin/python3 "$HERE/sni-fixture.py" --root "$ROOT" --initial-failures 2 "${ARGS[@]}" >>"$ROOT/fixture.log" 2>&1 & FIXTURE_PID=$!
  for _ in {1..100}; do [[ -f "$ROOT/ready" ]] && break; kill -0 "$FIXTURE_PID" 2>/dev/null || { cat "$ROOT/fixture.log"; exit 1; }; sleep .02; done
 }
 TEST=private_bus_backend
 if [[ $MODE == lifecycle ]]; then TEST=private_bus_reconnect; else start_services; fi
 echo "Testing real backend: $MODE"
 (cd "$REPO"; CARGO_BUILD_JOBS=4 cargo test --lib "sni::private_tests::$TEST" -- --ignored --exact --test-threads=1) & TEST_PID=$!
 STARTED=false; RESTARTED=false
 while kill -0 "$TEST_PID" 2>/dev/null; do
  if [[ $MODE == lifecycle ]]; then
   if [[ -f "$ROOT/start-request" && $STARTED == false ]]; then start_services; STARTED=true; fi
   if [[ -f "$ROOT/restart-request" && $RESTARTED == false ]]; then
    kill "$FIXTURE_PID" "$BUS_PID"; wait "$FIXTURE_PID" 2>/dev/null || true; wait "$BUS_PID" 2>/dev/null || true
    rm -f "$ROOT/bus" "$ROOT/ready"; sleep .2; start_services; RESTARTED=true
   fi
  fi
  sleep .02
 done
 STATUS=0; wait "$TEST_PID" || STATUS=$?; unset TEST_PID
 if [[ $STATUS != 0 ]]; then cat "$ROOT/fixture.log" "$ROOT/bus.log" "$ROOT/events.jsonl" 2>/dev/null || true; exit "$STATUS"; fi
 cleanup
 unset FIXTURE_PID BUS_PID
 trap - EXIT
done
