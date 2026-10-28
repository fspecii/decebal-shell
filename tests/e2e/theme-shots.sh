#!/usr/bin/env bash
# Screenshots of the bundled asset themes (docs/themes/<id>.png and themes/<id>/screenshots/main.png): private Xvfb + rdwm +
# decbar + decdock + decwidgets + a small decsettings (Theme pane), each theme applied from the repository's themes/ folder.
#   MODE=light OUT=/tmp/x tests/e2e/theme-shots.sh brutalist     (a light-variant shot; the bundled wallpaper stays the same)
#   OUT=docs/themes PROFILE=release E2E_DISPLAY=:93 tests/e2e/theme-shots.sh [id...]
# Only PIDs started here are killed; the live session is never touched.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
PROFILE=${PROFILE:-release}
BIN=${CARGO_TARGET_DIR:-$HERE/target}/$PROFILE
RDWM_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
N=${E2E_DISPLAY:-:93}
case "$N" in :0|:1) echo "refusing the live display"; exit 2 ;; esac
OUT=${OUT:-$HERE/docs/themes}; mkdir -p "$OUT"
IDS=${*:-nothing-dot nothing-light brutalist terminal-minimal terminal-amber}
PIDS=(); TMP=$(mktemp -d /tmp/decebal-shots.XXXXXX)
cleanup() { for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done; sleep 0.3; for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done; wait 2>/dev/null; rm -rf "$TMP"; }
trap cleanup EXIT
export HOME=$TMP/home XDG_CONFIG_HOME=$TMP/cfg XDG_RUNTIME_DIR=$TMP/run XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata XDG_STATE_HOME=$TMP/state
export DECEBAL_CONFIG=$TMP/cfg/omarchy DECEBAL_BACKEND=x11 DISPLAY=$N RDWMCTL=$RDWM_DIR/rdwmctl DECSETTINGS_CONFIG=$TMP/cfg/rdwm/config.toml
export DECEBAL_COLOR_SCHEME=${SCHEME:-dark} DECEBAL_SYSTEM_THEMES=$HERE/themes
unset WAYLAND_DISPLAY RDWM_SOCK
mkdir -p "$HOME" "$XDG_CONFIG_HOME/rdwm" "$DECEBAL_CONFIG" "$XDG_RUNTIME_DIR" "$XDG_DATA_HOME/applications" "$TMP/nodata"; chmod 700 "$XDG_RUNTIME_DIR"
export PATH="$RDWM_DIR:$BIN:$PATH"
printf '[general]\ngap_inner = 6\n\n[decoration]\nenabled = true\n' > "$DECSETTINGS_CONFIG"
printf '{"version":1,"pinned":["launchpad","trash"],"order":["launchpad","trash"]}\n' > "$DECEBAL_CONFIG/dock-pinned-macos.json"
Xvfb $N -screen 0 1600x900x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 & PIDS+=($!)
for i in $(seq 50); do [ -e /tmp/.X11-unix/X${N#:} ] && break; sleep 0.1; done
rdwm --backend x11 --config "$DECSETTINGS_CONFIG" >"$TMP/rdwm.log" 2>&1 & PIDS+=($!); sleep 2
decbar >"$TMP/decbar.log" 2>&1 & PIDS+=($!)
decdock >"$TMP/decdock.log" 2>&1 & PIDS+=($!)
decwidgets >"$TMP/decwidgets.log" 2>&1 & PIDS+=($!)
for id in $IDS; do
  decthemectl theme apply "$id" >/dev/null 2>&1 || { echo "apply $id failed"; continue; }
  [ -n "${MODE:-}" ] && decthemectl apply "$id" --mode "$MODE" >/dev/null 2>&1
  sleep 4
  decsettings --config "$DECSETTINGS_CONFIG" theme >"$TMP/ds.log" 2>&1 & DP=$!
  sleep 3
  W=$(xdotool search --class DecSettings 2>/dev/null | head -1)
  [ -n "$W" ] && { rdwmctl action togglefloating >/dev/null 2>&1; sleep 0.5; xdotool windowsize "$W" 700 460; xdotool windowmove "$W" 90 120; }
  sleep 2
  import -window root "$TMP/$id.png"
  kill $DP 2>/dev/null; wait $DP 2>/dev/null
  # small palette PNG: the screenshots ship in the package and the docs
  magick "$TMP/$id.png" -colors 128 -define png:compression-level=9 "$OUT/$id.png"
  echo "$id: $(stat -c %s "$OUT/$id.png") bytes"
done
