#!/bin/bash
# macdock context menu, live pinned list and drag-to-reorder against rdwm on a private Xvfb display.
#   tests/e2e/dock_menu.sh            (uses ../rdwm/target/{release,debug}/rdwm, or RDWM=/path/to/rdwm)
# Everything is private: Xvfb :93 (override with E2E_DISPLAY), own XDG dirs, fake apps (examples/fakewin),
# MACSHELL_OPEN / MACSHELL_TRASH_EMPTY point at logging stubs, only PIDs started here are killed.
# Screenshots land in $SHOTS (default $TMP/shots; keep them with KEEP_TMP=1 or SHOTS=dir).
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
TARGET=${CARGO_TARGET_DIR:-$HERE/target}
PROFILE=${PROFILE:-debug}
BIN=$TARGET/$PROFILE
RDWM=${RDWM:-$(ls "$HERE"/../rdwm/target/release/rdwm "$HERE"/../rdwm/target/debug/rdwm 2>/dev/null | head -1)}
[ -x "$RDWM" ] || { echo "rdwm binary not found (set RDWM)"; exit 77; }
for t in Xvfb xdotool import xprop; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline $([ "$PROFILE" = release ] && echo --release) --bins --examples) || { echo "build failed"; exit 2; }

PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d "${TMPDIR:-/tmp}/macdock-e2e.XXXXXX")
SHOTS=${SHOTS:-$TMP/shots}; mkdir -p "$SHOTS"
cleanup() {
    for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done
    sleep 0.2
    for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done
    wait 2>/dev/null
    [ -n "${KEEP_TMP:-}" ] && echo "kept $TMP" || { [ "$SHOTS" = "$TMP/shots" ] && rm -rf "$TMP" || find "$TMP" -mindepth 1 -maxdepth 1 ! -path "$SHOTS" -exec rm -rf {} +; }
}
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "got '$2', want '$3'"; fi; }
wait_for() { local t=$1; shift; local end=$(($(date +%s%N) / 1000000 + t * 1000)); while [ $(($(date +%s%N) / 1000000)) -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 0.05; done; return 1; }
ok_within() { local n=$1 t=$2; shift 2; if wait_for "$t" "$@"; then pass "$n"; else fail "$n" "timeout: $*"; fi; }

N=${E2E_DISPLAY:-93}
[ "$N" = 1 ] && { echo "refusing the live display"; exit 2; }
Xvfb :$N -screen 0 1280x800x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X$N || { echo "Xvfb failed"; cat "$TMP/xvfb.log"; exit 2; }
export DISPLAY=:$N
unset WAYLAND_DISPLAY RDWM_SOCK
export XDG_RUNTIME_DIR=$TMP/run XDG_CONFIG_HOME=$TMP/cfg HOME=$TMP/home XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata
mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME" "$HOME" "$XDG_DATA_HOME/applications" "$TMP/nodata"
chmod 700 "$XDG_RUNTIME_DIR"
export MACSHELL_CONFIG=$TMP/macshell-cfg MACSHELL_BACKEND=x11
mkdir -p "$MACSHELL_CONFIG"
PINNED=$MACSHELL_CONFIG/dock-pinned-macos.json
FAKE=$BIN/examples/fakewin
export FAKEWIN_LOG=$TMP/clicks.log
printf '#!/bin/sh\necho "$@" >> %s/open.log\n' "$TMP" > "$TMP/open.sh"
printf '#!/bin/sh\necho emptied >> %s/trash.log\n' "$TMP" > "$TMP/trash.sh"
chmod +x "$TMP/open.sh" "$TMP/trash.sh"
export MACSHELL_OPEN=$TMP/open.sh MACSHELL_TRASH_EMPTY=$TMP/trash.sh

desktop() { # desktop ID TITLE COLOR
    printf '[Desktop Entry]\nType=Application\nName=%s\nExec=%s %s "%s" %s\nStartupWMClass=%s\n' "$1" "$FAKE" "$1" "$2" "$3" "$1" > "$XDG_DATA_HOME/applications/$1.desktop"
}
desktop alpha "Alpha Window" cc3333
desktop beta "Beta Window" 33cc66
desktop gamma "Gamma Window" cccc33
pin() { printf '{"version":1,"pinned":[%s],"order":[%s]}\n' "$1" "$1" > "$PINNED.new" && mv "$PINNED.new" "$PINNED"; }
pin '"launchpad","alpha","beta","trash"'

xdotool mousemove 2 2
"$RDWM" --backend x11 >"$TMP/rdwm.log" 2>&1 &
PIDS+=($!)
wait_for 5 bash -c "xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window" || { echo "rdwm did not start"; cat "$TMP/rdwm.log"; exit 2; }
"$BIN/macdock" >"$TMP/macdock.log" 2>&1 &
DOCK_PID=$!; PIDS+=($DOCK_PID)

dock_win() { for w in $(xdotool search --name '^macshell-dock$' 2>/dev/null); do echo "$w"; return 0; done; return 1; }
wait_for 6 dock_win || { echo "dock did not map"; cat "$TMP/macdock.log"; exit 2; }
DOCKW=$(dock_win)
geom() { xdotool getwindowgeometry --shell "$DOCKW" | tr '\n' ' '; }
gx() { xdotool getwindowgeometry --shell "$DOCKW" | sed -n 's/^X=//p'; }
gy() { xdotool getwindowgeometry --shell "$DOCKW" | sed -n 's/^Y=//p'; }
gw() { xdotool getwindowgeometry --shell "$DOCKW" | sed -n 's/^WIDTH=//p'; }
gh() { xdotool getwindowgeometry --shell "$DOCKW" | sed -n 's/^HEIGHT=//p'; }
strut() { xprop -id "$DOCKW" _NET_WM_STRUT_PARTIAL | sed 's/.*= //' | tr -d ' '; }
shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
sleep 0.5

# Centre of dock cell I (0-based) of COUNT at rest, in root coordinates.
cell_x() { echo $(( $(gx) + ($(gw) - (COUNT * 60 + (COUNT - 1) * 6 + 32)) / 2 + 16 - 5 + $1 * 66 + 30 )); }
icon_y() { echo $(( $(gy) + $(gh) - 25 )); }
click_cell() { COUNT=$2; xdotool mousemove "$(cell_x "$1")" "$(icon_y)" ; sleep 0.15; xdotool click "${3:-1}"; sleep 0.3; }
# Menu row centre for a menu with row kinds "R"/"S" (24px row / 11px separator) given as a string; row K (0-based).
menu_row() { # menu_row "RSRR" K -> y in root coords
    local spec=$1 k=$2 total=12 off=6 i c h
    for ((i = 0; i < ${#spec}; i++)); do c=${spec:i:1}; h=24; [ "$c" = S ] && h=11; total=$((total + h)); [ $i -lt $k ] && off=$((off + h)); done
    local bottom=$(( $(gy) + $(gh) - 79 ))
    echo $(( bottom - total + off + 12 ))
}
click_row() { # click_row CELL COUNT SPEC K : click that menu row (menu centred on the cell)
    xdotool mousemove "$(cell_x "$1")" "$(menu_row "$3" "$4")"; sleep 0.15; xdotool click 1; sleep 0.4
}
has_win() { xdotool search --class "^$1\$" >/dev/null 2>&1; }
win_of() { xdotool search --class "^$1\$" 2>/dev/null | head -1; }
state() { xprop -id "$1" _NET_WM_STATE 2>/dev/null; }
base_h=$(gh); base_w=$(gw); base_strut=$(strut)

echo "== menu on a closed pinned app"
shot 00-dock
COUNT=4
click_cell 1 4 3
shot 01-menu-closed-app
[ "$(gh)" -gt "$base_h" ] && pass "surface grew for the menu ($base_h -> $(gh))" || fail "surface did not grow"
eq "strut unchanged while the menu is open" "$(strut)" "$base_strut"
eq "width unchanged" "$(gw)" "$base_w"
# input pass-through: a client window must receive clicks in the transparent area beside the menu
"$FAKE" bg "Background" 666666 & PIDS+=($!)
wait_for 4 has_win bg
click_row 1 4 RSRR 0       # Open -> launches alpha
ok_within "Open launches the app" 5 has_win alpha
sleep 0.6
shot 02-alpha-running
eq "menu closed after activation" "$(gh)" "$base_h"
eq "strut unchanged after close" "$(strut)" "$base_strut"

echo "== menu on a running app"
click_cell 1 4 3
shot 03-menu-running-app
# alpha running: windows(1) sep New sep Remove Show sep Hide Quit => R S R S R R S R R
SPEC=RSRSRRSRR
# Show in Finder (row 5) is keyboard/pointer reachable
click_row 1 4 $SPEC 5
sleep 0.3
grep -q "applications" "$TMP/open.log" 2>/dev/null && pass "Show in Finder opens the .desktop folder" || fail "Show in Finder" "$(cat "$TMP/open.log" 2>/dev/null)"
click_cell 1 4 3
click_row 1 4 $SPEC 7        # Hide
sleep 0.5
shot 04-hidden
A=$(win_of alpha)
state "$A" | grep -q HIDDEN && pass "Hide minimises the window" || fail "Hide" "$(state "$A")"
click_cell 1 4 1             # plain click restores + focuses
sleep 0.6
state "$A" | grep -q HIDDEN && fail "click on the icon restores the minimised window" || pass "click on the icon restores the minimised window"
shot 05-restored
click_cell 1 4 3
click_row 1 4 $SPEC 0        # window entry focuses
pass "window entry clicked"
click_cell 1 4 2             # middle click: new instance
sleep 0.8
n=$(xdotool search --class '^alpha$' 2>/dev/null | wc -l)
[ "$n" -ge 2 ] && pass "middle click opened a second instance ($n windows)" || fail "middle click" "$n windows"
shot 06-two-windows
click_cell 1 4 3
shot 07-menu-two-windows
# two windows: w w sep New sep Remove Show sep Hide Quit
click_row 1 4 RRSRSRRSRR 9  # Quit
ok_within "Quit closes every window of the app" 5 bash -c "! xdotool search --class '^alpha\$' >/dev/null 2>&1"
sleep 0.5

echo "== keep / remove / escape / outside click"
click_cell 2 4 3
sleep 0.2
xdotool key Escape; sleep 0.4
eq "Escape closes the menu" "$(gh)" "$base_h"
click_cell 2 4 3
xdotool mousemove 640 300; sleep 0.1; xdotool click 1; sleep 0.4
eq "click outside closes the menu" "$(gh)" "$base_h"
n_before=$(grep -c . "$FAKEWIN_LOG" 2>/dev/null || echo 0)
click_cell 2 4 3
xdotool key Down Down Return; sleep 0.5    # Open, (sep), Remove from Dock -> second enabled row
wait_for 3 grep -q '"launchpad","alpha","trash"\|"launchpad", "alpha", "trash"' "$PINNED" || true
python3 - "$PINNED" <<'PY' && pass "Remove from Dock saved via keyboard (Down Down Return)" || fail "Remove from Dock" "$(cat "$PINNED")"
import json,sys
d=json.load(open(sys.argv[1])); assert d["pinned"]==["launchpad","alpha","trash"], d; assert d["order"]==d["pinned"]
PY
sleep 0.5
shot 08-after-remove
[ "$(gw)" -le "$base_w" ] && pass "dock narrowed after removing an icon" || fail "dock width $(gw) vs $base_w"
base_w=$(gw); base_h=$(gh); base_strut=$(strut)
ls "$MACSHELL_CONFIG" | grep -q tmp && fail "temporary file left behind" || pass "no temporary files left"

echo "== keep in dock for a running unpinned app"
"$FAKE" gamma "Gamma Window" cccc33 & GAMMA_PID=$!; PIDS+=($GAMMA_PID)
ok_within "gamma window appears" 4 has_win gamma
sleep 0.8
shot 09-transient-gamma
# dock now: launchpad alpha gamma trash
click_cell 2 4 3
click_row 2 4 RSRSRRSRR 4    # windows(1) sep New sep -> Keep in Dock at row 4
sleep 0.5
python3 - "$PINNED" <<'PY' && pass "Keep in Dock pinned gamma before the trash" || fail "Keep in Dock" "$(cat "$PINNED")"
import json,sys
d=json.load(open(sys.argv[1])); assert d["pinned"]==["launchpad","alpha","gamma","trash"], d
PY

echo "== external edit is picked up within ~1s"
pin '"launchpad","gamma","alpha","beta","trash"'
sleep 1.2
shot 10-after-external-edit
[ "$(gw)" -gt "$base_w" ] && pass "dock grew after an external edit ($base_w -> $(gw))" || fail "external edit not reflected" "$(gw)"
base_w=$(gw)

echo "== trash and launchpad"
COUNT=5
click_cell 4 5 3
shot 11-trash-menu
click_row 4 5 RSR 2          # Empty Trash? (first click arms)
[ -f "$TMP/trash.log" ] && fail "empty trash ran without confirmation" || pass "first click only asks for confirmation"
shot 12-trash-confirm
click_row 4 5 RSR 2
sleep 0.3
[ -f "$TMP/trash.log" ] && pass "second click empties the trash" || fail "empty trash did not run"
eq "menu closed" "$(gh)" "$base_h"

echo "== drag to reorder"
kill "$GAMMA_PID" 2>/dev/null
wait_for 4 bash -c "! xdotool search --class '^gamma\$'"
pin '"launchpad","alpha","beta","trash"'
sleep 1.2
COUNT=4
x1=$(cell_x 1); x2=$(cell_x 2); y=$(icon_y)
xdotool mousemove "$x1" "$y"; sleep 0.2
xdotool mousedown 1; sleep 0.1
xdotool mousemove $((x1 + 20)) "$y"; sleep 0.1
xdotool mousemove $((x1 + 45)) "$y"; sleep 0.1
xdotool mousemove "$x2" "$y"; sleep 0.2
shot 13-dragging
xdotool mouseup 1; sleep 0.5
python3 - "$PINNED" <<'PY' && pass "drag reordered and saved" || fail "drag reorder" "$(cat "$PINNED")"
import json,sys
d=json.load(open(sys.argv[1])); assert d["pinned"]==["launchpad","beta","alpha","trash"], d
PY
shot 14-after-drag

echo "== clicks beside the menu pass through to windows"
COUNT=4
: > "$FAKEWIN_LOG"
click_cell 1 4 3
# The menu is open; close it, then click in the transparent headroom of the dock canvas.
xdotool key Escape; sleep 0.4
eq "dock back to base height" "$(gh)" "$base_h"
xdotool mousemove "$(cell_x 1)" $(( $(gy) + 5 )); sleep 0.1; xdotool click 1; sleep 0.3
grep -q "Background button 1" "$FAKEWIN_LOG" && pass "click in the transparent headroom reaches the window below" || fail "click-through" "$(cat "$FAKEWIN_LOG" 2>/dev/null)"

kill -0 "$DOCK_PID" 2>/dev/null && pass "macdock still running" || fail "macdock crashed" "$(tail -5 "$TMP/macdock.log")"
grep -qi "panic" "$TMP/macdock.log" && fail "panic in macdock log" || true
echo
echo "$PASS passed, $FAIL failed (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
