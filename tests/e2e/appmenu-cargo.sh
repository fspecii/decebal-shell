#!/usr/bin/env bash
# Runs cargo for the app-menu lane inside a private environment: fresh HOME/XDG dirs, no host
# DISPLAY/WAYLAND/XAUTHORITY/rdwm/session variables, an invalid session bus address, a recorder
# PATH entry first (any accidental spawn of a desktop helper is logged, not executed), and the
# shared build slot taken with `flock --close` so no spawned helper inherits the lock fd.
#   tests/e2e/appmenu-cargo.sh test --lib appmenu
set -euo pipefail
REPO=$(cd -- "$(dirname -- "$0")/../.." && pwd)
ROOT=$(mktemp -d "${TMPDIR:-/var/tmp}/decebal-appmenu-env.XXXXXX")
trap 'rm -rf "$ROOT"' EXIT
mkdir -p "$ROOT/home" "$ROOT/config" "$ROOT/cache" "$ROOT/state" "$ROOT/data" "$ROOT/runtime" "$ROOT/recorder"
chmod 700 "$ROOT/runtime"
for tool in xdg-open notify-send systemctl loginctl hyprctl omarchy decsettings decsession mac-menu-action nmcli wpctl pactl brightnessctl; do
  printf '#!/bin/sh\necho "%s $*" >> "%s/recorder.log"\n' "$tool" "$ROOT" > "$ROOT/recorder/$tool"
  chmod +x "$ROOT/recorder/$tool"
done
CARGO_BIN=$(command -v cargo)
CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}
RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}
TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/decebal-appmenu-target}
LOCK=${DECEBAL_BUILD_SLOT:-$HOME/.cache/decebal-build-slots/slot2.lock}
STATUS=0
env -i \
  PATH="$ROOT/recorder:$(dirname "$CARGO_BIN"):/usr/local/bin:/usr/bin:/bin" \
  HOME="$ROOT/home" XDG_CONFIG_HOME="$ROOT/config" XDG_CACHE_HOME="$ROOT/cache" XDG_STATE_HOME="$ROOT/state" \
  XDG_DATA_HOME="$ROOT/data" XDG_RUNTIME_DIR="$ROOT/runtime" DECEBAL_CONFIG="$ROOT/config/omarchy" \
  DBUS_SESSION_BUS_ADDRESS="unix:path=$ROOT/runtime/no-such-bus" \
  CARGO_HOME="$CARGO_HOME" RUSTUP_HOME="$RUSTUP_HOME" CARGO_TARGET_DIR="$TARGET" TMPDIR=/var/tmp LANG=C.UTF-8 \
  ${DECEBAL_APPMENU_EXTRA_ENV:-} \
  flock --close "$LOCK" nice -n10 "$CARGO_BIN" "$@" || STATUS=$?
if [[ -s "$ROOT/recorder.log" ]]; then
  echo "appmenu-cargo: recorder caught spawns:" >&2
  cat "$ROOT/recorder.log" >&2
fi
exit "$STATUS"
