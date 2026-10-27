#!/usr/bin/env bash
# The vertical Caelestia bar end to end: the real macbar and macdock on a private Xvfb with a private rdwm and fake system tools.
#   tests/e2e/vbar.sh                  (E2E_DISPLAY=:97 by default; SHOTS=dir keeps the screenshots; PROFILE=debug uses debug binaries)
# It checks that `macthemectl style caelestia` moves the bar to the left edge, that the strut is published and rdwm tiles
# windows beside it, the workspace pills (click, wheel), the hover popups, the clock / OS icon / power button launching
# macdash / macspot / macsession, that the dock hides and comes back, and that macos / windows / caelestia switch cleanly
# both ways at runtime, with a screenshot of each stage.
# Everything is private: own HOME / XDG dirs, a private Xvfb, rdwm's own socket in the private XDG_RUNTIME_DIR, tools on PATH
# are the fakes of tests/fakebin; only PIDs started here are killed. It refuses the live displays.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
PROFILE=${PROFILE:-release}
BIN=${CARGO_TARGET_DIR:-$HERE/target}/$PROFILE
RDWM_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
[ -x "$RDWM_DIR/rdwm" ] || { echo "rdwm not found (set RDWM_BIN_DIR)"; exit 77; }
for t in Xvfb xdotool import xprop jq convert; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline $([ "$PROFILE" = release ] && echo --release) --bins --examples) || { echo "build failed"; exit 2; }
N=${E2E_DISPLAY:-:97}
case "$N" in :0 | :1 | :2 | 0 | 1 | 2) echo "refusing a live display"; exit 2 ;; esac

PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d /tmp/macshell-vbar-e2e.XXXXXX)
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
export MACSHELL_CONFIG=$TMP/cfg/omarchy MACSHELL_BACKEND=x11 DISPLAY=$N RDWMCTL=$RDWM_DIR/rdwmctl MACSETTINGS_CONFIG=$TMP/cfg/rdwm/config.toml
export MACSHELL_COLOR_SCHEME=dark MACSHELL_THEMES=$TMP/cfg/macshell/themes
unset WAYLAND_DISPLAY RDWM_SOCK HYPRLAND_INSTANCE_SIGNATURE
mkdir -p "$HOME" "$XDG_CONFIG_HOME/rdwm" "$MACSHELL_CONFIG" "$XDG_RUNTIME_DIR" "$XDG_DATA_HOME/applications" "$TMP/nodata" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$TMP/ps/BAT0"
chmod 700 "$XDG_RUNTIME_DIR"
# fake system tools (volume, Wi-Fi, Bluetooth, brightness) and a fake battery
export FAKE_STATE=$TMP/fs MACBAR_POWER_SUPPLY=$TMP/ps
mkdir -p "$FAKE_STATE"
echo 0.50 > "$FAKE_STATE/vol"; echo 0 > "$FAKE_STATE/mute"; echo enabled > "$FAKE_STATE/wifi"; echo yes > "$FAKE_STATE/btpower"; echo 50 > "$FAKE_STATE/bright"; : > "$FAKE_STATE/log"
echo Battery > "$TMP/ps/BAT0/type"; echo 64 > "$TMP/ps/BAT0/capacity"; echo Discharging > "$TMP/ps/BAT0/status"
export PATH=$HERE/tests/fakebin:$RDWM_DIR:$BIN:$PATH
CFG=$MACSETTINGS_CONFIG
printf '# test config\n[general]\ngap_inner = 6\n\n[decoration]\nenabled = true\n\n[bar]\nshow = false\n' > "$CFG"
FAKE=$BIN/examples/fakewin
printf '{"version":1,"pinned":["launchpad","trash"],"order":["launchpad","trash"]}\n' > "$MACSHELL_CONFIG/dock-pinned-macos.json"

SW=1600; SH=900
Xvfb $N -screen 0 ${SW}x${SH}x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X${N#:} || { echo "Xvfb failed"; cat "$TMP/xvfb.log"; exit 2; }
xdotool mousemove 800 450
rdwm --backend x11 --config "$CFG" >"$TMP/rdwm.log" 2>&1 & PIDS+=($!)
wait_for 5 bash -c "xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window" || { echo "rdwm did not start"; cat "$TMP/rdwm.log"; exit 2; }
rdwmctl wallpaper --color "#46607a" >/dev/null 2>&1
printf '{"workspaces": true}\n' > "$MACSHELL_CONFIG/macshell.json"

shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
px() { import -window root -crop 1x1+$1+$2 +repage -depth 8 txt:- 2>/dev/null | tail -1 | grep -o '#[0-9A-F]\{6\}' | head -1 | tr 'A-F' 'a-f'; }
# eqpx DESC X Y WANT: the pixel must reach WANT within 6 s
eqpx() { local end=$((SECONDS + 6)) got; while [ $SECONDS -lt $end ]; do got=$(px "$2" "$3"); [ "$got" = "$4" ] && { pass "$1"; return; }; sleep 0.3; done; fail "$1" "got '$got', want '$4'"; }
nepx() { local end=$((SECONDS + 6)) got; while [ $SECONDS -lt $end ]; do got=$(px "$2" "$3"); [ "$got" != "$4" ] && [ -n "$got" ] && { pass "$1 ($got)"; return; }; sleep 0.3; done; fail "$1" "still '$got'"; }
style() { macthemectl style "$1" >/dev/null 2>&1; }
color() { macthemectl get --json | jq -r ".colors.$1"; }

bar_win() { xdotool search --name '^macshell-bar$' 2>/dev/null | head -1; }
dock_visible() { xdotool search --onlyvisible --name '^macshell-dock$' 2>/dev/null | head -1 | grep -q .; }
bar_visible() { xdotool search --onlyvisible --name '^macshell-bar$' 2>/dev/null | head -1 | grep -q .; }
dock_gone() { ! dock_visible; }
bar_gone() { ! bar_visible; }
view_is() { [ "$(view_mask)" = "$1" ]; }
# geom: "X Y W H" of the bar window
geom() { local w; w=$(bar_win); [ -n "$w" ] && xdotool getwindowgeometry --shell "$w" 2>/dev/null | awk -F= '/^(X|Y|WIDTH|HEIGHT)=/ {printf "%s ", $2}' | sed 's/ $//'; }
strut() { local w; w=$(bar_win); [ -n "$w" ] && xprop -id "$w" _NET_WM_STRUT_PARTIAL 2>/dev/null | sed 's/.*= //; s/,//g'; }
geom_is() { [ "$(geom)" = "$1" ]; }
strut_is() { [ "$(strut)" = "$1" ]; }
# tray_xy KIND: centre of an item as MACBAR_DEBUG_TRAY writes it ("Kind X Y")
tray_xy() { awk -v k="$1" '$1 == k {print $2, $3}' "$TMP/tray" 2>/dev/null | head -1; }
view_mask() { rdwmctl state 2>/dev/null | jq -r '.state.monitors[0].view'; }
macwin() { xdotool search --name "^$1\$" 2>/dev/null | head -1 | grep -q .; }

# a window on each of two tags, so there is an occupied tag besides the active one
"$FAKE" Alpha "Alpha Window" cc3333 >/dev/null 2>&1 & PIDS+=($!)
sleep 1

echo "== macOS style: the horizontal bar and the dock"
style macos
MACBAR_DEBUG_TRAY=$TMP/tray macbar >"$TMP/macbar.log" 2>&1 & BARPID=$!; PIDS+=($BARPID)
macdock >"$TMP/macdock.log" 2>&1 & DOCKPID=$!; PIDS+=($DOCKPID)
until_ok "the bar window is on the top edge, full width" 8 geom_is "0 0 $SW 420"
eq "its strut is a top strut of 26" "$(strut)" "0 0 26 0 0 0 0 0 0 1599 0 0"
until_ok "the dock is on screen" 8 dock_visible
shot 10-macos

echo "== Caelestia style: the vertical bar"
style caelestia
eq "style reads back" "$(macthemectl style)" caelestia
eq "the theme asks for a vertical bar" "$(macthemectl get --json | jq -r .bar_style)" vertical
eq "dock.enabled is auto" "$(jq -r .dock.enabled "$MACSHELL_CONFIG/macshell.json")" auto
until_ok "the bar window moved to the left edge: x 0, y 0, 444 wide, as tall as the screen" 8 geom_is "0 0 444 $SH"
eq "its strut is a left strut of 52 over every row (and nothing on top)" "$(strut)" "52 0 0 0 0 $((SH - 1)) 0 0 0 0 0 0"
BG=$(color background)
eqpx "the strip is painted in the theme background" 3 450 "$BG"
nepx "the room right of the strip is not (the wallpaper shows)" 300 450 "$BG"
until_ok "the dock hid itself" 10 dock_gone
kill -0 $DOCKPID 2>/dev/null && pass "macdock is still running (the supervisor would restart it otherwise)" || fail "macdock exited"
sleep 1
shot 20-caelestia

# rdwm tiles beside the bar: a new window starts right of the strut
"$FAKE" Beta "Beta Window" 33cc66 >/dev/null 2>&1 & PIDS+=($!)
sleep 1.5
BW=$(xdotool search --name '^Beta Window$' 2>/dev/null | head -1)
BX=$(xdotool getwindowgeometry --shell "$BW" 2>/dev/null | awk -F= '/^X=/ {print $2}')
[ -n "$BX" ] && [ "$BX" -ge 52 ] && pass "rdwm tiles windows right of the 52 px strut (x = $BX)" || fail "a window overlaps the bar" "x=$BX"
shot 21-caelestia-windows

echo "== workspace pills"
[ -s "$TMP/tray" ] && pass "the bar reports its items" || fail "no item list from MACBAR_DEBUG_TRAY"
read -r WX WY <<< "$(tray_xy Workspaces)"
# nine tags: the pill is 9 * 26 + 12 = 246 tall, cell i is centred 6 + 26 * (i + 0.5) below its top
cell_y() { echo $((WY - 123 + 6 + 26 * $1 + 13)); }
eq "tag 1 is in view" "$(view_mask)" 1
xdotool mousemove "$WX" "$(cell_y 2)" click 1
until_ok "clicking pill 3 views tag 3 (rdwm view mask 4)" 5 view_is 4
sleep 0.5
shot 30-workspace-3
xdotool mousemove "$WX" "$(cell_y 0)" click 1
until_ok "clicking pill 1 goes back" 5 view_is 1
xdotool mousemove "$WX" "$(cell_y 4)" click 5
until_ok "the wheel over the pills steps to the next tag" 5 view_is 2
xdotool click 4
until_ok "...and back" 5 view_is 1

echo "== hover popups"
read -r AX AY <<< "$(tray_xy Audio)"
[ -n "$AX" ] && pass "the sound icon is on the bar ($AX,$AY)" || fail "no sound icon (fake wpctl not found?)"
UNDER_A=$(px 62 "$AY")
xdotool mousemove 26 "$AY"; sleep 0.1; xdotool mousemove 27 "$AY"; sleep 0.8
nepx "hovering the sound icon opens its popup right of the bar" 62 "$AY" "$UNDER_A"
shot 40-audio-popup
read -r NX NY <<< "$(tray_xy Network)"
xdotool mousemove 800 450; sleep 0.9
UNDER_N=$(px 62 "$NY")
xdotool mousemove "$NX" "$NY"; sleep 0.8
shot 41-network-popup
nepx "sliding to the Wi-Fi icon shows its popup beside it" 62 "$NY" "$UNDER_N"
xdotool mousemove 800 450; sleep 0.9
eqpx "moving away closes it" 62 "$NY" "$UNDER_N"
# wheel over the sound icon changes the volume through the fake wpctl
xdotool mousemove "$AX" "$AY"; sleep 0.2; xdotool click 4 click 4; sleep 0.8
eq "wheel up over the sound icon raises the volume" "$(cat "$FAKE_STATE/vol")" 0.60
xdotool mousemove 800 450; sleep 0.6

echo "== widgets"
read -r LX LY <<< "$(tray_xy Logo)"
UNDER_L=$(px 1100 212)
xdotool mousemove "$LX" "$LY" click 1
sleep 0.8
nepx "the OS icon opens the launcher (macspot's search pill appears)" 1100 212 "$UNDER_L"
shot 50-launcher
xdotool key Escape; sleep 0.5
read -r CX CY <<< "$(tray_xy Clock)"
xdotool mousemove "$CX" "$CY" click 1
until_ok "the clock opens the dashboard (macdash)" 6 macwin macdash
sleep 1; shot 51-dashboard
xdotool key Escape; sleep 0.6
read -r PX PY <<< "$(tray_xy Power)"
xdotool mousemove "$PX" "$PY" click 1
until_ok "the power button opens the session menu (macsession)" 5 macwin macsession
shot 52-session
xdotool key Escape; sleep 0.6
read -r TX TY <<< "$(tray_xy Control)"
UNDER_T=$(px 200 "$TY")
xdotool mousemove "$TX" "$TY" click 1; sleep 0.8
nepx "the quick-settings tile opens the Control Center beside the bar" 200 "$TY" "$UNDER_T"
shot 53-control-center
xdotool key Escape; sleep 0.4

echo "== back to macOS"
style macos
until_ok "the bar is on the top edge again" 8 geom_is "0 0 $SW 420"
eq "top strut again, no left strut" "$(strut)" "0 0 26 0 0 0 0 0 0 1599 0 0"
until_ok "the dock is back" 10 dock_visible
eq "dock.enabled is true again" "$(jq -r .dock.enabled "$MACSHELL_CONFIG/macshell.json")" true
shot 60-macos-again

echo "== Windows style"
style windows
until_ok "the menu bar is hidden" 8 bar_gone
shot 70-windows

echo "== and Caelestia again from Windows, then macOS"
style caelestia
until_ok "the vertical bar returns" 10 geom_is "0 0 444 $SH"
until_ok "the dock hides again" 10 dock_gone
shot 80-caelestia-again
style macos
until_ok "the horizontal bar returns" 10 geom_is "0 0 $SW 420"
until_ok "and the dock" 10 dock_visible

echo "== no crashes"
for p in $BARPID $DOCKPID; do kill -0 "$p" 2>/dev/null || fail "process $p died"; done
grep -il "panicked" "$TMP"/*.log 2>/dev/null | head -3 | while read -r f; do fail "panic in $f"; done
echo; echo "passed $PASS, failed $FAIL (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
