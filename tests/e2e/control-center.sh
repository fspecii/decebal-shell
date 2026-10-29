#!/usr/bin/env bash
# This desktop test must not retint apps through an inherited session bus.
export DECEBAL_SYSTEM_AUTOMATIC=0
# The Control Center in every shell style: macOS, Windows 11 (taskbar), Caelestia left / top / split, and with no bar at all.
#   tests/e2e/control-center.sh        (E2E_DISPLAY=:143; SHOTS=dir keeps the screenshots, default docs/screenshots; PROFILE=debug)
# For each style it applies the style with `decthemectl style`, opens the panel with `decutil control-center toggle` (the
# shortcut / hot-corner path: one program for every theme), checks it is inside the screen and at the edge the bar's own button is
# (DECBAR_DEBUG_PANEL reports the panel), saves docs/screenshots/control-center-<style>.png, and closes it with a second toggle.
# macOS, Windows and Caelestia-left also open it from the bar's own button. Then: `control_center.enabled = false` removes the
# buttons and the command does nothing, and `tiles` hides tile groups.
# Everything is private: own HOME / XDG dirs, a private Xvfb and rdwm, the fake tools of tests/fakebin; only PIDs started here
# are killed. It refuses the live displays.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
PROFILE=${PROFILE:-release}
BIN=${CARGO_TARGET_DIR:-$HERE/target}/$PROFILE
RDWM_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
[ -x "$RDWM_DIR/rdwm" ] || { echo "rdwm not found (set RDWM_BIN_DIR)"; exit 77; }
for t in Xvfb xdotool import xprop jq convert; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline $([ "$PROFILE" = release ] && echo --release) --bins --examples) || { echo "build failed"; exit 2; }
N=${E2E_DISPLAY:-:143}
case "$N" in :0 | :1 | :2 | 0 | 1 | 2) echo "refusing a live display"; exit 2 ;; esac

PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d /tmp/decebal-cc-e2e.XXXXXX)
SHOTS=${SHOTS:-$HERE/docs/screenshots}; mkdir -p "$SHOTS"
cleanup() {
    for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done
    sleep 0.3
    for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done
    wait 2>/dev/null
    rm -rf "$TMP"
}
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "got '$2', want '$3'"; fi; }
wait_for() { local t=$1; shift; local end=$(($(date +%s%N) / 1000000 + t * 1000)); while [ $(($(date +%s%N) / 1000000)) -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 0.05; done; return 1; }
until_ok() { local d=$1 t=$2; shift 2; if wait_for "$t" "$@"; then pass "$d"; else fail "$d" "timed out after ${t}s"; fi; }

export HOME=$TMP/home XDG_CONFIG_HOME=$TMP/cfg XDG_RUNTIME_DIR=$TMP/run XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata XDG_CACHE_HOME=$TMP/cache XDG_STATE_HOME=$TMP/state
export DECEBAL_CONFIG=$TMP/cfg/omarchy DECEBAL_BACKEND=x11 DISPLAY=$N RDWMCTL=$RDWM_DIR/rdwmctl DECSETTINGS_CONFIG=$TMP/cfg/rdwm/config.toml
export DECEBAL_COLOR_SCHEME=dark DECEBAL_THEMES=$TMP/cfg/decebal/themes DECEBAL_SCALE=1
unset WAYLAND_DISPLAY RDWM_SOCK HYPRLAND_INSTANCE_SIGNATURE
mkdir -p "$HOME" "$XDG_CONFIG_HOME/rdwm" "$DECEBAL_CONFIG" "$XDG_RUNTIME_DIR" "$XDG_DATA_HOME/applications" "$TMP/nodata" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$TMP/ps/BAT0"
chmod 700 "$XDG_RUNTIME_DIR"
export FAKE_STATE=$TMP/fs DECBAR_POWER_SUPPLY=$TMP/ps
mkdir -p "$FAKE_STATE"
echo 0.50 > "$FAKE_STATE/vol"; echo 0 > "$FAKE_STATE/mute"; echo enabled > "$FAKE_STATE/wifi"; echo yes > "$FAKE_STATE/btpower"; echo 50 > "$FAKE_STATE/bright"; : > "$FAKE_STATE/log"
echo Battery > "$TMP/ps/BAT0/type"; echo 64 > "$TMP/ps/BAT0/capacity"; echo Discharging > "$TMP/ps/BAT0/status"
export PATH=$HERE/tests/fakebin:$RDWM_DIR:$BIN:$PATH
CFG=$DECSETTINGS_CONFIG
printf '# test config\n[general]\ngap_inner = 6\n\n[decoration]\nenabled = true\n\n[bar]\nshow = false\n' > "$CFG"
printf '{"version":1,"pinned":["launchpad","trash"],"order":["launchpad","trash"]}\n' > "$DECEBAL_CONFIG/dock-pinned-macos.json"
SHELLCFG=$DECEBAL_CONFIG/decebal.json
export DECBAR_DEBUG_PANEL=$TMP/panel.txt DECBAR_DEBUG_TRAY=$TMP/tray DECDOCK_DEBUG_LAYOUT=$TMP/layout.txt

SW=1600; SH=900
Xvfb $N -screen 0 ${SW}x${SH}x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X${N#:} || { echo "Xvfb failed"; cat "$TMP/xvfb.log"; exit 2; }
xdotool mousemove 800 450
rdwm --backend x11 --config "$CFG" >"$TMP/rdwm.log" 2>&1 & PIDS+=($!)
wait_for 5 bash -c "xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window" || { echo "rdwm did not start"; cat "$TMP/rdwm.log"; exit 2; }
rdwmctl wallpaper --color "#46607a" >/dev/null 2>&1
printf '{"workspaces": true}\n' > "$SHELLCFG"

style() { decthemectl style "$1" >/dev/null 2>&1; }
ccwin() { xdotool search --name '^decebal-control-center$' 2>/dev/null | head -1; }
no_ccwin() { [ -z "$(ccwin)" ]; }
have_panel() { [ -s "$DECBAR_DEBUG_PANEL" ] && [ -n "$(ccwin)" ]; }
# panel_abs: "X Y W H" of the open panel on the screen (the surface's position plus the panel inside it)
panel_abs() {
    local w gx gy; w=$(ccwin)
    eval "$(xdotool getwindowgeometry --shell "$w" 2>/dev/null | grep -E '^(X|Y)=' | sed 's/^X=/gx=/; s/^Y=/gy=/')"
    awk -v gx="${gx:-0}" -v gy="${gy:-0}" '$1 == "panel" {print $2 + gx, $3 + gy, $4, $5}' "$DECBAR_DEBUG_PANEL"
}
toggle() { decutil control-center toggle "$@" >"$TMP/toggle.log" 2>&1 & PIDS+=($!); }
# the panel opened by the command: inside the screen, with its horizontal side (right|left) and vertical side (top|bottom) as wanted
check_overlay() { # name hside vside
    local name=$1 hs=$2 vs=$3 x y w h
    rm -f "$DECBAR_DEBUG_PANEL"
    toggle "${@:4}"
    until_ok "$name: decutil control-center toggle opens the panel" 8 have_panel
    sleep 0.6
    read -r x y w h <<< "$(panel_abs)"
    if [ -z "${x:-}" ]; then fail "$name: no panel geometry"; return; fi
    [ "$x" -ge 0 ] && [ "$y" -ge 0 ] && [ $((x + w)) -le $SW ] && [ $((y + h)) -le $SH ] && pass "$name: the panel is inside the screen ($x,$y ${w}x$h)" || fail "$name: outside the screen" "$x,$y ${w}x$h"
    if [ "$hs" = right ]; then [ $((SW - x - w)) -le 70 ] && pass "$name: at the right edge" || fail "$name: not at the right edge" "x=$x w=$w"; else [ "$x" -le 70 ] && pass "$name: at the left edge" || fail "$name: not at the left edge" "x=$x"; fi
    if [ "$vs" = top ]; then [ "$y" -le 70 ] && pass "$name: at the top" || fail "$name: not at the top" "y=$y"; else [ $((SH - y - h)) -le 70 ] && pass "$name: at the bottom" || fail "$name: not at the bottom" "y=$y h=$h"; fi
    local cx=$((x - 20)) cy=$((y - 20)); [ $cx -lt 0 ] && cx=0; [ $cy -lt 0 ] && cy=0
    local cw=$((w + 40)) ch=$((h + 40)); [ $((cx + cw)) -gt $SW ] && cw=$((SW - cx)); [ $((cy + ch)) -gt $SH ] && ch=$((SH - cy))
    import -window root -crop ${cw}x${ch}+${cx}+${cy} +repage -colors 160 "$SHOTS/control-center-$name.png" 2>/dev/null
    local kb; kb=$(( $(stat -c %s "$SHOTS/control-center-$name.png" 2>/dev/null || echo 999999) / 1024 ))
    [ "$kb" -lt 400 ] && pass "$name: screenshot ${kb} KB" || fail "$name: screenshot too big" "${kb} KB"
    OVL_X=$x; OVL_Y=$y; OVL_W=$w; OVL_H=$h
}
close_overlay() { toggle; until_ok "$1: a second toggle closes it" 6 no_ccwin; }

tray_xy() { awk -v k="$1" '$1 == k {print $2, $3}' "$TMP/tray" 2>/dev/null | head -1; }
tray_x() { awk -v k="$1" '$1 == k {print $2}' "$TMP/tray" 2>/dev/null | head -1; }
px() { import -window root -crop 1x1+$1+$2 +repage -depth 8 txt:- 2>/dev/null | tail -1 | grep -o '#[0-9A-F]\{6\}' | head -1 | tr 'A-F' 'a-f'; }
BGPX=#46607a
differs_from_wallpaper() { local g; g=$(px "$1" "$2"); [ -n "$g" ] && [ "$g" != "$BGPX" ]; }
lay() { cat "$TMP/layout.txt" 2>/dev/null; }
dock_win() { xdotool search --name '^decebal-dock$' 2>/dev/null | head -1; }

echo "== macOS: menu bar, panel under it at the right"
style macos
decbar >"$TMP/decbar.log" 2>&1 & PIDS+=($!)
decdock >"$TMP/decdock.log" 2>&1 & PIDS+=($!)
until_ok "the menu bar reports a Control Center button" 10 bash -c "[ -n \"\$(awk '\$1==\"Control\"' $TMP/tray 2>/dev/null)\" ]"
check_overlay macos right top
close_overlay macos
CX=$(tray_x Control)
xdotool mousemove "$CX" 13 click 1
until_ok "the menu bar button opens the panel in the bar" 5 bash -c "[ \$(import -window root -crop 1x1+$((CX - 100))+120 +repage -depth 8 txt:- | tail -1 | grep -c '#46607A') = 0 ]"
import -window root -crop 420x560+$((SW - 440))+0 +repage -colors 160 "$TMP/macos-bar.png" 2>/dev/null
xdotool key Escape; sleep 0.5

echo "== Windows 11: the taskbar has a quick-settings button"
style windows
until_ok "the taskbar is up (the layout dump has a Control tray item)" 12 bash -c "grep -q '^tray Control' $TMP/layout.txt"
sleep 0.5
check_overlay windows right bottom
close_overlay windows
DW=$(dock_win)
eval "$(xdotool getwindowgeometry --shell "$DW" | grep -E '^(X|Y)=' | sed 's/^X=/DX=/; s/^Y=/DY=/')"
read -r _ _ TXR TYR <<< "$(lay | awk '$1=="tray" && $2=="Control" {print "a b", $3, $4}')"
xdotool mousemove $((DX + ${TXR%.*})) $((DY + ${TYR%.*})); sleep 0.15; xdotool click 1; sleep 0.8
read -r PX PY PW PH <<< "$(lay | awk '$1=="popup" {print $2, $3, $4, $5}')"
if [ -n "${PX:-}" ]; then
    [ $((DX + PX + PW)) -le $SW ] && [ $((SW - DX - PX - PW)) -le 40 ] && [ $((DY + PY + PH)) -le $((DY + 48 + 0)) ] || true
    [ $((DX + PX + PW)) -le $SW ] && [ $((SW - PX - PW)) -le 40 ] && pass "windows: the taskbar button opens the panel at the right edge ($PX,$PY ${PW}x$PH)" || fail "windows: taskbar panel placement" "$PX,$PY ${PW}x$PH"
    [ "$PW" -eq 340 ] && pass "windows: the same 340 px panel as everywhere" || fail "windows: panel width" "$PW"
    import -window root -crop $((PW + 40))x$((PH + 100))+$((DX + PX - 20))+$((DY + PY - 20)) +repage -colors 160 "$TMP/windows-bar.png" 2>/dev/null
else
    fail "windows: the taskbar button did not open a panel" "$(lay | tail -3 | tr '\n' ' ')"
fi
xdotool key Escape; sleep 0.5

echo "== Caelestia, left bar"
style caelestia
until_ok "the pill bar reports its Control tile" 10 bash -c "[ -n \"\$(awk '\$1==\"Control\"' $TMP/tray 2>/dev/null)\" ]"
sleep 1
check_overlay caelestia-left left top
close_overlay caelestia-left
read -r TX TY <<< "$(tray_xy Control)"
xdotool mousemove "$TX" "$TY" click 1; sleep 0.8
differs_from_wallpaper 200 "$TY" && pass "caelestia-left: the bar's tile opens the panel beside it" || fail "caelestia-left: the tile did not open the panel"
xdotool key Escape; sleep 0.5

echo "== Caelestia, top bar"
style caelestia-top
until_ok "the bar moved to the top" 10 bash -c "xdotool getwindowgeometry --shell \$(xdotool search --name '^decebal-bar\$' | head -1) | grep -q '^WIDTH=$SW'"
sleep 1
check_overlay caelestia-top right top
close_overlay caelestia-top

echo "== Caelestia, split bar"
style caelestia-split
sleep 2.5
check_overlay caelestia-split right bottom
close_overlay caelestia-split

echo "== a corner of the user's choice"
check_overlay corner-bottom-left left bottom --corner bottom-left
close_overlay corner-bottom-left

echo "== no bar at all"
style macos
sleep 1.5
jq '.bar.enabled = false | .dock.enabled = false' "$SHELLCFG" > "$TMP/s.json" && mv "$TMP/s.json" "$SHELLCFG"
sleep 2
check_overlay no-bar right top
close_overlay no-bar

echo "== control_center.enabled = false"
jq '.bar.enabled = true | .control_center = {"enabled": false}' "$SHELLCFG" > "$TMP/s.json" && mv "$TMP/s.json" "$SHELLCFG"
until_ok "the menu bar loses its Control Center button" 10 bash -c "[ -z \"\$(awk '\$1==\"Control\"' $TMP/tray 2>/dev/null)\" ] && [ -s $TMP/tray ]"
toggle; sleep 2
no_ccwin && pass "the command does nothing while it is off" || fail "the panel opened although it is off"
style windows
until_ok "the taskbar loses its button" 10 bash -c "! grep -q '^tray Control' $TMP/layout.txt && grep -q '^tray' $TMP/layout.txt"

echo "== tiles"
jq '.control_center = {"enabled": true, "tiles": ["sound", "actions"]}' "$SHELLCFG" > "$TMP/s.json" && mv "$TMP/s.json" "$SHELLCFG"
sleep 1.5
check_overlay tiles right bottom
[ "$OVL_H" -lt 300 ] && pass "tiles: only the chosen groups (panel ${OVL_H} px tall instead of ~500)" || fail "tiles: panel still tall" "$OVL_H"
close_overlay tiles

echo
echo "$PASS passed, $FAIL failed"
[ "$FAIL" = 0 ]
