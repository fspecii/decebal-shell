#!/usr/bin/env bash
# decmon (Activity Monitor) end to end on a private Xvfb with a private rdwm, HOME and XDG dirs.
#   tests/e2e/procmon.sh               (E2E_DISPLAY=:171 by default; SHOTS=dir keeps the screenshots; PROFILE=debug uses debug binaries)
# Starts a few dummy processes of its own (symlinks of sleep / bash named dm-*: an "app" with two children, two background
# processes), opens decmon, finds them through DECMON_DUMP (the visible rows are written to a file), searches, sorts, ends one with
# the Delete key (SIGTERM) and another through the Force quit button and its confirmation (SIGKILL), and checks that the
# process died and that nothing else was touched. Then the Performance tab, and the bar's agents icon opening the popup.
# Screenshots: procmon-processes.png,, procmon-performance.png (kept in SHOTS; DOCS=dir also copies them).
# Only the dummies' exact PIDs are ever signalled by this script.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
PROFILE=${PROFILE:-release}
BIN=${CARGO_TARGET_DIR:-$HERE/target}/$PROFILE
RDWM_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
[ -x "$RDWM_DIR/rdwm" ] || { echo "rdwm not found (set RDWM_BIN_DIR)"; exit 77; }
for t in Xvfb xdotool import xprop jq convert; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline $([ "$PROFILE" = release ] && echo --release) --bins --examples) || { echo "build failed"; exit 2; }
N=${E2E_DISPLAY:-:171}
case "$N" in :0 | :1 | :2 | 0 | 1 | 2) echo "refusing a live display"; exit 2 ;; esac

PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d /tmp/decebal-procmon-e2e.XXXXXX)
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


# ------------------------------------------------------------------ dummies
mkdir -p "$TMP/bin"
SLEEP=$(command -v sleep); BASH_BIN=$(command -v bash)
ln -s "$SLEEP" "$TMP/bin/dm-bg"; ln -s "$SLEEP" "$TMP/bin/dm-bg2"; ln -s "$SLEEP" "$TMP/bin/dm-child"; ln -s "$BASH_BIN" "$TMP/bin/dm-app"
export PATH=$TMP/bin:$PATH
printf '[Desktop Entry]\nType=Application\nName=Demo App\nExec=dm-app\nIcon=utilities-terminal\n' > "$XDG_DATA_HOME/applications/dm-app.desktop"
unset XDG_DATA_DIRS
# background processes are reparented (a subshell that exits), so a killed one is reaped at once and not left as a zombie of ours
( dm-bg 900 & echo $! > "$TMP/bg.pid" ) ; ( dm-bg2 900 & echo $! > "$TMP/bg2.pid" )
dm-app -c 'dm-child 901 & dm-child 902 & wait' >/dev/null 2>&1 & APP=$!; PIDS+=($APP)
sleep 0.5
BG=$(cat "$TMP/bg.pid"); BG2=$(cat "$TMP/bg2.pid")
CHILDREN=$(pgrep -P "$APP" | tr '\n' ' ')
for p in $BG $BG2 $APP $CHILDREN; do PIDS+=($p); done
alive() { [ -r "/proc/$1/stat" ] && ! grep -q ') Z' "/proc/$1/stat" 2>/dev/null; }
dead() { ! alive "$1"; }
DUMP=$TMP/dump
dump_has() { grep -q -- "$1" "$DUMP" 2>/dev/null; }
dump_lacks() { ! dump_has "$1"; }
wid() { xdotool search --name '^Activity Monitor$' 2>/dev/null | head -1; }
have_win() { [ -n "$(wid)" ]; }
no_win() { [ -z "$(wid)" ]; }
type_text() { xdotool type --delay 40 "$1"; }
clear_search() { for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do xdotool key BackSpace; done; }
crop_shot() { # NAME: the monitor window (or the whole screen)
    local w; w=$(wid)
    if [ -n "$w" ]; then import -window "$w" "$SHOTS/$1.png" 2>/dev/null; else import -window root "$SHOTS/$1.png" 2>/dev/null; fi
}

echo "== window mode: the Processes tab"
DECMON_DUMP=$DUMP decmon >"$TMP/decmon.log" 2>&1 & MON=$!; PIDS+=($MON)
until_ok "the Activity Monitor window opens" 8 have_win
until_ok "the list shows the dummy background process" 10 dump_has " $BG dm-bg "
until_ok "and the app, grouped under its desktop-file name" 5 dump_has "G $APP Demo App"
dump_has "S Apps" && pass "there is an Apps section" || fail "no Apps section"
dump_has "S Background" && pass "and a Background section" || fail "no Background section"
W=$(wid); xdotool windowfocus --sync "$W" 2>/dev/null; sleep 0.3
read -r WW WH <<< "$(xdotool getwindowgeometry --shell "$W" | awk -F= '/^(WIDTH|HEIGHT)=/ {printf "%s ", $2}')"
echo "  (window $WW x $WH)"
type_text "dm-"
until_ok "searching 'dm-' narrows the list to the dummies (and unfolds the app)" 5 dump_lacks "systemd"
until_ok "the app's children are listed under it" 5 dump_has "C .* dm-child"
[ "$(grep -c '^C .* dm-child' "$DUMP")" = 2 ] && pass "both children of the app" || fail "children of the app" "$(grep '^C' "$DUMP")"
sleep 3   # a few samples for the numbers
crop_shot procmon-processes
# sort by memory: the header of the Memory column
xdotool mousemove --window "$W" $((WW - 16 - 96 - 48)) 108 click 1; sleep 0.8
crop_shot procmon-sorted
# the footer says how many processes there are
shot 05-window

echo "== End task (SIGTERM) with the keyboard"
clear_search; type_text "dm-bg2"
until_ok "only dm-bg2 is listed" 5 dump_lacks "dm-bg 900"
[ "$(grep -c '^G ' "$DUMP")" = 1 ] && pass "exactly one row matches" || fail "rows: $(grep '^G' "$DUMP")"
xdotool key Down; sleep 0.3
xdotool key Delete
until_ok "End task: the process died" 6 dead "$BG2"
alive "$BG" && pass "the other dummy is untouched" || fail "dm-bg died as well"
until_ok "and it left the list" 6 dump_lacks "dm-bg2"

echo "== Force quit (SIGKILL) asks first"
clear_search; type_text "dm-bg"
until_ok "dm-bg is listed" 5 dump_has " $BG dm-bg "
xdotool key Down; sleep 0.3
# the footer's Force quit button: right edge, 23 px above the bottom
xdotool mousemove --window "$W" $((WW - 16 - 56)) $((WH - 23)) click 1; sleep 0.6
crop_shot procmon-confirm
alive "$BG" && pass "nothing happens before the confirmation" || fail "killed without asking"
xdotool key Escape; sleep 0.4
alive "$BG" && pass "Escape cancels" || fail "Escape killed it"
have_win && pass "and keeps the window open" || fail "the window closed"
xdotool mousemove --window "$W" $((WW - 16 - 56)) $((WH - 23)) click 1; sleep 0.5
xdotool key Return
until_ok "confirming Force quit kills it" 6 dead "$BG"

echo "== an app is ended with all its children"
clear_search; type_text "Demo"
until_ok "the Demo App group is listed" 5 dump_has "G $APP Demo App"
xdotool key Down; sleep 0.3
xdotool key Delete
until_ok "End task on the group ends the app" 6 dead "$APP"
for c in $CHILDREN; do until_ok "and its child $c" 6 dead "$c"; done

echo "== guards"
clear_search; sleep 0.3
alive 1 && pass "PID 1 is still there (never signalled)" || fail "pid 1 gone?"

echo "== Performance tab"
xdotool key Tab; sleep 4
crop_shot procmon-performance
xdotool key Down; sleep 0.8; crop_shot procmon-performance-memory
xdotool key Down; sleep 0.8; crop_shot procmon-performance-disk
xdotool key Tab; sleep 0.5
xdotool key Escape
until_ok "Escape closes the window" 5 no_win
kill -0 "$MON" 2>/dev/null && fail "decmon still running" || pass "decmon exited"

echo "== popup mode (what the bar opens)"
DECMON_DUMP=$DUMP decmon --popup --x 800 --top 40 >"$TMP/decmon-popup.log" 2>&1 & POP=$!; PIDS+=($POP)
until_ok "the popup window opens" 8 have_win
sleep 3; crop_shot procmon-popup
GEO=$(xdotool getwindowgeometry --shell "$(wid)" | tr '\n' ' ')
echo "  ($GEO)"
case "$GEO" in *"Y=40"*) pass "it hangs 40 px below the top edge" ;; *) fail "popup y" "$GEO" ;; esac
xdotool key Escape
until_ok "Escape closes it" 5 no_win
decmon --popup --x 800 & POP2=$!; PIDS+=($POP2)
until_ok "popup again" 8 have_win
decmon --popup --x 800; sleep 0.8
until_ok "running decmon --popup again toggles it closed" 5 no_win

echo "== the bar's agents icon opens it"
style macos
DECBAR_DEBUG_TRAY=$TMP/tray decbar >"$TMP/decbar.log" 2>&1 & BARPID=$!; PIDS+=($BARPID)
has_agents() { grep -q '^Agents ' "$TMP/tray" 2>/dev/null; }
if wait_for 8 has_agents; then
    sleep 3   # the items to its right (battery, sound, Wi-Fi) appear as the probes finish and move it
    read -r GX GY <<< "$(awk '$1 == "Agents" {print $2, $3}' "$TMP/tray" | head -1)"
    GY=12   # the horizontal bar reports only x
    xdotool mousemove 700 400; sleep 0.3
    xdotool mousemove "$GX" "$GY"; sleep 0.4; xdotool click 1
    sleep 1; have_win || { echo "  (first click ignored; clicking again)"; xdotool mousemove "$((GX + 1))" "$GY"; sleep 0.3; xdotool click 1; }
    until_ok "a click on the agents icon (macOS bar) opens the popup" 6 have_win
    gx=$(xdotool getwindowgeometry --shell "$(wid)" | awk -F= '/^X=/ {print $2}')
    [ -n "$gx" ] && [ "$gx" -gt 0 ] && pass "under the icon, not at the screen edge (x=$gx)" || fail "popup x" "$gx"
    crop_shot procmon-bar-popup
    xdotool key Escape; until_ok "closes again" 5 no_win
    style caelestia; sleep 2
    until_ok "the pill bar reports its items" 10 has_agents
    read -r GX GY <<< "$(awk '$1 == "Agents" {print $2, $3}' "$TMP/tray" | head -1)"
    xdotool mousemove "$GX" "$GY" click 1
    until_ok "a click on the agents icon (Caelestia bar) opens the popup" 6 have_win
    xdotool key Escape; until_ok "closes again" 5 no_win
else
    echo "  skip: no agents icon (no Nerd Font on this machine)"
fi

echo "== idle cost: nothing runs once it is closed"
sleep 0.5
for p in $MON $POP $POP2; do kill -0 "$p" 2>/dev/null && fail "decmon $p is still running after being closed"; done; pass "every decmon instance this test started has exited"

echo "== no crashes"
grep -il "panicked" "$TMP"/*.log 2>/dev/null | head -3 | while read -r f; do fail "panic in $f"; done
if [ -n "${DOCS:-}" ]; then for f in procmon-processes procmon-popup procmon-performance; do convert "$SHOTS/$f.png" -strip "$DOCS/$f.png" 2>/dev/null; done; fi
echo; echo "passed $PASS, failed $FAIL (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
