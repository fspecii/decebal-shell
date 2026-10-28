#!/usr/bin/env bash
# Shell styles end to end: the real decbar and decdock under the macOS, Windows and Caelestia styles on a private
# Xvfb + rdwm. `decthemectl style X` switches the running programs; pixels of the bar and the dock are checked per style,
# the Caelestia bar (a vertical strip with pills, the dock hidden; tests/e2e/vbar.sh covers it in depth), wallpaper colours (`decthemectl dynamic`), and that the macOS style is
# pixel-identical to the binaries built before the Caelestia theme existed (BASELINE_BIN=dir with those binaries; skipped
# when unset).
#   tests/e2e/style.sh                  (SHOTS=dir keeps the PNGs; E2E_DISPLAY=:137 by default)
# Everything is private: own HOME/XDG dirs, only PIDs started here are killed, never the live display.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
BIN=${CARGO_TARGET_DIR:-$HERE/target}/release
RDWM_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
[ -x "$RDWM_DIR/rdwm" ] || { echo "rdwm not found (set RDWM_BIN_DIR)"; exit 77; }
for t in Xvfb xdotool import jq convert compare; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline --release --bins --examples) || { echo "build failed"; exit 2; }
N=${E2E_DISPLAY:-:137}
case "$N" in :1|:2|1|2|:0|0) echo "refusing a live display"; exit 2;; esac

PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d /tmp/decebal-style-e2e.XXXXXX)
SHOTS=${SHOTS:-$TMP/shots}; mkdir -p "$SHOTS"
cleanup() { for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done; sleep 0.3; for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done; wait 2>/dev/null
  if [ "$SHOTS" = "$TMP/shots" ]; then rm -rf "$TMP"; else find "$TMP" -mindepth 1 -maxdepth 1 ! -path "$SHOTS" -exec rm -rf {} +; rmdir "$TMP" 2>/dev/null; fi; }
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "got '$2', want '$3'"; fi; }
ne() { if [ "$2" != "$3" ]; then pass "$1"; else fail "$1" "both '$2'"; fi; }
wait_for() { local t=$1; shift; local end=$(($(date +%s%N) / 1000000 + t * 1000)); while [ $(($(date +%s%N) / 1000000)) -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 0.05; done; return 1; }

export HOME=$TMP/home XDG_CONFIG_HOME=$TMP/cfg XDG_RUNTIME_DIR=$TMP/run XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata
export DECEBAL_CONFIG=$TMP/cfg/omarchy DECEBAL_BACKEND=x11 DISPLAY=$N RDWMCTL=$RDWM_DIR/rdwmctl DECSETTINGS_CONFIG=$TMP/cfg/rdwm/config.toml
export DECEBAL_COLOR_SCHEME=dark DECEBAL_THEMES=$TMP/cfg/decebal/themes
unset WAYLAND_DISPLAY RDWM_SOCK
mkdir -p "$HOME" "$XDG_CONFIG_HOME/rdwm" "$DECEBAL_CONFIG" "$XDG_RUNTIME_DIR" "$XDG_DATA_HOME/applications" "$TMP/nodata"
chmod 700 "$XDG_RUNTIME_DIR"
export PATH="$RDWM_DIR:$BIN:$PATH"
CFG=$DECSETTINGS_CONFIG
printf '# test config\n[general]\ngap_inner = 6\n\n[decoration]\nenabled = true\n\n[bar]\nshow = false\n' > "$CFG"
FAKE=$BIN/examples/fakewin
for a in Alpha:cc3333 Beta:33cc66; do n=${a%%:*}; c=${a##*:}; printf '[Desktop Entry]\nType=Application\nName=%s\nExec=%s %s "%s Window" %s\nStartupWMClass=%s\n' "$n" "$FAKE" "$n" "$n" "$c" "$n" > "$XDG_DATA_HOME/applications/$n.desktop"; done
printf '{"version":1,"pinned":["launchpad","Alpha","Beta","trash"],"order":["launchpad","Alpha","Beta","trash"]}\n' > "$DECEBAL_CONFIG/dock-pinned-macos.json"

Xvfb $N -screen 0 1600x1000x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X${N#:} || { echo "Xvfb failed"; exit 2; }
xdotool mousemove 800 500
rdwm --backend x11 --config "$CFG" >"$TMP/rdwm.log" 2>&1 & PIDS+=($!)
wait_for 5 bash -c "xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window" || { echo "rdwm did not start"; cat "$TMP/rdwm.log"; exit 2; }
rdwmctl wallpaper --color "#46607a" >/dev/null 2>&1
printf '{"workspaces": false}\n' > "$DECEBAL_CONFIG/decebal.json"

shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
px() { import -window root -crop 1x1+$1+$2 +repage -depth 8 txt:- 2>/dev/null | tail -1 | grep -o '#[0-9A-F]\{6\}' | head -1 | tr 'A-F' 'a-f'; }
settle() { sleep 1.3; }
# eqpx DESC X Y WANT: the pixel must reach WANT within 6 s (programs poll the theme every 500 ms)
eqpx() { local end=$((SECONDS + 6)) got; while [ $SECONDS -lt $end ]; do got=$(px "$2" "$3"); [ "$got" = "$4" ] && { pass "$1"; return; }; sleep 0.3; done; fail "$1" "got '$got', want '$4'"; }
# nepx DESC X Y NOT: the pixel must settle on something else than NOT
nepx() { local end=$((SECONDS + 6)) got; while [ $SECONDS -lt $end ]; do got=$(px "$2" "$3"); [ "$got" != "$4" ] && [ -n "$got" ] && { pass "$1 ($got)"; return; }; sleep 0.3; done; fail "$1" "still '$got'"; }
style() { decthemectl style "$1" >/dev/null 2>&1; }
# region screenshots for pixel-exact comparison: the bar without the clock, the dock panel below the window area
bar_png() { import -window root -crop 1000x26+0+0 +repage "$1" 2>/dev/null; }
dock_png() { import -window root -crop 760x78+420+922 +repage "$1" 2>/dev/null; }
same() { local d; d=$(compare -metric AE "$2" "$3" "$SHOTS/diff-$(basename "$3")" 2>&1 | awk '{print $1}'); if [ "$d" = "0" ]; then pass "$1"; else fail "$1" "$d pixels differ"; fi; }

start_shell() { # $1 = directory with decbar/decdock
  "$1/decbar" >"$TMP/decbar.log" 2>&1 & BARPID=$!; PIDS+=($BARPID)
  "$1/decdock" >"$TMP/decdock.log" 2>&1 & DOCKPID=$!; PIDS+=($DOCKPID)
  sleep 2.5
}
stop_shell() { kill $BARPID $DOCKPID 2>/dev/null; wait $BARPID $DOCKPID 2>/dev/null; sleep 0.5; }

# the two fake apps run in every phase, so the dock shows running dots and the bar names the focused app the same way
"$FAKE" Alpha "Alpha Window" cc3333 >/dev/null 2>&1 & PIDS+=($!)
"$FAKE" Beta "Beta Window" 33cc66 >/dev/null 2>&1 & PIDS+=($!)
sleep 1.5

if [ -n "${BASELINE_BIN:-}" ] && [ -x "$BASELINE_BIN/decbar" ]; then
  echo "== baseline (binaries from before the Caelestia theme)"
  start_shell "$BASELINE_BIN"
  bar_png "$TMP/base-bar.png"; dock_png "$TMP/base-dock.png"; shot 00-baseline-macos
  stop_shell
else
  echo "== (no BASELINE_BIN: skipping the before/after comparison)"
fi

start_shell "$BIN"

echo "== macOS style (the default look)"
eq "with nothing configured the style is macOS" "$(decthemectl style)" macos
style macos; settle
eq "style reads back" "$(decthemectl style)" macos
eqpx "macOS bar strip colour" 900 3 "#131313"
eqpx "macOS bar reaches the screen corner" 1 1 "#131313"
MAC_DOCK=$(px 760 975); MAC_BAR=$(px 900 13)
shot 10-macos
if [ -n "${BASELINE_BIN:-}" ]; then
  bar_png "$TMP/new-bar.png"; dock_png "$TMP/new-dock.png"
  same "macOS bar pixel-identical to the pre-Caelestia build" "$TMP/base-bar.png" "$TMP/new-bar.png"
  same "macOS dock pixel-identical to the pre-Caelestia build" "$TMP/base-dock.png" "$TMP/new-dock.png"
fi

echo "== Caelestia style"
style caelestia; settle
eq "style reads back" "$(decthemectl style)" caelestia
eq "theme is caelestia" "$(decthemectl get --json | jq -r .theme)" caelestia
eq "bar style is vertical" "$(decthemectl get --json | jq -r .bar_style)" vertical
# the vertical bar: the OS icon's pill (a circle) is the surface container, the strip around it the theme background
eqpx "Caelestia bar pill is the surface container" 26 12 "#261d20"
nepx "...and the strip corner is outside the pill" 2 12 "#261d20"
nepx "...the top of the old horizontal bar is empty (wallpaper)" 900 13 "$MAC_BAR"
eqpx "the room right of the strip belongs to the windows" 900 13 "#46607a"
wait_for 10 bash -c '! xdotool search --onlyvisible --name "^decebal-dock\$" 2>/dev/null | grep -q .' && pass "Caelestia has no dock: it hid itself" || fail "the dock is still on screen"
eq "dock-settings has no roundness override (the pill is the theme's)" "$(jq -r '.roundness // "none"' "$DECEBAL_CONFIG/dock-settings.json")" none
shot 20-caelestia-dark

decthemectl apply caelestia --mode light >/dev/null; settle
eqpx "Caelestia light bar" 26 12 "#f8eaee"
shot 21-caelestia-light
decthemectl apply caelestia --mode dark >/dev/null; settle

echo "== Windows style"
style windows; settle
eq "style reads back" "$(decthemectl style)" windows
eq "dock style is windows" "$(jq -r .style "$DECEBAL_CONFIG/dock-settings.json")" windows
eq "bar enabled auto" "$(jq -r .bar.enabled "$DECEBAL_CONFIG/decebal.json")" auto
nepx "Windows: the menu bar is gone (no #1c1c1c strip)" 900 13 "#1c1c1c"
nepx "Windows: ...and not the macOS one either" 900 13 "#131313"
nepx "Windows taskbar colour differs from the macOS dock" 300 985 "$MAC_DOCK"
shot 30-windows

echo "== back to macOS"
style macos; settle
eqpx "macOS bar strip again" 900 3 "#131313"
eq "dock style mac" "$(jq -r .style "$DECEBAL_CONFIG/dock-settings.json")" mac
if [ -n "${BASELINE_BIN:-}" ]; then
  bar_png "$TMP/again-bar.png"; dock_png "$TMP/again-dock.png"
  same "macOS bar identical after a round trip through the other styles" "$TMP/base-bar.png" "$TMP/again-bar.png"
  same "macOS dock identical after a round trip" "$TMP/base-dock.png" "$TMP/again-dock.png"
fi

echo "== colours from wallpaper"
convert -size 400x300 xc:'#2a9d8f' "$TMP/teal.png"
convert -size 400x300 xc:'#e63946' "$TMP/red.png"
barcol() { decthemectl get --json | jq -r .colors.bar; }
style caelestia; settle
BAR_PLAIN=$(barcol)
eqpx "plain Caelestia bar" 26 12 "$BAR_PLAIN"
decthemectl dynamic --wallpaper "$TMP/teal.png" --dark | grep -q "seed #2a9d8f" && pass "dynamic: seed taken from the teal wallpaper" || fail "dynamic seed"
TEAL_BAR=$(barcol)
ne "teal palette differs from the plain Caelestia surface" "$TEAL_BAR" "$BAR_PLAIN"
eqpx "the running bar follows the wallpaper (teal)" 26 12 "$TEAL_BAR"
eq "selection stores the seed" "$(jq -r .theme.dynamic.seed "$DECEBAL_CONFIG/decebal.json")" "#2a9d8f"
shot 40-dynamic-teal
decthemectl dynamic --wallpaper "$TMP/red.png" --scheme vibrant >/dev/null
RED_BAR=$(barcol)
ne "a red wallpaper gives another bar colour" "$RED_BAR" "$TEAL_BAR"
eqpx "the running bar follows (red, vibrant)" 26 12 "$RED_BAR"
shot 41-dynamic-red-vibrant
decthemectl dynamic --wallpaper "$TMP/teal.png" --light >/dev/null
LIGHT_BAR=$(barcol)
eqpx "light wallpaper palette: the running bar follows" 26 12 "$LIGHT_BAR"
eq "...and it is a light colour" "$([ $((16#${LIGHT_BAR:1:2})) -gt 200 ] && echo light)" light
shot 42-dynamic-teal-light
decthemectl dynamic off >/dev/null; decthemectl apply caelestia --mode dark >/dev/null
eqpx "dynamic off: Caelestia's own surface again" 26 12 "#261d20"
decthemectl dynamic --wallpaper "$TMP/nope.png" >/dev/null 2>&1 && fail "missing wallpaper accepted" || pass "missing wallpaper refused"
decthemectl dynamic --scheme bogus --wallpaper "$TMP/red.png" >/dev/null 2>&1 && fail "bad scheme accepted" || pass "bad scheme refused"
decthemectl style nonsense >/dev/null 2>&1 && fail "bad style accepted" || pass "bad style refused"
style macos; settle

echo "== no crashes"
for p in $BARPID $DOCKPID; do kill -0 "$p" 2>/dev/null || fail "process $p died"; done
grep -il "panicked" "$TMP"/*.log 2>/dev/null | head -3 | while read -r f; do fail "panic in $f"; done
echo; echo "passed $PASS, failed $FAIL (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
