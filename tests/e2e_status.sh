#!/bin/bash
# End-to-end check of the bar's status items on a private Xvfb with a real rdwm and fake system tools.
# Usage: tests/e2e_status.sh [display-number]   (default 92; needs Xvfb, xdotool, rdwm, rdwmctl, import)
# Env: MACBAR (binary), RDWM, RDWMCTL, SHOTS=dir to keep screenshots.
# Never touches the live session: own X display, own XDG_RUNTIME_DIR, the tools on PATH are fakes.
set -u
D=${1:-92}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
BAR=${MACBAR:-$ROOT/target/release/macbar}
RDWM=${RDWM:-rdwm}; RDWMCTL=${RDWMCTL:-rdwmctl}
T=$(mktemp -d); XR=$T/xr; mkdir -p $XR $T/ps/BAT0; chmod 700 $XR
export FAKE_STATE=$T/fs; mkdir -p $FAKE_STATE
echo 0.50 > $FAKE_STATE/vol; echo 0 > $FAKE_STATE/mute; echo enabled > $FAKE_STATE/wifi; echo yes > $FAKE_STATE/btpower; echo 50 > $FAKE_STATE/bright; : > $FAKE_STATE/log
echo Battery > $T/ps/BAT0/type; echo 64 > $T/ps/BAT0/capacity; echo Discharging > $T/ps/BAT0/status
unset WAYLAND_DISPLAY; export DISPLAY=:$D XDG_RUNTIME_DIR=$XR
PIDS=()
cleanup() { for p in "${PIDS[@]}"; do kill $p 2>/dev/null; done; rm -rf $T; }
trap cleanup EXIT
Xvfb :$D -screen 0 1600x900x24 >/dev/null 2>&1 & PIDS+=($!)
sleep 1
$RDWM --backend x11 >$T/rdwm.log 2>&1 & PIDS+=($!)
sleep 1.5
MACBAR_DEBUG_TRAY=$T/tray PATH=$ROOT/tests/fakebin:$PATH MACBAR_POWER_SUPPLY=$T/ps $BAR >$T/bar.log 2>&1 & PIDS+=($!)
for i in $(seq 40); do [ -s $T/tray ] && grep -q Layout $T/tray && break; sleep 0.25; done
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
xdotool mousemove $(tx Clock) 12 click 1; sleep 0.6; shot calendar-popup
xdotool mousemove 400 600; sleep 0.5; shot popup-closed-on-leave
check "no crash" 'kill -0 ${PIDS[2]}'
[ $fail = 0 ] && echo "e2e passed" || echo "e2e FAILED"
exit $fail
