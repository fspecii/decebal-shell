#!/usr/bin/env bash
# No locker process or authenticator is used; exercises render + X11 upload only.
set -eu
HERE=$(cd "$(dirname "$0")/../.." && pwd)
N=${E2E_DISPLAY:-:237}
case "$N" in :0|:1) echo 'refusing live display'; exit 2;; esac
[ ! -e "/tmp/.X11-unix/X${N#:}" ] || { echo 'display occupied'; exit 2; }
export CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}
export RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}
T=$(mktemp -d /tmp/declock-perf.XXXXXX)
cleanup() { kill "$XVFB" 2>/dev/null || true; wait "$XVFB" 2>/dev/null || true; rm -rf "$T"; }
Xvfb "$N" -screen 0 3440x1440x24 -nolisten tcp -ac >"$T/xvfb.log" 2>&1 & XVFB=$!
trap cleanup EXIT
export DISPLAY="$N" DECLOCK_PERF_PRIVATE=1
unset WAYLAND_DISPLAY XAUTHORITY
export HOME="$T/home" XDG_CONFIG_HOME="$T/cfg" XDG_RUNTIME_DIR="$T/run" DECEBAL_CONFIG="$T/cfg/omarchy"
mkdir -p "$HOME" "$DECEBAL_CONFIG" "$XDG_RUNTIME_DIR";chmod 700 "$XDG_RUNTIME_DIR"
for _ in $(seq 1 60); do [ ! -e "/tmp/.X11-unix/X${N#:}" ] || break; sleep .05; done
cd "$HERE"
CARGO_BUILD_JOBS=2 cargo test --offline --lib lock::x11::tests::private_x11_render_performance -- --ignored --nocapture
