#!/usr/bin/env bash
# End-to-end test of the system panes (sound, network, bluetooth, displays, input, ...) on a private Xvfb with
# rdwm and FAKE tools on PATH (tests/e2e/fakes): nothing touches the real system. Screenshots go to $SHOTS.
# Usage: tests/e2e/macsettings_system.sh [display]   (default :146)
set -u
OLDHOME=$HOME; export OLDHOME
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
HERE=$(pwd)
source tests/e2e/env.sh
cleanup() { [ -n "${SHOTS:-}" ] && { mkdir -p "$SHOTS"; cp "$E2E_T/macsettings.log" "$SHOTS/macsettings.log" 2>/dev/null; cp "$FAKE_STATE/log" "$SHOTS/fake-tools.log" 2>/dev/null; cp -r "$E2E_T/cfg/macshell" "$SHOTS/session-files" 2>/dev/null; }; e2e_down; }
trap cleanup EXIT
e2e_up "${1:-:146}"
export FAKE_STATE="$E2E_T/fake"; mkdir -p "$FAKE_STATE"; : > "$FAKE_STATE/log"
export PATH="$HERE/tests/e2e/fakes:$PATH"
export MACSETTINGS_CONFIRM_SECS=4
# a private application list for the Default Apps pane
export XDG_DATA_HOME="$E2E_T/share"; export XDG_DATA_DIRS="$E2E_T/share2"; mkdir -p "$XDG_DATA_HOME/applications" "$XDG_DATA_DIRS/applications"
mkapp() { printf '[Desktop Entry]\nType=Application\nName=%s\nMimeType=%s\nExec=%s\n' "$2" "$3" "$1" > "$XDG_DATA_HOME/applications/$1.desktop"; }
mkapp firefox Firefox "text/html;x-scheme-handler/http;x-scheme-handler/https;"
mkapp chromium Chromium "text/html;x-scheme-handler/http;x-scheme-handler/https;"
mkapp mpv mpv "video/mp4;video/webm;audio/mpeg;"
mkapp thunar Thunar "inode/directory;"
DUMP="$E2E_T/dump.json"; export MACSETTINGS_DEBUG_LAYOUT="$DUMP"
SESS="$E2E_T/cfg/macshell"
PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); echo "  ok   $*"; }
bad()  { FAIL=$((FAIL+1)); echo "  FAIL $*"; }
check() { local d=$1; shift; if "$@"; then ok "$d"; else bad "$d"; fi; }
q() { jq -r "$1" "$DUMP" 2>/dev/null; }
wait_for() { local t=${3:-5} i=0; while [ $i -lt $((t*10)) ]; do [ "$(q "$2")" = "true" ] && { ok "$1"; return 0; }; sleep 0.1; i=$((i+1)); done; bad "$1 (waited ${t}s; pane=$(q .pane) notice=$(q .notice))"; return 1; }
wait_log() { local t=${3:-5} i=0; while [ $i -lt $((t*10)) ]; do grep -qE -- "$2" "$FAKE_STATE/log" && { ok "$1"; return 0; }; sleep 0.1; i=$((i+1)); done; bad "$1 (log lacks /$2/)"; return 1; }
has() { [ "$(q "[.ctls[]|select(.id==\"$1\")]|length")" -ge 1 ]; }
wait_has() { local i=0; while [ $i -lt 60 ]; do has "$2" && { ok "$1"; return 0; }; sleep 0.1; i=$((i+1)); done; bad "$1 (no control $2; pane=$(q .pane))"; return 1; }
wait_gone() { local i=0; while [ $i -lt ${3:-100} ]; do has "$2" || { ok "$1"; return 0; }; sleep 0.1; i=$((i+1)); done; bad "$1 (control $2 still there)"; return 1; }
center() { jq -r "[.ctls[]|select(.id==\"$1\")][0].rect|\"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)\"" "$DUMP"; }
click_xy() { [ $# -ge 2 ] || { bad "click without coordinates"; return 1; }; xdotool mousemove --window "$WID" "$1" "$2"; sleep 0.05; xdotool click --window "$WID" 1; sleep 0.3; }
click_id() { local c; c=$(center "$1"); { [ -z "$c" ] || [ "$c" = "null null" ]; } && { bad "no control $1"; return 1; }; click_xy $c; }
click_part() { local r; r=$(jq -r "[.ctls[]|select(.id==\"$1\")][0].parts[$2]|\"\(.[0]+.[2]*${3:-0.5}|floor) \(.[1]+.[3]/2|floor)\"" "$DUMP"); click_xy $r; }
pick() { # popup id, typed text: opens the popup, type-ahead, Enter
  click_id "$1" || return 1; sleep 0.3; xdotool type --window "$WID" --delay 40 "$2"; sleep 0.2; xdotool key --window "$WID" Return; sleep 0.5; }
goto() {
  local n=0 c
  clear_search
  while [ "$(jq -r "[.panes[]|select(.id==\"$1\")][0].visible" "$DUMP")" != "true" ] && [ $n -lt 40 ]; do
    xdotool mousemove --window "$WID" 100 300
    if [ "$(jq -r "[.panes[]|select(.id==\"$1\")][0].rect[1] < 300" "$DUMP")" = "true" ]; then xdotool click --window "$WID" 4; else xdotool click --window "$WID" 5; fi
    sleep 0.12; n=$((n+1))
  done
  c=$(jq -r "[.panes[]|select(.id==\"$1\")][0].rect|\"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)\"" "$DUMP"); click_xy $c; wait_for "pane $1 shown" ".pane==\"$1\"" 3; sleep 0.6; }
clear_search() { local n; n=$(q '.search_text|length'); [ "${n:-0}" -gt 0 ] && { for _ in $(seq $n); do xdotool key --window "$WID" BackSpace; done; sleep 0.8; }; }
scrollto() { # scroll the content until the control is on screen
  local n=0 y
  while [ $n -lt 25 ]; do
    y=$(jq -r "[.ctls[]|select(.id==\"$1\")][0].rect|\"\(.[1]|floor) \(.[3]|floor)\"" "$DUMP")
    set -- "$1" $y
    if [ "$2" -gt 900 ] 2>/dev/null; then xdotool mousemove --window "$WID" 900 500; xdotool click --window "$WID" 5; elif [ "$2" -lt 60 ] 2>/dev/null; then xdotool mousemove --window "$WID" 900 500; xdotool click --window "$WID" 4; else break; fi
    sleep 0.2; n=$((n+1))
  done
}
shot() { [ -n "${SHOTS:-}" ] && { mkdir -p "$SHOTS"; e2e_shot "$SHOTS/$1.png"; }; }
loglines() { grep -c -- "$1" "$FAKE_STATE/log"; }

echo "== start on the Sound pane"
e2e_start_app sound
WID=$(xdotool search --sync --class MacSettings 2>/dev/null | head -1)
check "window found" test -n "$WID"
wait_for "sound pane" '.pane=="sound"' 5
wait_has "output slider appears (loaded off the UI thread)" s:snd:vol:fake_speakers
shot sound
click_part s:snd:vol:fake_speakers 0 0.8
wait_log "volume change runs pactl set-sink-volume" 'pactl set-sink-volume fake_speakers [0-9]+%'
click_id s:snd:mute:fake_speakers
wait_log "mute runs pactl set-sink-mute ... 1" 'pactl set-sink-mute fake_speakers 1'
wait_has "undo offered" s:undo
click_id s:undo
wait_log "undo unmutes" 'pactl set-sink-mute fake_speakers 0'
pick s:snd:default:out "HDMI"
wait_log "default output changes" 'pactl set-default-sink fake_hdmi'
click_part s:snd:vol-app:77 0 0.2
wait_log "per-app volume" 'pactl set-sink-input-volume 77 [0-9]+%'
shot sound-after

echo "== search over settings"
xdotool key --window "$WID" ctrl+f; sleep 0.2; xdotool type --window "$WID" --delay 40 "volume"; sleep 1.5
check "search 'volume' lists Sound first ($(q '.panes|map(.id)|join(",")'))" test "$(q '.panes[0].id')" = "sound"
shot search-volume
clear_search; xdotool type --window "$WID" --delay 40 "night"; sleep 1.5
check "search 'night' finds Displays" test "$(q '.panes[0].id')" = "displays"
clear_search

echo "== network"
goto network
wait_has "wifi toggle" s:net:wifi:
shot network
click_id s:net:join:CafeFree
wait_log "open network joins directly" 'nmcli device wifi connect CafeFree$'
click_id s:net:join:Neighbor:5G
wait_has "password sheet opens for a secured network" s:net:pw:secret
shot network-password
click_id s:net:pw:secret; sleep 0.2
xdotool type --window "$WID" --delay 40 "wrongpassword"; xdotool key --window "$WID" Return; sleep 0.3
click_id s:net:joinpw:
wait_log "join with password" 'nmcli device wifi connect Neighbor:5G password wrongpassword'
wait_for "failure shown inline" '(.notice|tostring|test("Secrets were required"))' 5
shot network-error
click_id s:net:wifi:
wait_log "wifi off" 'nmcli radio wifi off'
sleep 0.8
check "available networks hidden while wifi is off" bash -c "! grep -q 'join:CafeFree' <(jq -r '.ctls[].id' $DUMP)"
click_id s:net:wifi:
wait_log "wifi on" 'nmcli radio wifi on'
sleep 0.5
click_id s:net:vpn:cccc-3333
wait_log "vpn up" 'nmcli connection up uuid cccc-3333'

echo "== bluetooth"
goto bluetooth
wait_has "bluetooth devices" s:bt:connect:AA:BB:CC:DD:EE:FF
shot bluetooth
click_id s:bt:connect:AA:BB:CC:DD:EE:FF
wait_log "connect" 'bluetoothctl connect AA:BB:CC:DD:EE:FF'
click_id s:bt:scan:
wait_log "scan" 'bluetoothctl --timeout 8 scan on'
click_id s:bt:power:
wait_log "power off" 'bluetoothctl power off'
sleep 1
check "device list hidden while powered off" bash -c "! grep -q 'bt:connect' <(jq -r '.ctls[].id' $DUMP)"
shot bluetooth-off

echo "== displays: keep / revert"
goto displays
wait_has "display controls" s:disp:rate:DP-1
shot displays
pick s:disp:rate:DP-1 "120"
wait_log "rate change runs xrandr --rate 120.00" 'xrandr --output DP-1 --mode 2560x1440 --rate 120.00'
wait_has "keep banner appears" s:disp:keep:
shot displays-confirm
wait_gone "banner disappears by itself" s:disp:keep: 120
wait_log "automatic revert restores 143.97 Hz" 'xrandr --output DP-1 --mode 2560x1440 --rate 143.97 --pos 0x0'
check "layout is back (fake state)" grep -q '^DP-1 1 2560 1440 0 0 143.97' "$FAKE_STATE/xr.state"
check "nothing persisted for a reverted change" bash -c "! test -s $SESS/session-settings.json || ! jq -e '.xrandr|length>0' $SESS/session-settings.json >/dev/null"
pick s:disp:rate:DP-1 "120"
wait_has "banner again" s:disp:keep:
click_id s:disp:keep:
sleep 1.2
check "kept change persisted to the session file" bash -c "jq -e '.xrandr|index(\"120.00\")' $SESS/session-settings.json >/dev/null"
check "session .sh applies it at login" grep -q 'xrandr --output DP-1 --mode 2560x1440 --rate 120.00' "$SESS/session-settings.sh"
pick s:disp:rot:HDMI-1 "90° clock"
wait_has "banner for rotation" s:disp:revert:
click_id s:disp:revert:
wait_gone "revert button works" s:disp:revert: 30
sleep 0.6
check "rotation reverted" grep -q '^HDMI-1 1 1920 1080 2560 0 60.00 normal' "$FAKE_STATE/xr.state"
click_id s:disp:mirror:
wait_has "banner for mirroring" s:disp:keep:
wait_log "mirror runs --same-as" 'xrandr --output DP-1 --mode 1920x1080 --pos 0x0 --rotate normal --output HDMI-1 --mode 1920x1080 --same-as DP-1'
click_id s:disp:keep:
sleep 1.2
pick s:disp:scale: "150"
sleep 0.8
check "scale wrote rdwm [decoration] scale" grep -q 'scale = 1.5' "$E2E_CFG"
check "scale sets Xft.dpi in the session" bash -c "jq -e '.xft_dpi==144' $SESS/session-settings.json >/dev/null"
wait_has "night shift offered (gammastep on PATH)" s:disp:night:
scrollto s:disp:night:
click_id s:disp:night:
wait_log "night shift on" 'gammastep -P -O 4000'
shot displays-final

echo "== keyboard & input"
goto inputdev
wait_has "layout popup" s:inp:layout:
shot input
pick s:inp:caps: "Control"
wait_log "caps lock as control" 'setxkbmap -layout us -variant  -option  -option ctrl:nocaps'
check "persisted in session-settings.json" bash -c "jq -e '.kbd_options|index(\"ctrl:nocaps\")' $SESS/session-settings.json >/dev/null"
check "persisted in session-settings.sh" grep -q 'setxkbmap -layout us' "$SESS/session-settings.sh"
click_part s:inp:delay: 0 0.1
wait_log "key repeat" 'xset r rate [0-9]+ 25'
click_id s:inp:tap:11
wait_log "tap to click via xinput" 'xinput set-prop 11 libinput Tapping Enabled 0'
check "pointer setting persisted for next login" grep -q "xinput --set-prop 'SYNA Touchpad' 'libinput Tapping Enabled' 0" "$SESS/session-settings.sh"
pick s:inp:layout: "de"
wait_log "layout change" 'setxkbmap -layout de'
shot input-after

echo "== notifications"
goto notifications
wait_has "dnd toggle" s:not:dnd:
click_id s:not:dnd:
sleep 0.5
check "notifications.toml has dnd = true" grep -q 'dnd = true' "$SESS/notifications.toml"
click_id s:not:newapp:; sleep 0.2; xdotool type --window "$WID" --delay 40 "Slack"; xdotool key --window "$WID" Return; sleep 0.3
click_id s:not:mute:
sleep 0.5
check "muted app stored" grep -q 'muted_apps = \["Slack"\]' "$SESS/notifications.toml"
click_id s:not:test:
wait_log "test notification" 'notify-send -a System Settings'
shot notifications

echo "== power"
goto power
wait_has "profile control" s:pwr:profile:
click_part s:pwr:profile: 2 0.5
wait_log "power profile" 'powerprofilesctl set power-saver'
pick s:pwr:blank: "5 min"
wait_log "screen saver timeout" 'xset s 300 300'
check "persisted" bash -c "jq -e '.blank_secs==300' $SESS/session-settings.json >/dev/null"
shot power

echo "== default apps"
goto defaults
wait_has "browser popup" s:def:set:browser
shot defaults
pick s:def:set:browser "Chromium"
wait_log "browser via xdg-settings" 'xdg-settings set default-web-browser chromium.desktop'
wait_has "terminal control" s:def:term:
shot defaults-after

echo "== date & time"
goto datetime
wait_has "zone popup" s:dt:zone:
pick s:dt:zone: "Asia/Tokyo"
wait_log "timezone set" 'timedatectl set-timezone Asia/Tokyo'
touch "$FAKE_STATE/polkit_deny"
pick s:dt:zone: "UTC"
wait_for "polkit refusal shown inline, no password prompt" '(.notice|tostring|test("administrator"))' 5
shot datetime-error
click_id s:dt:hour12:
sleep 0.6
check "macbar clock setting written" bash -c "jq -e '.clockHour12==true' $E2E_T/cfg/omarchy/macshell.json >/dev/null"
pick s:dt:datefmt: "26"
sleep 0.6
check "date style written" bash -c "jq -e '.clockDate==\"day-month\"' $E2E_T/cfg/omarchy/macshell.json >/dev/null"

echo "== users, privacy, storage, accessibility"
goto users; sleep 0.6; shot users
check "users pane shows the account" test "$(q '[.ctls[]|select(.id=="lbl:home")]|length')" = "1"
goto privacy; shot privacy
goto storage
wait_has "check config button" s:sto:check:
shot storage
scrollto s:sto:check:
click_id s:sto:check:
wait_for "Check config reports" '(.notice|tostring|test("Configuration"))' 5
goto accessibility
wait_has "cursor size" s:acc:cursor:
pick s:acc:cursor: "48"
sleep 0.6
check "cursor size exported at login" grep -q 'export XCURSOR_SIZE=48' "$SESS/session-settings.sh"
click_id s:acc:motion:
sleep 0.5
check "reduce motion exported" grep -q 'MACSHELL_REDUCE_MOTION=1' "$SESS/session-settings.sh"
shot accessibility

echo "== the old panes still work"
goto tiling
wait_for "tiling pane" '.pane=="tiling"' 3
goto displays
check "rdwm part of Displays still there" test "$(q '[.ctls[]|select(.id|startswith("t:output") or startswith("r:wallpaper"))]|length')" -ge 0
shot displays-bottom

echo "== resources"
APP=$(pgrep -n -x macsettings)
for p in general appearance desktop wallpaper tiling windows rules keyboard autostart displays sound network bluetooth notifications inputdev power defaults datetime users privacy storage accessibility; do goto $p; done
sleep 3
RSS=$(awk '/VmRSS/{print int($2/1024)}' /proc/$APP/status)
THREADS=$(ls /proc/$APP/task | wc -l)
echo "  RSS ${RSS} MB, threads $THREADS"
check "RSS under 30 MB after visiting every pane" test "$RSS" -lt 30
[ -n "${SHOTS:-}" ] && echo "RSS ${RSS} MB, threads $THREADS" > "$SHOTS/rss.txt"
echo "== no polling while a polled pane is not visible"
goto storage; sleep 1
N0=$(wc -l < "$FAKE_STATE/log"); sleep 8; N1=$(wc -l < "$FAKE_STATE/log")
check "no external commands while on a pane that needs no polling ($((N1-N0)) in 8 s)" test $((N1-N0)) -le 0
goto sound; sleep 1
N0=$(loglines 'pactl -f json list sinks'); sleep 8; N1=$(loglines 'pactl -f json list sinks')
check "sound pane polls sparingly ($((N1-N0)) polls in 8 s, every 3 s)" bash -c "test $((N1-N0)) -ge 1 -a $((N1-N0)) -le 4"
echo "== window hidden: polling stops"
xdotool windowunmap "$WID"; sleep 1.5
N0=$(loglines 'pactl -f json list sinks'); sleep 8; N1=$(loglines 'pactl -f json list sinks')
check "no polling while unmapped ($((N1-N0)))" test $((N1-N0)) -le 1
xdotool windowmap "$WID"; sleep 4
N2=$(loglines 'pactl -f json list sinks')
check "polling resumes when shown again" test "$N2" -gt "$N1"

echo
echo "passed $PASS, failed $FAIL"
[ "$FAIL" -eq 0 ]
