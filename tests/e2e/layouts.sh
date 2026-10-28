#!/usr/bin/env bash
# The Caelestia layouts end to end: left, right, top, bottom and split, the real macbar (and macdock) on a private Xvfb with a
# private rdwm and fake system tools.
#   tests/e2e/layouts.sh               (E2E_DISPLAY=:96 by default; SHOTS=dir keeps the screenshots; PROFILE=debug uses debug binaries)
# For every position it switches with `macthemectl layout`, waits for the running bar to re-lay out by itself (hot reload), and checks
# the bar window's geometry, the _NET_WM_STRUT_PARTIAL it publishes, that the strip is painted at the right edge, that rdwm tiles a
# new window inside the free area, the workspace pills, the hover popout opening on the side that faces the screen, and (split) that
# there are two surfaces with a strut each, one process per role, and that a second copy of either role leaves at once. A screenshot of
# each layout is kept. It also checks that the dock stays hidden, comes back only when macshell.json says so (above a bottom bar), that
# `macthemectl style macos` and back remember the layout, and that the processes survive.
# Everything is private: own HOME / XDG dirs, a private Xvfb, rdwm's own socket in the private XDG_RUNTIME_DIR, tools on PATH are the
# fakes of tests/fakebin; only PIDs started here (and the bottom bar macbar itself starts) are killed. It refuses the live displays.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
PROFILE=${PROFILE:-release}
BIN=${CARGO_TARGET_DIR:-$HERE/target}/$PROFILE
RDWM_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
[ -x "$RDWM_DIR/rdwm" ] || { echo "rdwm not found (set RDWM_BIN_DIR)"; exit 77; }
for t in Xvfb xdotool import xprop jq; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline $([ "$PROFILE" = release ] && echo --release) --bins --examples) || { echo "build failed"; exit 2; }
N=${E2E_DISPLAY:-:96}
case "$N" in :0 | :1 | :2 | 0 | 1 | 2) echo "refusing a live display"; exit 2 ;; esac
[ -e "/tmp/.X11-unix/X${N#:}" ] && { echo "display $N is already in use; pick another with E2E_DISPLAY"; exit 2; }

PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d /tmp/macshell-layouts-e2e.XXXXXX)
SHOTS=${SHOTS:-$TMP/shots}; mkdir -p "$SHOTS"
cleanup() {
    for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done
    # the bottom bar is started by the main bar, not by this script: its pid is in its lock file
    [ -s "$XDG_RUNTIME_DIR/macbar-bottom.lock" ] && kill "$(cat "$XDG_RUNTIME_DIR/macbar-bottom.lock")" 2>/dev/null
    sleep 0.3
    for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done
    wait 2>/dev/null
    if [ "$SHOTS" = "$TMP/shots" ]; then rm -rf "$TMP"; else find "$TMP" -mindepth 1 -maxdepth 1 ! -path "$SHOTS" -exec rm -rf {} +; rmdir "$TMP" 2>/dev/null; fi
}
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "got '$2', want '$3'"; fi; }
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
THICK=52                       # the strut of every pill bar
SURF_W=444                     # a side bar's surface: the strip plus room for popouts
SURF_H=772                     # a top / bottom bar's surface
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
color() { macthemectl get --json | jq -r ".colors.$1"; }

# win_of NAME: the window; geom/strut of it ("X Y W H", the 12 strut numbers)
win_of() { xdotool search --name "^$1\$" 2>/dev/null | head -1; }
geom_of() { local w; w=$(win_of "$1"); [ -n "$w" ] && xdotool getwindowgeometry --shell "$w" 2>/dev/null | awk -F= '/^(X|Y|WIDTH|HEIGHT)=/ {printf "%s ", $2}' | sed 's/ $//'; }
strut_of() { local w; w=$(win_of "$1"); [ -n "$w" ] && xprop -id "$w" _NET_WM_STRUT_PARTIAL 2>/dev/null | sed 's/.*= //; s/,//g'; }
geom_is() { [ "$(geom_of "$1")" = "$2" ]; }
gone() { [ -z "$(win_of "$1")" ]; }
dock_visible() { xdotool search --onlyvisible --name '^macshell-dock$' 2>/dev/null | head -1 | grep -q .; }
dock_gone() { ! dock_visible; }
view_mask() { rdwmctl state 2>/dev/null | jq -r '.state.monitors[0].view'; }
view_is() { [ "$(view_mask)" = "$1" ]; }
# tray_xy FILE KIND: centre of an item as MACBAR_DEBUG_TRAY writes it ("Kind X Y"); the bar's surface origin is added by the caller
tray_xy() { awk -v k="$2" '$1 == k {print $2, $3}' "$1" 2>/dev/null | head -1; }

# a window on the first tag so the tag has something tiled
"$FAKE" Alpha "Alpha Window" cc3333 >/dev/null 2>&1 & PIDS+=($!)
sleep 1

echo "== start: Caelestia style, default layout"
macthemectl style caelestia >/dev/null 2>&1
TRAY=$TMP/tray
MACBAR_DEBUG_TRAY=$TRAY macbar >"$TMP/macbar.log" 2>&1 & BARPID=$!; PIDS+=($BARPID)
macdock >"$TMP/macdock.log" 2>&1 & DOCKPID=$!; PIDS+=($DOCKPID)
until_ok "the bar window is on the left edge" 10 geom_is macshell-bar "0 0 $SURF_W $SH"
eq "macthemectl layout prints left by default" "$(macthemectl layout)" left
BG=$(color background)
until_ok "the dock hid itself" 10 dock_gone
MAIN_LOCK=$(cat "$XDG_RUNTIME_DIR/macbar.lock" 2>/dev/null)
eq "the main bar holds its lock (its pid)" "$MAIN_LOCK" "$BARPID"
macbar >"$TMP/macbar2.log" 2>&1 & P2=$!
if wait_for 5 bash -c "! kill -0 $P2 2>/dev/null"; then pass "a second main bar exits at once (single instance)"; else fail "a second macbar kept running"; kill $P2 2>/dev/null; fi
kill -0 $BARPID 2>/dev/null && pass "...and the first one is untouched" || fail "the first macbar died"

# check_layout POS: everything that must hold for one position once the bar has moved.
# Arguments after POS: edge  mainGeom  mainStrut  [bottomGeom bottomStrut]
free_area() { # the rectangle windows may use, as "x0 y0 x1 y1", for the struts published by the bars
    local l=0 r=0 t=0 b=0 s w
    for w in macshell-bar macshell-bar-bottom; do
        s=$(strut_of $w) || continue
        [ -n "$s" ] || continue
        set -- $s
        [ "$1" -gt "$l" ] && l=$1; [ "$2" -gt "$r" ] && r=$2; [ "$3" -gt "$t" ] && t=$3; [ "$4" -gt "$b" ] && b=$4
    done
    echo "$l $t $((SW - r)) $((SH - b))"
}
check_layout() {
    local pos=$1 main_geom=$2 main_strut=$3 bottom_geom=${4:-} bottom_strut=${5:-}
    echo "== layout $pos"
    macthemectl layout "$pos" >/dev/null
    eq "macthemectl layout reads back $pos" "$(macthemectl layout)" "$pos"
    eq "macshell.json has bar.position = $pos" "$(jq -r .bar.position "$MACSHELL_CONFIG/macshell.json")" "$pos"
    until_ok "$pos: the bar re-laid out by itself (geometry $main_geom)" 10 geom_is macshell-bar "$main_geom"
    eq "$pos: strut" "$(strut_of macshell-bar)" "$main_strut"
    if [ -n "$bottom_geom" ]; then
        until_ok "$pos: the second surface appeared (geometry $bottom_geom)" 12 geom_is macshell-bar-bottom "$bottom_geom"
        eq "$pos: the bottom strut" "$(strut_of macshell-bar-bottom)" "$bottom_strut"
        BOTTOM_PID=$(cat "$XDG_RUNTIME_DIR/macbar-bottom.lock" 2>/dev/null)
        kill -0 "$BOTTOM_PID" 2>/dev/null && pass "$pos: the bottom bar is its own process ($BOTTOM_PID, lock held)" || fail "$pos: no bottom process"
        macbar --role bottom >"$TMP/macbar3.log" 2>&1 & P3=$!
        if wait_for 5 bash -c "! kill -0 $P3 2>/dev/null"; then pass "$pos: a second bottom bar exits at once"; else fail "$pos: a second bottom bar kept running"; kill $P3 2>/dev/null; fi
    else
        until_ok "$pos: no second surface" 10 gone macshell-bar-bottom
    fi
    until_ok "$pos: the dock is still hidden" 4 dock_gone
    sleep 0.8
    shot "layout-$pos"
    # the strip is painted in the theme background at its screen edge, and the room beside it is not
    case $pos in
        left)   eqpx "left: the strip is painted" 3 450 "$BG"; nepx "left: the popout room is clear (wallpaper)" 300 450 "$BG" ;;
        right)  eqpx "right: the strip is painted" $((SW - 3)) 450 "$BG"; nepx "right: the popout room is clear (wallpaper)" $((SW - 300)) 450 "$BG" ;;
        top)    eqpx "top: the strip is painted" 800 3 "$BG"; nepx "top: the popout room is clear (wallpaper)" 800 300 "$BG" ;;
        bottom) eqpx "bottom: the strip is painted" 800 $((SH - 3)) "$BG"; nepx "bottom: the popout room is clear (wallpaper)" 800 $((SH - 300)) "$BG" ;;
        split)  eqpx "split: the top strip is painted" 800 3 "$BG"; eqpx "split: the bottom strip is painted" 800 $((SH - 3)) "$BG"; nepx "split: the middle is the wallpaper" 800 450 "$BG" ;;
    esac
    # rdwm tiles a new window inside the free area
    local area x0 y0 x1 y1
    area=$(free_area); read -r x0 y0 x1 y1 <<< "$area"
    "$FAKE" Beta "Beta Window" 33cc66 >/dev/null 2>&1 & local wp=$!; PIDS+=($wp)
    sleep 1.5
    local bw geo wx wy ww wh
    bw=$(xdotool search --name '^Beta Window$' 2>/dev/null | head -1)
    geo=$(xdotool getwindowgeometry --shell "$bw" 2>/dev/null | awk -F= '/^(X|Y|WIDTH|HEIGHT)=/ {printf "%s ", $2}')
    read -r wx wy ww wh <<< "$geo"
    if [ -n "$wx" ] && [ "$wx" -ge "$x0" ] && [ "$wy" -ge "$y0" ] && [ $((wx + ww)) -le "$x1" ] && [ $((wy + wh)) -le "$y1" ]; then
        pass "$pos: a tiled window ($wx,$wy ${ww}x$wh) lies inside the free area ($x0,$y0)-($x1,$y1)"
    else
        fail "$pos: a tiled window overlaps a bar" "window '$geo', free area '$area'"
    fi
    kill $wp 2>/dev/null; wait $wp 2>/dev/null
    PIDS=("${PIDS[@]/$wp}")
}

# hover_popout POS EDGE SURFACE_X SURFACE_Y TRAYFILE: hover the sound icon; its popup must open on the side that faces the screen
hover_popout() {
    local pos=$1 file=$2 ox=$3 oy=$4 ax ay
    read -r ax ay <<< "$(tray_xy "$file" Audio)"
    if [ -z "$ax" ]; then fail "$pos: no sound icon in $file (fake wpctl not found?)"; return; fi
    ax=$((ax + ox)); ay=$((ay + oy))
    xdotool mousemove 800 450; sleep 0.9
    # a probe point one popup-gap beyond the icon, toward the screen
    local probe_x probe_y
    case $pos in
        left)   probe_x=$((ax + 60)); probe_y=$ay ;;
        right)  probe_x=$((ax - 60)); probe_y=$ay ;;
        top)    probe_x=$ax; probe_y=$((ay + 60)) ;;
        bottom | split-bottom) probe_x=$ax; probe_y=$((ay - 60)) ;;
        split-top) probe_x=$ax; probe_y=$((ay + 60)) ;;
    esac
    local under; under=$(px $probe_x $probe_y)
    xdotool mousemove $((ax - 1)) $((ay - 1)); sleep 0.1; xdotool mousemove $ax $ay; sleep 0.9
    shot "popout-$pos"
    nepx "$pos: hovering the sound icon opens its popup toward the screen (probe $probe_x,$probe_y)" $probe_x $probe_y "$under"
    xdotool mousemove 800 450; sleep 0.9
    eqpx "$pos: moving away closes it" $probe_x $probe_y "$under"
}

# workspace_click POS TRAYFILE OX OY: pill 3 views tag 3 (rdwm view mask 4), pill 1 goes back
workspace_click() {
    local pos=$1 file=$2 ox=$3 oy=$4 wx wy cell_x cell_y
    read -r wx wy <<< "$(tray_xy "$file" Workspaces)"
    if [ -z "$wx" ]; then fail "$pos: no workspace pills in $file"; return; fi
    wx=$((wx + ox)); wy=$((wy + oy))
    # nine tags: 9 cells plus 12 px of padding; cell i is centred 6 + cell * (i + 0.5) from the pill's start
    case $pos in
        left | right) cell_x() { echo $wx; }; cell_y() { echo $((wy - 123 + 6 + 26 * $1 + 13)); } ;;
        *)            cell_x() { echo $((wx - 141 + 6 + 30 * $1 + 15)); }; cell_y() { echo $wy; } ;;
    esac
    xdotool mousemove "$(cell_x 2)" "$(cell_y 2)" click 1
    until_ok "$pos: clicking pill 3 views tag 3" 5 view_is 4
    xdotool mousemove "$(cell_x 0)" "$(cell_y 0)" click 1
    until_ok "$pos: clicking pill 1 goes back" 5 view_is 1
    xdotool mousemove 800 450; sleep 0.5
}

# surface origin of the bars on the 1600x900 screen
RIGHT_X=$((SW - SURF_W)); BOTTOM_Y=$((SH - SURF_H))

check_layout left   "0 0 $SURF_W $SH"                   "$THICK 0 0 0 0 $((SH - 1)) 0 0 0 0 0 0"
workspace_click left "$TRAY" 0 0
hover_popout left "$TRAY" 0 0

check_layout right  "$RIGHT_X 0 $SURF_W $SH"            "0 $THICK 0 0 0 0 0 $((SH - 1)) 0 0 0 0"
workspace_click right "$TRAY" "$RIGHT_X" 0
hover_popout right "$TRAY" "$RIGHT_X" 0

check_layout top    "0 0 $SW $SURF_H"                   "0 0 $THICK 0 0 0 0 0 0 $((SW - 1)) 0 0"
workspace_click top "$TRAY" 0 0
hover_popout top "$TRAY" 0 0

check_layout bottom "0 $BOTTOM_Y $SW $SURF_H"           "0 0 0 $THICK 0 0 0 0 0 0 0 $((SW - 1))"
workspace_click bottom "$TRAY" 0 "$BOTTOM_Y"
hover_popout bottom "$TRAY" 0 "$BOTTOM_Y"

check_layout split  "0 0 $SW $SURF_H"                   "0 0 $THICK 0 0 0 0 0 0 $((SW - 1)) 0 0" \
                    "0 $BOTTOM_Y $SW $SURF_H"           "0 0 0 $THICK 0 0 0 0 0 0 0 $((SW - 1))"
# the top bar owns the OS icon, workspaces and window; the bottom bar the status icons, clock and power
HEAD_ITEMS=$(awk '{print $1}' "$TRAY" | tr '\n' ' ')
BOTTOM_ITEMS=$(awk '{print $1}' "$TRAY.bottom" 2>/dev/null | tr '\n' ' ')
case "$HEAD_ITEMS" in *Logo*Workspaces*) pass "split: the top bar has the OS icon and the workspaces";; *) fail "split: top bar items" "$HEAD_ITEMS";; esac
case "$HEAD_ITEMS" in *Clock*|*Power*|*Audio*) fail "split: the top bar must not hold the tail" "$HEAD_ITEMS";; *) pass "split: the top bar has no clock, power or status icons";; esac
case "$BOTTOM_ITEMS" in *Control*Audio*Clock*Power*) pass "split: the bottom bar has the tray, status icons, clock and power";; *) fail "split: bottom bar items" "$BOTTOM_ITEMS";; esac
case "$BOTTOM_ITEMS" in *Logo*|*Workspaces*) fail "split: the bottom bar must not hold the head" "$BOTTOM_ITEMS";; *) pass "split: the bottom bar has no OS icon or workspaces";; esac
workspace_click split-top "$TRAY" 0 0
hover_popout split-bottom "$TRAY.bottom" 0 "$BOTTOM_Y"
shot "layout-split-final"

echo "== the dock stays hidden unless macshell.json says so; above a bottom bar when it does"
jq '.dock.enabled = true' "$MACSHELL_CONFIG/macshell.json" > "$TMP/m.json" && mv "$TMP/m.json" "$MACSHELL_CONFIG/macshell.json"
until_ok "dock.enabled = true shows the dock under the split layout" 10 dock_visible
DG=$(geom_of macshell-dock); read -r dx dy dw dh <<< "$DG"
[ -n "$dy" ] && [ $((dy + dh)) -le $((SH - THICK)) ] && pass "the dock floats above the bottom bar (bottom edge ${dy}+${dh} <= $((SH - THICK)))" || fail "the dock overlaps the bottom bar" "$DG"
shot "layout-split-with-dock"
jq '.dock.enabled = "auto"' "$MACSHELL_CONFIG/macshell.json" > "$TMP/m.json" && mv "$TMP/m.json" "$MACSHELL_CONFIG/macshell.json"
until_ok "dock.enabled = auto hides it again" 10 dock_gone

echo "== styles keep the layout"
macthemectl layout bottom >/dev/null
until_ok "bottom again: the second surface goes away by itself" 12 gone macshell-bar-bottom
macthemectl style macos >/dev/null
until_ok "macOS style: the classic bar is back on top" 10 geom_is macshell-bar "0 0 $SW 420"
eq "...with its own top strut of 26" "$(strut_of macshell-bar)" "0 0 26 0 0 0 0 0 0 $((SW - 1)) 0 0"
eq "the layout setting is remembered" "$(macthemectl layout)" bottom
until_ok "the dock is back" 10 dock_visible
macthemectl style caelestia-split >/dev/null
eq "caelestia-split is a style word" "$(macthemectl layout)" split
until_ok "...and the split layout returns" 12 geom_is macshell-bar-bottom "0 $BOTTOM_Y $SW $SURF_H"
until_ok "the top half too" 10 geom_is macshell-bar "0 0 $SW $SURF_H"
until_ok "the dock hides again" 10 dock_gone
macthemectl layout left >/dev/null
until_ok "left: single bar again" 12 gone macshell-bar-bottom
shot "layout-left-again"

echo "== no crashes"
kill -0 $BARPID 2>/dev/null || fail "macbar died"
kill -0 $DOCKPID 2>/dev/null || fail "macdock died"
grep -il "panicked" "$TMP"/*.log 2>/dev/null | head -3 | while read -r f; do fail "panic in $f"; done
echo; echo "passed $PASS, failed $FAIL (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
