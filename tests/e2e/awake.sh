#!/usr/bin/env bash
# This desktop test must not retint apps through an inherited session bus.
export DECEBAL_SYSTEM_AUTOMATIC=0
# The Stay Awake coffee cup of the bar end to end: the real decbar on a private Xvfb with a private rdwm, a fake `omarchy` that
# isolates unrelated Omarchy probes. Stay Awake itself uses real decutil/X11 inhibition.
#   tests/e2e/awake.sh                 (E2E_DISPLAY=:172 by default; SHOTS=dir keeps the screenshots; PROFILE=debug uses debug binaries)
# Checks: the cup shows while the indicator exists; a click on it turns Stay Awake off (indicator gone, one toggle, icon gone) on the
# macOS bar and on the Caelestia pill bar; a hover popup on the pill bar and a right-click popup on the horizontal bar say what it
# is; an expired timed session is switched off when the bar starts. Everything is private; only PIDs started here are killed.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
PROFILE=${PROFILE:-release}
BIN=${CARGO_TARGET_DIR:-$HERE/target}/$PROFILE
RDWM_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
[ -x "$RDWM_DIR/rdwm" ] || { echo "rdwm not found (set RDWM_BIN_DIR)"; exit 77; }
for t in Xvfb xdotool import xprop jq convert; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline $([ "$PROFILE" = release ] && echo --release) --bins --examples) || { echo "build failed"; exit 2; }
N=${E2E_DISPLAY:-:172}
case "$N" in :0 | :1 | :2 | 0 | 1 | 2) echo "refusing a live display"; exit 2 ;; esac

PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d /tmp/decebal-awake-e2e.XXXXXX)
SHOTS=${SHOTS:-$TMP/shots}; mkdir -p "$SHOTS"
cleanup() {
    for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done
    sleep 0.3
    for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done
    wait 2>/dev/null
    if [ "$SHOTS" = "$TMP/shots" ]; then rm -rf "$TMP"; else find "$TMP" -mindepth 1 -maxdepth 1 ! -path "$SHOTS" -exec rm -rf {} +; rmdir "$TMP" 2>/dev/null; fi
}
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "got '$2', want '$3'"; fi; }
ne() { if [ "$2" != "$3" ]; then pass "$1"; else fail "$1" "both '$2'"; fi; }
wait_for() { local t=$1; shift; local end=$(($(date +%s%N) / 1000000 + t * 1000)); while [ $(($(date +%s%N) / 1000000)) -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 0.05; done; return 1; }
# until DESC SECONDS CMD...: CMD must succeed within SECONDS
until_ok() { local d=$1 t=$2; shift 2; if wait_for "$t" "$@"; then pass "$d"; else fail "$d" "timed out after ${t}s"; fi; }

export HOME=$TMP/home XDG_CONFIG_HOME=$TMP/cfg XDG_RUNTIME_DIR=$TMP/run XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata XDG_CACHE_HOME=$TMP/cache XDG_STATE_HOME=$TMP/state
export DECEBAL_CONFIG=$TMP/cfg/omarchy DECEBAL_BACKEND=x11 DISPLAY=$N RDWMCTL=$RDWM_DIR/rdwmctl DECSETTINGS_CONFIG=$TMP/cfg/rdwm/config.toml
export DECEBAL_COLOR_SCHEME=dark DECEBAL_THEMES=$TMP/cfg/decebal/themes
unset WAYLAND_DISPLAY RDWM_SOCK HYPRLAND_INSTANCE_SIGNATURE
mkdir -p "$HOME" "$XDG_CONFIG_HOME/rdwm" "$DECEBAL_CONFIG" "$XDG_RUNTIME_DIR" "$XDG_DATA_HOME/applications" "$TMP/nodata" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$TMP/ps/BAT0"
chmod 700 "$XDG_RUNTIME_DIR"
# fake system tools (volume, Wi-Fi, Bluetooth, brightness) and a fake battery
export FAKE_STATE=$TMP/fs DECBAR_POWER_SUPPLY=$TMP/ps
mkdir -p "$FAKE_STATE"
echo 0.50 > "$FAKE_STATE/vol"; echo 0 > "$FAKE_STATE/mute"; echo enabled > "$FAKE_STATE/wifi"; echo yes > "$FAKE_STATE/btpower"; echo 50 > "$FAKE_STATE/bright"; : > "$FAKE_STATE/log"
echo Battery > "$TMP/ps/BAT0/type"; echo 64 > "$TMP/ps/BAT0/capacity"; echo Discharging > "$TMP/ps/BAT0/status"
export PATH=$HERE/tests/fakebin:$RDWM_DIR:$BIN:$PATH
CFG=$DECSETTINGS_CONFIG
printf '# test config\n[general]\ngap_inner = 6\n\n[decoration]\nenabled = true\n\n[bar]\nshow = false\n' > "$CFG"
FAKE=$BIN/examples/fakewin
printf '{"version":1,"pinned":["launchpad","trash"],"order":["launchpad","trash"]}\n' > "$DECEBAL_CONFIG/dock-pinned-macos.json"

SW=1600; SH=900
Xvfb $N -screen 0 ${SW}x${SH}x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X${N#:} || { echo "Xvfb failed"; cat "$TMP/xvfb.log"; exit 2; }
xdotool mousemove 800 450
rdwm --backend x11 --config "$CFG" >"$TMP/rdwm.log" 2>&1 & PIDS+=($!)
wait_for 5 bash -c "xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window" || { echo "rdwm did not start"; cat "$TMP/rdwm.log"; exit 2; }
rdwmctl wallpaper --color "#46607a" >/dev/null 2>&1
printf '{"workspaces": true}\n' > "$DECEBAL_CONFIG/decebal.json"

shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
px() { import -window root -crop 1x1+$1+$2 +repage -depth 8 txt:- 2>/dev/null | tail -1 | grep -o '#[0-9A-F]\{6\}' | head -1 | tr 'A-F' 'a-f'; }
# eqpx DESC X Y WANT: the pixel must reach WANT within 6 s
eqpx() { local end=$((SECONDS + 6)) got; while [ $SECONDS -lt $end ]; do got=$(px "$2" "$3"); [ "$got" = "$4" ] && { pass "$1"; return; }; sleep 0.3; done; fail "$1" "got '$got', want '$4'"; }
nepx() { local end=$((SECONDS + 6)) got; while [ $SECONDS -lt $end ]; do got=$(px "$2" "$3"); [ "$got" != "$4" ] && [ -n "$got" ] && { pass "$1 ($got)"; return; }; sleep 0.3; done; fail "$1" "still '$got'"; }
style() { decthemectl style "$1" >/dev/null 2>&1; }
color() { decthemectl get --json | jq -r ".colors.$1"; }

bar_win() { xdotool search --name '^decebal-bar$' 2>/dev/null | head -1; }
dock_visible() { xdotool search --onlyvisible --name '^decebal-dock$' 2>/dev/null | head -1 | grep -q .; }
bar_visible() { xdotool search --onlyvisible --name '^decebal-bar$' 2>/dev/null | head -1 | grep -q .; }
dock_gone() { ! dock_visible; }
bar_gone() { ! bar_visible; }

# the fake omarchy: `omarchy toggle idle` flips the indicator and is logged; anything else is ignored
mkdir -p "$TMP/fake"
cat > "$TMP/fake/omarchy" <<'FAKE'
#!/bin/bash
f=${XDG_STATE_HOME:?}/omarchy/indicators/stay-awake
if [ "$1 $2" = "toggle idle" ]; then
    if [ -e "$f" ]; then rm -f "$f"; else mkdir -p "$(dirname "$f")"; : > "$f"; fi
    echo "toggle idle" >> "${FAKE_STATE:?}/omarchy.log"
fi
exit 0
FAKE
chmod +x "$TMP/fake/omarchy"
export PATH=$TMP/fake:$PATH
IND=$XDG_STATE_HOME/omarchy/indicators/stay-awake
UNTIL=$XDG_STATE_HOME/decebal/stay-awake-until
mkdir -p "$(dirname "$IND")" "$(dirname "$UNTIL")"
toggles() { grep -c "toggle idle" "$FAKE_STATE/omarchy.log" 2>/dev/null || echo 0; }
has_tray() { grep -q "^$1 " "$TMP/tray" 2>/dev/null; }
no_tray() { ! has_tray "$1"; }
tray_xy() { awk -v k="$1" '$1 == k {print $2, $3}' "$TMP/tray" 2>/dev/null | head -1; }
indicator_gone() { [ ! -e "$IND" ]; }
shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
style() { decthemectl style "$1" >/dev/null 2>&1; }
bar_win() { xdotool search --name '^decebal-bar$' 2>/dev/null | head -1; }

echo "== an expired timed session ends when the bar starts"
: > "$IND"; echo $(( $(date +%s) - 5 )) > "$UNTIL"
style macos
DECBAR_DEBUG_TRAY=$TMP/tray decbar >"$TMP/decbar.log" 2>&1 & BARPID=$!; PIDS+=($BARPID)
until_ok "the bar switched the expired Stay Awake off" 8 indicator_gone
xset q | grep -Eq "timeout: +600" && pass "expired session leaves the idle timeout restored" || fail "idle timeout not restored"
[ ! -e "$UNTIL" ] && pass "and dropped the expiry" || fail "expiry file left"
kill $BARPID; wait $BARPID 2>/dev/null; sleep 0.3
: > "$FAKE_STATE/omarchy.log"

echo "== macOS bar: the cup"
: > "$IND"; rm -f "$UNTIL"; rm -f "$TMP/tray"
DECBAR_DEBUG_TRAY=$TMP/tray decbar >"$TMP/decbar.log" 2>&1 & BARPID=$!; PIDS+=($BARPID)
until_ok "the cup is on the bar while Stay Awake is on" 8 has_tray Awake
read -r AX AY <<< "$(tray_xy Awake)"; AY=${AY:-12}
shot 10-macos-cup
# right click: the options popup under the cup
xdotool mousemove "$AX" "$AY" click 3; sleep 0.6
shot 11-macos-options
xdotool key Escape; xdotool mousemove 800 450; sleep 0.5
read -r AX AY <<< "$(tray_xy Awake)"; AY=${AY:-12}
xdotool mousemove "$AX" "$AY" click 1
until_ok "a click on the cup turns Stay Awake off (indicator gone)" 6 indicator_gone
xset q | grep -Eq "timeout: +600" && pass "turning off restores the X11 idle timeout" || fail "X11 timeout not restored"
until_ok "and the cup disappears from the bar" 6 no_tray Awake

echo "== Caelestia pill bar: the cup"
: > "$IND"; : > "$FAKE_STATE/omarchy.log"
style caelestia
sleep 1.5
# the bar only re-reads the indicator on its minute tick or when something refreshes it: a Control Center round trip does
until_ok "the pill bar reports its items" 10 has_tray Control
read -r TX TY <<< "$(tray_xy Control)"
xdotool mousemove "$TX" "$TY" click 1; sleep 1.0; xdotool key Escape; sleep 0.5
until_ok "the cup is on the pill bar once the bar re-reads the state" 10 has_tray Awake
read -r AX AY <<< "$(tray_xy Awake)"
xdotool mousemove 800 450; sleep 0.3
UNDER=$(px 62 "$AY")
xdotool mousemove "$((AX - 1))" "$AY"; sleep 0.1; xdotool mousemove "$AX" "$AY"; sleep 0.9
nepx "hovering the cup opens the popup beside the bar" 62 "$AY" "$UNDER"
shot 20-caelestia-hover
xdotool click 1
until_ok "clicking it turns Stay Awake off" 6 indicator_gone
xset q | grep -Eq "timeout: +600" && pass "pill toggle restores the X11 idle timeout" || fail "X11 timeout not restored"
until_ok "and the cup is gone" 6 no_tray Awake
shot 21-caelestia-off

echo "== no crashes"
kill -0 "$BARPID" 2>/dev/null || fail "decbar died"
grep -il "panicked" "$TMP"/*.log 2>/dev/null | head -3 | while read -r f; do fail "panic in $f"; done
echo; echo "passed $PASS, failed $FAIL (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
