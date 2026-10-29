#!/usr/bin/env bash
# This desktop test must not retint apps through an inherited session bus.
export DECEBAL_SYSTEM_AUTOMATIC=0
# Five original design packages on a private Xvfb/HOME: validate, apply, verify
# selection and wallpaper assets, then capture dark/light Settings and a shared
# desktop clock. Set SHOTS to retain screenshots; NO_BUILD=1 uses existing binaries.
# Live displays and occupied displays are refused.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
PROFILE=${PROFILE:-debug}
BIN=${CARGO_TARGET_DIR:-$HERE/target}/$PROFILE
RDWM_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
[ -x "$RDWM_DIR/rdwm" ] || { echo "rdwm not found (set RDWM_BIN_DIR)"; exit 77; }
for t in Xvfb xdotool import jq convert; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
if [ "${NO_BUILD:-0}" = 1 ]; then :
elif [ "$PROFILE" = release ]; then (cd "$HERE" && cargo build -j2 -q --offline --release --bins --examples) || { echo "build failed"; exit 2; }
else (cd "$HERE" && cargo build -j2 -q --offline --bins --examples) || { echo "build failed"; exit 2; }; fi
N=${E2E_DISPLAY:-:87}
case "$N" in :0|:1) echo "refusing the live display"; exit 2;; esac
[ ! -e "/tmp/.X11-unix/X${N#:}" ] || { echo "display already occupied"; exit 2; }

IDS="orbit-glass paper-signal moss-atlas signal-deck nextkde"
PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d "${TMPDIR:-/tmp}/decebal-themes-e2e.XXXXXX")
SHOTS=${SHOTS:-$TMP/shots}; mkdir -p "$SHOTS"
cleanup() { for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done; sleep 0.3; for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done; wait 2>/dev/null
  [ "$SHOTS" = "$TMP/shots" ] && rm -rf "$TMP" || find "$TMP" -mindepth 1 -maxdepth 1 ! -path "$SHOTS" -exec rm -rf {} +; }
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "got '$2', want '$3'"; fi; }
has() { if printf '%s' "$2" | grep -q -- "$3"; then pass "$1"; else fail "$1" "no '$3' in: $(printf '%s' "$2" | head -c 300)"; fi; }
wait_for() { local t=$1; shift; local end=$(($(date +%s%N) / 1000000 + t * 1000)); while [ $(($(date +%s%N) / 1000000)) -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 0.05; done; return 1; }

export HOME=$TMP/home XDG_CONFIG_HOME=$TMP/cfg XDG_RUNTIME_DIR=$TMP/run XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata XDG_STATE_HOME=$TMP/state XDG_CACHE_HOME=$TMP/cache
export DECEBAL_CONFIG=$TMP/cfg/omarchy DECEBAL_BACKEND=x11 DISPLAY=$N RDWMCTL=$RDWM_DIR/rdwmctl DECSETTINGS_CONFIG=$TMP/cfg/rdwm/config.toml
export DECEBAL_COLOR_SCHEME=dark DECEBAL_SYSTEM_THEMES=$TMP/system-themes
unset WAYLAND_DISPLAY RDWM_SOCK
mkdir -p "$HOME" "$XDG_CONFIG_HOME/rdwm" "$DECEBAL_CONFIG" "$XDG_RUNTIME_DIR" "$XDG_DATA_HOME/applications" "$TMP/nodata" "$DECEBAL_SYSTEM_THEMES"
chmod 700 "$XDG_RUNTIME_DIR"
export PATH="$RDWM_DIR:$BIN:$PATH"
CFG=$DECSETTINGS_CONFIG
# what `make install` does: the packages, with their wallpapers, in the system theme folder
for id in $IDS; do cp -r "$HERE/themes/$id" "$DECEBAL_SYSTEM_THEMES/$id"; done
printf '# test config\n[general]\ngap_inner = 6\n\n[decoration]\nenabled = true\n' > "$CFG"
printf '{"version":1,"pinned":["launchpad","trash"],"order":["launchpad","trash"]}\n' > "$DECEBAL_CONFIG/dock-pinned-macos.json"

Xvfb $N -screen 0 1600x900x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X${N#:} || { echo "Xvfb failed"; exit 2; }
xdotool mousemove 800 450
rdwm --backend x11 --config "$CFG" >"$TMP/rdwm.log" 2>&1 & PIDS+=($!)
wait_for 5 bash -c "xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window" || { echo "rdwm did not start"; cat "$TMP/rdwm.log"; exit 2; }
decbar >"$TMP/decbar.log" 2>&1 & PIDS+=($!)
decdock >"$TMP/decdock.log" 2>&1 & PIDS+=($!)
sleep 2
decsettings --config "$CFG" theme >"$TMP/decsettings.log" 2>&1 & PIDS+=($!)
sleep 2
settings_window=$(xdotool search --class decsettings 2>/dev/null | head -1)
if [ -n "$settings_window" ]; then
  xdotool windowactivate --sync "$settings_window"
  rdwmctl action togglefloating >/dev/null
  xdotool windowsize "$settings_window" 980 680 windowmove "$settings_window" 50 90
fi
decwidgets add builtin:next-clock --anchor top-right --x -35 --y 80 >/dev/null 2>&1
# The host is started by the first add; keep its PID for private cleanup.
wpid=$(cat "$XDG_RUNTIME_DIR/decwidgets.lock" 2>/dev/null || true)
[ -z "$wpid" ] || PIDS+=("$wpid")

# a value of rdwm's config.toml: tomlv SECTION KEY
tomlv() { awk -v sec="[$1]" -v key="$2" '/^\[/ { in_s = ($0 == sec) } in_s && $1 == key { sub(/^[^=]*= *"?/, ""); sub(/"? *(#.*)?$/, ""); print; exit }' "$CFG"; }
shell() { jq -r "$1" "$DECEBAL_CONFIG/decebal.json"; }
echo "== every package validates"
for id in $IDS; do out=$(decthemectl theme validate "$HERE/themes/$id" 2>&1); has "$id validates" "$out" "^ok:"; done
echo "== apply each: files, then a screenshot"
for id in $IDS; do
  out=$(decthemectl theme apply "$id" 2>&1); has "$id applies" "$out" "applied $id"
  sleep 2.5
  if [ -n "$settings_window" ]; then
    xdotool mousemove --window "$settings_window" 300 120
    sleep 1
  fi
  eq "$id: selected" "$(shell '.theme.name // .theme')" "$id"
  wp=$(tomlv wallpaper image); has "$id: wallpaper comes from the package" "$wp" "$id/wallpapers/"
  [ -f "$wp" ] && pass "$id: wallpaper file exists" || fail "$id: wallpaper file" "$wp"
  import -window root "$SHOTS/$id.png" 2>/dev/null
  decthemectl apply "$id" --mode light >/dev/null 2>&1; sleep 1.5
  eq "$id: light mode keeps the selection" "$(shell '.theme.name')" "$id"
  import -window root "$SHOTS/$id-light.png" 2>/dev/null
  decthemectl apply "$id" --mode dark >/dev/null 2>&1; sleep 0.5
done 2>&1 | tee "$TMP/loop.out"
PASS=$((PASS + $(grep -c '^  ok' "$TMP/loop.out"))); FAIL=$((FAIL + $(grep -c '^  FAIL' "$TMP/loop.out")))

echo
echo "$PASS passed, $FAIL failed (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
