#!/bin/bash
# decdock's Windows-style taskbar against rdwm on a private Xvfb display: switch the style by editing
# dock-settings.json (and from the dock's own menu), struts / workarea, Start, search, task buttons
# (focus / minimise / restore / middle click / right-click menu / drag), tray popups, calendar, show desktop,
# the left aligned and titled variants, and switching back to the macOS dock. Also decbar's bar.enabled.
#   tests/e2e/taskbar.sh                (uses ../rdwm/target/{release,debug}/rdwm, or RDWM=/path/to/rdwm)
# Everything is private: Xvfb :140 (E2E_DISPLAY), own XDG dirs, fake apps (examples/fakewin), fake system tools
# (tests/fakebin), logging stubs for the launcher / search; only PIDs started here are killed.
# Screenshots land in $SHOTS (default $TMP/shots; keep them with KEEP_TMP=1 or SHOTS=dir).
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
TARGET=${CARGO_TARGET_DIR:-$HERE/target}
PROFILE=${PROFILE:-release}
BIN=$TARGET/$PROFILE
RDWM=${RDWM:-$(ls "$HERE"/../rdwm/target/release/rdwm "$HERE"/../rdwm/target/debug/rdwm 2>/dev/null | head -1)}
[ -x "$RDWM" ] || { echo "rdwm binary not found (set RDWM)"; exit 77; }
for t in Xvfb xdotool import xprop python3; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline $([ "$PROFILE" = release ] && echo --release) --bins --examples) || { echo "build failed"; exit 2; }

PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d "${TMPDIR:-/tmp}/taskbar-e2e.XXXXXX")
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

N=${E2E_DISPLAY:-140}
[ "$N" = 1 ] && { echo "refusing the live display"; exit 2; }
Xvfb :$N -screen 0 1280x800x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X$N || { echo "Xvfb failed"; cat "$TMP/xvfb.log"; exit 2; }
export DISPLAY=:$N
unset WAYLAND_DISPLAY RDWM_SOCK
export XDG_RUNTIME_DIR=$TMP/run XDG_CONFIG_HOME=$TMP/cfg HOME=$TMP/home XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata
mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME" "$HOME" "$XDG_DATA_HOME/applications" "$TMP/nodata"
chmod 700 "$XDG_RUNTIME_DIR"
export DECEBAL_CONFIG=$TMP/decebal-cfg DECEBAL_BACKEND=x11 DECEBAL_SCALE=${DECEBAL_SCALE:-1}
mkdir -p "$DECEBAL_CONFIG"
PINNED=$DECEBAL_CONFIG/dock-pinned-macos.json
SETTINGS=$DECEBAL_CONFIG/dock-settings.json
SHELLCFG=$DECEBAL_CONFIG/decebal.json
FAKE=$BIN/examples/fakewin
RDWMCTL=${RDWMCTL:-$(dirname "$RDWM")/rdwmctl}
export FAKEWIN_LOG=$TMP/clicks.log
# stubs that log instead of starting the real launcher / search / settings
printf '#!/bin/sh\necho launchpad >> %s/start.log\n' "$TMP" > "$TMP/launchpad.sh"
printf '#!/bin/sh\necho search >> %s/start.log\n' "$TMP" > "$TMP/search.sh"
printf '#!/bin/sh\necho open "$@" >> %s/open.log\n' "$TMP" > "$TMP/open.sh"
chmod +x "$TMP"/*.sh
export DECEBAL_LAUNCHPAD=$TMP/launchpad.sh DECEBAL_SPOTLIGHT=$TMP/search.sh DECEBAL_OPEN=$TMP/open.sh
# fake system tools for the tray
export FAKE_STATE=$TMP/fs; mkdir -p "$FAKE_STATE" "$TMP/ps/BAT0"
echo 0.50 > $FAKE_STATE/vol; echo 0 > $FAKE_STATE/mute; echo enabled > $FAKE_STATE/wifi; echo yes > $FAKE_STATE/btpower; echo 50 > $FAKE_STATE/bright; : > $FAKE_STATE/log
echo Battery > $TMP/ps/BAT0/type; echo 64 > $TMP/ps/BAT0/capacity; echo Discharging > $TMP/ps/BAT0/status
export DECBAR_POWER_SUPPLY=$TMP/ps PATH=$HERE/tests/fakebin:$PATH

desktop() { printf '[Desktop Entry]\nType=Application\nName=%s\nExec=%s %s "%s" %s\nStartupWMClass=%s\n' "$1" "$FAKE" "$1" "$2" "$3" "$1" > "$XDG_DATA_HOME/applications/$1.desktop"; }
desktop alpha "Alpha Window" cc3333
desktop beta "Beta Window" 33cc66
desktop gamma "Gamma Window" cccc33
pin() { printf '{"version":1,"pinned":[%s],"order":[%s]}\n' "$1" "$1" > "$PINNED.new" && mv "$PINNED.new" "$PINNED"; }
setcfg() { printf '%s\n' "$1" > "$SETTINGS.new" && mv "$SETTINGS.new" "$SETTINGS"; }
pin '"launchpad","alpha","beta","trash"'
setcfg '{"appSpacing":6,"magnification":1.4,"transparency":0.4,"roundness":0.45}'

xdotool mousemove 2 2
"$RDWM" --backend x11 >"$TMP/rdwm.log" 2>&1 &
PIDS+=($!)
wait_for 5 bash -c "xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window" || { echo "rdwm did not start"; cat "$TMP/rdwm.log"; exit 2; }
export DECDOCK_DEBUG_LAYOUT=$TMP/layout.txt
"$BIN/decdock" >"$TMP/decdock.log" 2>&1 &
DOCK_PID=$!; PIDS+=($DOCK_PID)

dock_win() { for w in $(xdotool search --name '^decebal-dock$' 2>/dev/null); do echo "$w"; return 0; done; return 1; }
wait_for 6 dock_win || { echo "dock did not map"; cat "$TMP/decdock.log"; exit 2; }
DOCKW=$(dock_win)
gx() { xdotool getwindowgeometry --shell "$DOCKW" | sed -n 's/^X=//p'; }
gy() { xdotool getwindowgeometry --shell "$DOCKW" | sed -n 's/^Y=//p'; }
gw() { xdotool getwindowgeometry --shell "$DOCKW" | sed -n 's/^WIDTH=//p'; }
gh() { xdotool getwindowgeometry --shell "$DOCKW" | sed -n 's/^HEIGHT=//p'; }
strut() { xprop -id "$DOCKW" _NET_WM_STRUT_PARTIAL 2>/dev/null | sed 's/.*= //' | tr -d ' '; }
workarea() { xprop -root _NET_WORKAREA | sed 's/.*= //' | tr -d ' ' | cut -d, -f1-4; }
shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
has_win() { xdotool search --class "^$1\$" >/dev/null 2>&1; }
win_of() { xdotool search --class "^$1\$" 2>/dev/null | head -1; }
hidden() { xprop -id "$1" _NET_WM_STATE 2>/dev/null | grep -q HIDDEN; }
# Root coordinates of a zone from the dock's layout dump: pos KEY [NAME]  (lines: "key [name] x y ...")
lay() { cat "$DECDOCK_DEBUG_LAYOUT" 2>/dev/null; }
pos() { # pos start | pos task alpha | pos tray Audio | pos clock ...
    local line
    if [ $# -ge 2 ]; then line=$(lay | awk -v k="$1" -v n="$2" '$1==k && $2==n {print $3, $4}'); else line=$(lay | awk -v k="$1" '$1==k {print $2, $3}'); fi
    set -- $line
    echo "$(( $(gx) + ${1%.*} )) $(( $(gy) + ${2%.*} ))"
}
click_at() { xdotool mousemove $1 $2; sleep 0.12; xdotool click ${3:-1}; sleep 0.35; }
click_zone() { local b=$1; shift; click_at $(pos "$@") $b; }
active_task() { lay | awk '$1=="task" && $5==1 {print $2}'; }
has_task() { lay | awk -v n="$1" '$1=="task" && $2==n {f=1} END{exit !f}'; }
sleep 0.6

echo "== macOS style baseline"
shot 00-mac
mac_w=$(gw); mac_h=$(gh); mac_strut=$(strut)
[ "$mac_w" -lt 1000 ] && pass "mac dock is a floating pill ($mac_w px wide)" || fail "mac dock width $mac_w"
eq "mac strut reserves the dock" "$mac_strut" "0,0,0,84,0,0,0,0,0,0,$(( (1280 - mac_w) / 2 )),$(( (1280 - mac_w) / 2 + mac_w - 1 ))"
mac_work=$(workarea)

echo "== switch to the Windows style by editing dock-settings.json"
setcfg '{"appSpacing":6,"magnification":1.4,"transparency":0.4,"roundness":0.45,"style":"windows"}'
ok_within "dock becomes monitor wide" 4 bash -c "[ \"\$(xdotool getwindowgeometry --shell $DOCKW | sed -n 's/^WIDTH=//p')\" = 1280 ]"
sleep 0.5
shot 01-windows-center
eq "taskbar height" "$(gh)" 48
eq "taskbar sits at the bottom edge" "$(( $(gy) + $(gh) ))" 800
eq "strut covers the taskbar over the full width" "$(strut)" "0,0,0,48,0,0,0,0,0,0,0,1279"
win_work=$(workarea)
[ "$win_work" != "$mac_work" ] && pass "rdwm workarea changed ($mac_work -> $win_work)" || fail "workarea unchanged" "$win_work"
eq "workarea height follows the strut" "$(echo $win_work | cut -d, -f4)" 752
sx=$(pos start | cut -d' ' -f1)
[ "$sx" -gt 400 ] && [ "$sx" -lt 640 ] && pass "centred alignment: Start button left of centre ($sx)" || fail "Start position $sx"

echo "== Start and search"
click_zone 1 start
ok_within "Start opens the launcher" 3 grep -q launchpad "$TMP/start.log"
click_zone 1 search
ok_within "search opens spotlight" 3 grep -q search "$TMP/start.log"

echo "== task buttons"
has_task alpha && pass "pinned alpha has a button" || fail "no alpha button" "$(lay | grep task)"
has_task launchpad && fail "launchpad must not be a task button" || pass "launchpad and trash are not task buttons"
click_zone 1 task alpha
ok_within "click on a closed pinned button launches it" 6 has_win alpha
sleep 1
shot 02-alpha-running
A=$(win_of alpha)
eq "the new window is the active task" "$(active_task)" alpha
click_zone 1 task alpha
ok_within "click on the active app's button minimises it" 4 hidden "$A"
shot 03-alpha-minimised
click_zone 1 task alpha
ok_within "click again restores and focuses it" 4 bash -c "! xprop -id $A _NET_WM_STATE | grep -q HIDDEN"
sleep 0.4
eq "restored window is active" "$(active_task)" alpha
click_zone 2 task alpha
sleep 1
n=$(xdotool search --class '^alpha$' 2>/dev/null | wc -l)
[ "$n" -ge 2 ] && pass "middle click opens a new instance ($n windows)" || fail "middle click" "$n windows"

echo "== right-click menu (Windows wording)"
click_zone 3 task alpha
shot 04-task-menu
[ "$(gh)" -gt 48 ] && pass "surface grew for the menu ($(gh))" || fail "menu did not grow the surface"
eq "strut unchanged while the menu is open" "$(strut)" "0,0,0,48,0,0,0,0,0,0,0,1279"
xdotool key Escape; sleep 0.4
eq "Escape closes the menu" "$(gh)" 48
click_zone 3 task beta
# beta is closed + pinned: rows Beta / sep / Unpin from taskbar / Open file location
bx=$(pos task beta | cut -d' ' -f1)
by=$(( $(gy) + $(gh) - 48 - 8 ))
shot 05-beta-menu
xdotool key Escape; sleep 0.3

echo "== tray popups and calendar"
for k in Audio Network Bluetooth Battery; do
    [ -n "$(lay | awk -v k=$k '$1=="tray" && $2==k')" ] && pass "tray shows $k" || fail "tray misses $k" "$(lay | grep tray)"
done
click_zone 1 tray Audio
shot 06-sound-popup
[ "$(gh)" -gt 100 ] && pass "sound popup opens above the taskbar" || fail "no popup" "$(gh)"
eq "strut unchanged with a popup open" "$(strut)" "0,0,0,48,0,0,0,0,0,0,0,1279"
xdotool key Escape; sleep 0.4
eq "Escape closes the popup" "$(gh)" 48
click_zone 1 tray Network; sleep 1.2; shot 07-wifi-popup; xdotool key Escape; sleep 0.3
click_zone 1 tray Bluetooth; sleep 1.2; shot 08-bluetooth-popup; xdotool key Escape; sleep 0.3
click_zone 1 tray Battery; sleep 0.5; shot 09-battery-popup; xdotool key Escape; sleep 0.3
vol0=$(cat $FAKE_STATE/vol)
xdotool mousemove $(pos tray Audio); sleep 0.2; xdotool click 4 click 4; sleep 0.8
[ "$(cat $FAKE_STATE/vol)" != "$vol0" ] && pass "wheel over the sound icon changes the volume ($vol0 -> $(cat $FAKE_STATE/vol))" || fail "wheel volume"
click_zone 2 tray Audio
eq "middle click mutes" "$(cat $FAKE_STATE/mute)" 1
click_zone 2 tray Audio
click_zone 1 clock
shot 10-calendar
[ "$(gh)" -gt 250 ] && pass "calendar opens above the clock" || fail "calendar" "$(gh)"
click_zone 1 clock
eq "second click on the clock closes the calendar" "$(gh)" 48
click_zone 1 tray Audio
click_at 600 300
eq "click outside closes a popup" "$(gh)" 48

echo "== show desktop"
"$FAKE" beta "Beta Window" 33cc66 & PIDS+=($!)
ok_within "beta window appears" 4 has_win beta
sleep 0.8
click_zone 1 desktop
sleep 0.8
shot 11-show-desktop
B=$(win_of beta)
hidden "$B" && pass "the sliver minimises the windows" || fail "show desktop" "$(xprop -id $B _NET_WM_STATE)"
click_zone 1 desktop
sleep 0.8
hidden "$B" && fail "second click restores them" || pass "second click restores the windows"
shot 12-after-show-desktop

echo "== drag to reorder pinned"
before=$(python3 -c "import json;print(','.join(json.load(open('$PINNED'))['pinned']))")
read -r ax ay <<< "$(pos task alpha)"; read -r bx2 by2 <<< "$(pos task beta)"
xdotool mousemove $ax $ay mousedown 1; sleep 0.2
xdotool mousemove $(( ax + 14 )) $ay; sleep 0.1; xdotool mousemove $(( (ax + bx2) / 2 )) $ay; sleep 0.1; xdotool mousemove $(( bx2 + 8 )) $ay; sleep 0.3
shot 13-dragging
xdotool mouseup 1; sleep 0.8
after=$(python3 -c "import json;print(','.join(json.load(open('$PINNED'))['pinned']))")
eq "dragging alpha over beta swaps them in the pinned file" "$after" "launchpad,beta,alpha,trash"

echo "== menu actions: pin / unpin, close, style from the menu"
click_zone 3 task alpha
shot 14-menu-alpha
n_before=$(xdotool search --class '^alpha$' 2>/dev/null | wc -l)
# alpha is running with 2 windows: w, w, sep, name, sep, Unpin, Open file location, sep, Minimize, Close all windows
# rows 24px, separators 9px; find row centres from the bottom: surface bottom is the strip top - 8
menu_y() { # menu_y "RRSRSRRSRR" K [BAR] -> root y of row K (the menu's bottom edge is 8px above the strip)
    local spec=$1 k=$2 bar=${3:-48} total=12 off=6 i c h
    for ((i = 0; i < ${#spec}; i++)); do c=${spec:i:1}; h=24; [ "$c" = S ] && h=11; total=$((total + h)); [ $i -lt $k ] && off=$((off + h)); done
    local bottom=$(( $(gy) + $(gh) - bar - 8 ))
    echo $(( bottom - total + off + 12 ))
}
ax=$(pos task alpha | cut -d' ' -f1)
click_at $ax $(menu_y RRSRSRRSRR 9)
ok_within "Close all windows closes every alpha window" 5 bash -c "! xdotool search --class '^alpha\$' >/dev/null 2>&1"
sleep 0.5
eq "menu closed after the action" "$(gh)" 48
click_zone 3 task alpha
# alpha closed but pinned: Alpha / sep / Unpin from taskbar / Open file location
ax=$(pos task alpha | cut -d' ' -f1)
click_at $ax $(menu_y RSRR 2)
sleep 0.6
python3 - "$PINNED" <<'PY' && pass "Unpin from taskbar saved the pinned file" || fail "Unpin" "$(cat "$PINNED")"
import json,sys
d=json.load(open(sys.argv[1])); assert "alpha" not in d["pinned"], d
PY
has_task alpha && fail "unpinned closed app leaves the taskbar" || pass "unpinned closed app leaves the taskbar"

echo "== dock menu: alignment and titles"
read -r ex ey <<< "$(pos task beta)"
ex=200   # empty part of the bar, left of the centred group
click_at $ex $ey 3
shot 15-dock-menu
# Dock Style: macOS / Dock Style: Windows / sep / Taskbar Alignment: Left / Show Window Titles / sep / Taskbar Settings
click_at $ex $(menu_y RRSRRSR 3)
sleep 0.8
grep -q '"windowsAlign": "left"' "$SETTINGS" && pass "menu wrote windowsAlign=left" || fail "alignment from the menu" "$(cat $SETTINGS)"
shot 16-windows-left
eq "left aligned: Start sits at the left pad" "$(( $(pos start | cut -d' ' -f1) - $(gx) ))" "$(( 12 + 24 ))"
eq "classic height" "$(gh)" 44
eq "classic strut" "$(strut)" "0,0,0,44,0,0,0,0,0,0,0,1279"
eq "workarea follows the new strut" "$(workarea | cut -d, -f4)" 756
"$FAKE" gamma "Gamma Window" cccc33 & PIDS+=($!)
ok_within "gamma window appears" 4 has_win gamma
sleep 0.6
setcfg '{"style":"windows","windowsAlign":"left","windowsLabels":"always"}'
sleep 1.4
shot 17-windows-left-titles
ok_within "titled mode lists the gamma window" 3 has_task gamma
setcfg '{"style":"windows","windowsAlign":"center","windowsLabels":"always"}'
sleep 1.2
shot 18-windows-center-titles
setcfg '{"style":"windows","windowsAlign":"center","windowsTheme":"light"}'
sleep 1.2
shot 19-windows-light
setcfg '{"style":"windows","windowsAlign":"left","windowsTheme":"light","windowsSearch":"button"}'
sleep 1.2
shot 20-windows-left-light
setcfg '{"style":"windows","windowsPosition":"top"}'
sleep 1.2
shot 21-windows-top
eq "top position: strut on the top edge" "$(strut)" "0,0,48,0,0,0,0,0,0,1279,0,0"
eq "top position: window at y 0" "$(gy)" 0

echo "== back to the macOS dock"
setcfg '{"style":"mac"}'
ok_within "dock is a pill again" 4 bash -c "[ \"\$(xdotool getwindowgeometry --shell $DOCKW | sed -n 's/^WIDTH=//p')\" -lt 1000 ]"
sleep 0.5
shot 22-back-to-mac
eq "mac strut restored" "$(strut)" "$mac_strut"
eq "workarea restored" "$(workarea)" "$mac_work"
echo "== switching from the dock's own menu"
# right click the Launchpad cell (4 icons: launchpad beta gamma trash): the menu ends with 'Dock Style: Windows'
COUNT=4
lx=$(( $(gx) + ($(gw) - (COUNT * 60 + (COUNT - 1) * 6 + 32)) / 2 + 16 - 5 + 30 )); ly=$(( $(gy) + $(gh) - 25 ))
click_at $lx $ly 3
shot 23-mac-menu
bottom=$(( $(gy) + $(gh) - 71 - 8 ))
click_at $lx $(( bottom - (12 + 24 + 11 + 24) + 6 + 24 + 11 + 12 ))
ok_within "menu item switches to the Windows style" 4 bash -c "[ \"\$(xdotool getwindowgeometry --shell $DOCKW | sed -n 's/^WIDTH=//p')\" = 1280 ]"
grep -q '"style": "windows"' "$SETTINGS" && pass "style saved to dock-settings.json" || fail "style not saved" "$(cat $SETTINGS)"

echo "== decbar can be hidden (bar.enabled)"
"$BIN/decbar" >"$TMP/decbar.log" 2>&1 &
BAR_PID=$!; PIDS+=($BAR_PID)
bar_win() { xdotool search --name '^decebal-bar$' 2>/dev/null | head -1; }
ok_within "decbar maps by default" 5 bar_win
printf '{"bar":{"enabled":false}}\n' > "$SHELLCFG"
ok_within "decbar unmaps when bar.enabled is false" 8 bash -c "! xdotool search --name '^decebal-bar\$' >/dev/null 2>&1"
kill -0 $BAR_PID 2>/dev/null && pass "decbar process stays alive (supervisor keeps happy)" || fail "decbar exited"
printf '{"bar":{"enabled":"auto"}}\n' > "$SHELLCFG"
setcfg '{"style":"mac"}'
ok_within "decbar returns for auto + macOS dock" 8 bar_win
setcfg '{"style":"windows"}'
ok_within "auto + Windows dock hides decbar" 8 bash -c "! xdotool search --name '^decebal-bar\$' >/dev/null 2>&1"
kill -0 $BAR_PID 2>/dev/null && pass "decbar still alive when hidden by auto" || fail "decbar exited"
shot 24-windows-only

echo
echo "$PASS passed, $FAIL failed (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
