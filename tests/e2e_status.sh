#!/bin/bash
# End-to-end check of the bar's status items on a private Xvfb with a real rdwm and fake system tools.
# Usage: tests/e2e_status.sh [display-number]   (default 92; needs Xvfb, xdotool, rdwm, rdwmctl, import)
# Env: DECBAR (binary), RDWM, RDWMCTL, SHOTS=dir to keep screenshots.
# Never touches the live session: own X display, own XDG_RUNTIME_DIR, the tools on PATH are fakes.
set -u
D=${1:-92}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
BAR=${DECBAR:-$ROOT/target/release/decbar}
RDWM=${RDWM:-rdwm}; RDWMCTL=${RDWMCTL:-rdwmctl}
T=$(mktemp -d); XR=$T/xr; mkdir -p $XR $T/ps/BAT0; chmod 700 $XR
export FAKE_STATE=$T/fs; mkdir -p $FAKE_STATE
echo 0.50 > $FAKE_STATE/vol; echo 0 > $FAKE_STATE/mute; echo enabled > $FAKE_STATE/wifi; echo yes > $FAKE_STATE/btpower; echo 50 > $FAKE_STATE/bright; : > $FAKE_STATE/log
echo Battery > $T/ps/BAT0/type; echo 64 > $T/ps/BAT0/capacity; echo Discharging > $T/ps/BAT0/status
unset WAYLAND_DISPLAY; export DISPLAY=:$D XDG_RUNTIME_DIR=$XR HOME=$T XDG_CONFIG_HOME=$T/cfg; mkdir -p $T/cfg
PIDS=()
cleanup() { [ -n "${SHOTS:-}" ] && cp $T/bar.log $T/rdwm.log $SHOTS/ 2>/dev/null; for p in "${PIDS[@]}"; do kill $p 2>/dev/null; done; rm -rf $T; }
trap cleanup EXIT
Xvfb :$D -screen 0 1600x900x24 >/dev/null 2>&1 & PIDS+=($!)
sleep 1
$RDWM --backend x11 >$T/rdwm.log 2>&1 & PIDS+=($!)
sleep 1.5
DECBAR_DEBUG_TRAY=$T/tray PATH=$ROOT/tests/fakebin:$PATH DECBAR_POWER_SUPPLY=$T/ps $BAR >$T/bar.log 2>&1 & PIDS+=($!)
for i in $(seq 40); do [ -s $T/tray ] && grep -q Layout $T/tray && break; sleep 0.25; done
sleep 2   # late data (weather, layout) shifts the right-aligned items; let them settle
fail=0
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fail=1; fi; }
layout() { $RDWMCTL state | grep -o '"layout":"[a-z]*"' | head -1 | cut -d'"' -f4; }
tx() { awk -v k=$1 '$1==k {print $2}' $T/tray; }
shot() { [ -n "${SHOTS:-}" ] && import -window root $SHOTS/$1.png 2>/dev/null; }
wait_for() { for i in $(seq 20); do eval "$1" && return 0; sleep 0.25; done; return 1; }

check "tray lists layout, battery, bluetooth, audio, network" 'for k in Layout Battery Bluetooth Audio Network Clock; do [ -n "$(tx $k)" ] || exit 1; done'

# layout: click the item, choose the third row (monocle)
xdotool mousemove $(tx Layout) 12 click 1; sleep 0.6; shot layout-popup
xdotool mousemove $(( $(tx Layout) + 20 )) $(( 28 + 5 + 24 + 2*22 + 11 )) click 1
check "picking a layout row applies it through rdwm" 'wait_for "[ \"$(layout)\" = monocle ]"'
# change by "hotkey" (rdwmctl): the bar item follows live
$RDWMCTL action setlayout bsp >/dev/null; sleep 0.8; shot layout-after-hotkey
xdotool mousemove $(tx Layout) 12 click 1; sleep 0.6; shot layout-popup-bsp; xdotool key Escape; sleep 0.3
xdotool mousemove $(tx Layout) 12 click 5; sleep 0.5
check "wheel over the layout item cycles layouts" '[ "$(layout)" != bsp ]'

# volume: wheel, mute, external change
xdotool mousemove $(tx Audio) 12; sleep 0.2; xdotool click 4 click 4; sleep 0.8
check "wheel up over sound raises the volume" '[ "$(cat $FAKE_STATE/vol)" = 0.60 ]'
xdotool click 2; sleep 0.6
check "middle click mutes" '[ "$(cat $FAKE_STATE/mute)" = 1 ]'
xdotool click 2; sleep 0.4
echo 0.15 > $FAKE_STATE/vol; touch $FAKE_STATE/evt; sleep 1
xdotool click 1; sleep 0.6; shot sound-popup
xdotool key Escape; sleep 0.3

# wifi and bluetooth popups
xdotool mousemove $(tx Network) 12 click 1; sleep 1.5; shot wifi-popup
xdotool key Escape; sleep 0.3
xdotool mousemove $(tx Bluetooth) 12 click 1; sleep 1.5; shot bluetooth-popup
xdotool key Escape; sleep 0.3
xdotool mousemove $(tx Battery) 12 click 1; sleep 0.6; shot battery-popup
xdotool key Escape; sleep 0.3
# ---- popups stay open while the pointer travels from the icon to the popup (user bug: "closes when I go down to the slider")
pix() { import -window root -crop 1x1+$1+$2 txt:- 2>/dev/null | tail -1 | awk '{print $3}'; }
vol() { cat $FAKE_STATE/vol; }
echo 0.50 > $FAKE_STATE/vol; echo 0 > $FAKE_STATE/mute; : > $FAKE_STATE/log
xdotool mousemove $(tx Audio) 12 click 1; sleep 0.6
for i in $(seq 31); do xdotool mousemove_relative -- 0 2; sleep 0.02; done   # y 12 -> 74, the slider row
shot sound-travel
xdotool mousemove_relative -- -60 0; sleep 0.05; xdotool click 1; sleep 0.6
check "volume popup stays open while moving from the icon down to the slider; a click on the track sets it" '[ "$(vol)" != 0.50 ]'
shot sound-after-click
# a drag that leaves the popup keeps tracking and does not close it
xdotool mousedown 1; for i in $(seq 25); do xdotool mousemove_relative -- 8 6; sleep 0.02; done; xdotool mouseup 1; sleep 0.4
check "dragging the slider outside the popup still follows the pointer" '[ "$(vol)" = 1.00 ] || [ "$(vol)" = 1.0 ]'
# wandering out for a moment and back (under the close delay) keeps it open
xdotool mousemove $(tx Audio) 12 click 1; sleep 0.5
echo 0.50 > $FAKE_STATE/vol
xdotool mousemove $(tx Audio) 74; sleep 0.1; xdotool mousemove 20 400; sleep 0.12; xdotool mousemove $(( $(tx Audio) - 40 )) 74; sleep 0.2; xdotool click 1; sleep 0.5
check "briefly leaving the popup and coming back does not close it" '[ "$(vol)" != 0.50 ]'
xdotool mousemove 20 400; sleep 0.9; shot sound-closed-after-delay
echo 0.50 > $FAKE_STATE/vol; xdotool mousemove $(tx Audio) 74 click 1; sleep 0.4
check "after the close delay the popup is gone (click does nothing)" '[ "$(vol)" = 0.50 ]'
# clicking the icon again toggles, Escape closes
xdotool mousemove $(tx Audio) 12 click 1; sleep 0.5; xdotool click 1; sleep 0.5
xdotool mousemove $(tx Audio) 74 click 1; sleep 0.4
check "clicking the icon again closes the popup" '[ "$(vol)" = 0.50 ]'
xdotool mousemove $(tx Audio) 12 click 1; sleep 0.5; xdotool key Escape; sleep 0.4
xdotool mousemove $(tx Audio) 74 click 1; sleep 0.4
check "Escape closes the popup" '[ "$(vol)" = 0.50 ]'
# the same travel for every popup: a row far down is reachable
for k in Network Bluetooth Layout Clock Battery; do
  x=$(tx $k); xdotool mousemove 20 500; sleep 0.4; empty=$(pix $x 60)
  xdotool mousemove $x 12 click 1; sleep 1.2
  for i in $(seq 12); do xdotool mousemove_relative -- 0 3; sleep 0.02; done
  sleep 0.1; shot popup-travel-$k
  check "$k popup still open after moving down from its icon" '[ "$(pix $x 60)" != "$empty" ]'
  xdotool key Escape; sleep 0.3
done

xdotool mousemove $(tx Clock) 12 click 1; sleep 0.6; shot calendar-popup
xdotool mousemove 400 600; sleep 0.5; shot popup-closed-on-leave
check "no crash" 'kill -0 ${PIDS[2]}'
[ $fail = 0 ] && echo "e2e passed" || echo "e2e FAILED"
exit $fail
