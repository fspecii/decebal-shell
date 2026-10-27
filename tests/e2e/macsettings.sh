#!/usr/bin/env bash
# End-to-end test of macsettings against a real rdwm (x11 backend) on a private Xvfb.
# Requires: Xvfb xdotool xprop jq import(optional, for screenshots). Build first:
#   cargo build   (macshell)   and   cargo build   in ~/Projects/rdwm-settings
# Usage: tests/e2e/macsettings.sh [display]   (default :95; screenshots go to $SHOTS if set)
set -u
OLDHOME=$HOME; export OLDHOME
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
source tests/e2e/env.sh
trap e2e_down EXIT
e2e_up "${1:-:95}"
DUMP="$E2E_T/dump.json"
export MACSETTINGS_DEBUG_LAYOUT="$DUMP"
PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); echo "  ok   $*"; }
bad()  { FAIL=$((FAIL+1)); echo "  FAIL $*"; }
check() { local d=$1; shift; if "$@"; then ok "$d"; else bad "$d"; fi; }
q() { jq -r "$1" "$DUMP" 2>/dev/null; }
wait_for() { # desc, jq-expr (true/false), [timeout s]
  local t=${3:-4}; local i=0
  while [ $i -lt $((t*10)) ]; do [ "$(q "$2")" = "true" ] && { ok "$1"; return 0; }; sleep 0.1; i=$((i+1)); done
  bad "$1 (waited ${t}s; dump: $(q '[.pane, .notice, (.warnings|length)]|tostring'))"; return 1
}
wait_file() { local t=${3:-4} i=0; while [ $i -lt $((t*10)) ]; do grep -q -- "$2" "$E2E_CFG" && { ok "$1"; return 0; }; sleep 0.1; i=$((i+1)); done; bad "$1 (file lacks $2; notice=$(q .notice) warnings=$(q ".warnings|tostring"))"; return 1; }
rdwm_cfg() { rdwmctl config | jq -r "$1"; }
center() { jq -r "[.ctls[]|select(.id==\"$1\")][0].rect|\"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)\"" "$DUMP"; }
click_xy() { xdotool mousemove --window "$WID" "$1" "$2"; sleep 0.05; xdotool click --window "$WID" 1; sleep 0.25; }
click_id() { local c; c=$(center "$1"); [ -z "$c" ] || [ "$c" = "null null" ] && { bad "no control $1"; return 1; }; click_xy $c; }
click_part() { # id part-index fraction-of-part(0..1)
  local r; r=$(jq -r "[.ctls[]|select(.id==\"$1\")][0].parts[$2]|\"\(.[0]+.[2]*${3:-0.5}|floor) \(.[1]+.[3]/2|floor)\"" "$DUMP"); click_xy $r; }
goto() { # scrolls the sidebar until the pane row is visible, then clicks it
  local n=0 c
  while [ "$(jq -r "[.panes[]|select(.id==\"$1\")][0].visible" "$DUMP")" != "true" ] && [ $n -lt 20 ]; do xdotool mousemove --window "$WID" 100 300; xdotool click --window "$WID" 5; sleep 0.15; n=$((n+1)); done
  c=$(jq -r "[.panes[]|select(.id==\"$1\")][0].rect|\"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)\"" "$DUMP"); click_xy $c; wait_for "pane $1 shown" ".pane==\"$1\"" 3; sleep 0.4; }
shot() { [ -n "${SHOTS:-}" ] && { mkdir -p "$SHOTS"; e2e_shot "$SHOTS/$1.png"; }; }
cpu_ticks() { awk '{print $14+$15}' /proc/$1/stat; }

echo "== start"
e2e_start_app windows
WID=$(xdotool search --sync --class MacSettings 2>/dev/null | head -1)
check "window found by class MacSettings" test -n "$WID"
check "WM_CLASS is macsettings/MacSettings" bash -c "xprop -id $WID WM_CLASS | grep -q '\"macsettings\", \"MacSettings\"'"
wait_for "dump written, pane windows" '.pane=="windows"' 5
shot windows

echo "== UI -> file -> rdwm"
cp "$E2E_CFG" "$E2E_T/before.toml"
wait_for "focus_follows_mouse starts on" '[.ctls[]|select(.id=="r:general:focus_follows_mouse")][0].value==true'
click_id r:general:focus_follows_mouse
wait_file "file has focus_follows_mouse = false" "focus_follows_mouse = false"
check "comment on another key survived" grep -q "# breathing room" "$E2E_CFG"
check "key order and unknown content survived (diff only adds one line)" bash -c "diff <(grep -v focus_follows_mouse $E2E_CFG) $E2E_T/before.toml"
check "backup written once" test -f "$E2E_CFG.bak-macsettings"
check "backup equals the original" cmp -s "$E2E_CFG.bak-macsettings" "$E2E_T/before.toml"
sleep 0.8
check "rdwm applied it (rdwmctl config)" test "$(rdwm_cfg .config.general.focus_follows_mouse)" = "false"
wait_for "toggle shows off" '[.ctls[]|select(.id=="r:general:focus_follows_mouse")][0].value==false'
shot windows-toggled

echo "== slider keeps trailing comment"
goto tiling
shot tiling
click_part r:general:gap_inner 0 0.5
wait_file "gap_inner rewritten" "gap_inner = [0-9]*   # breathing room"
check "slider value matches file" bash -c "test \"\$(grep -o 'gap_inner = [0-9]*' $E2E_CFG | grep -o '[0-9]*\$')\" = \"\$(jq -r '[.ctls[]|select(.id==\"r:general:gap_inner\")][0].value|floor' $DUMP)\""
sleep 0.5
check "rdwm gap_inner follows" test "$(rdwm_cfg .config.general.gap_inner)" = "$(grep -o 'gap_inner = [0-9]*' "$E2E_CFG" | grep -o '[0-9]*$')"

echo "== file -> UI (sed edit)"
sed -i 's/gap_inner = [0-9]*/gap_inner = 11/' "$E2E_CFG"
wait_for "slider shows 11 after the sed edit" '[.ctls[]|select(.id=="r:general:gap_inner")][0].value==11' 3
wait_for "notice says Reloaded from disk" '.notice=="Reloaded from disk"' 3
check "window kept the tiling pane" test "$(q .pane)" = "tiling"
sed -i 's/^smart_gaps = true/smart_gaps = false/' "$E2E_CFG"
wait_for "toggle follows sed edit" '[.ctls[]|select(.id=="r:general:smart_gaps")][0].value==false' 3
sed -i 's/^layouts = .*/layouts = ["bsp", "tile", "monocle"]/' "$E2E_CFG"
wait_for "default layout tile changes to bsp" '[.layouts[]|select(.id=="l:set:bsp")]|length==1' 3
shot after-sed

echo "== broken file"
cp "$E2E_CFG" "$E2E_T/good.toml"
printf '[general\ngap_inner =' > "$E2E_CFG"
wait_for "warning shown for half-written file" '(.warnings|length)>0' 3
GV=$(q '[.ctls[]|select(.id=="r:general:gap_inner")][0].value|floor'); check "values kept while invalid (gap_inner=$GV)" test "$GV" = "11"
shot broken
cp "$E2E_T/good.toml" "$E2E_CFG"
wait_for "warning gone when the file is valid again" '(.warnings|length)==0' 3
echo "== rejected value never written"
goto windows
sed -i 's/^gap_inner = 11.*/gap_inner = "wide"   # breathing room/' "$E2E_CFG"
sleep 1.2
cp "$E2E_CFG" "$E2E_T/badvalue.toml"
click_id r:general:resize_hints
sleep 0.8
check "file untouched after rejected save" cmp -s "$E2E_CFG" "$E2E_T/badvalue.toml"
wait_for "inline error mentions the problem" '(.warnings|length)>0' 3
shot rejected
cp "$E2E_T/good.toml" "$E2E_CFG"
sleep 1.2

echo "  (app alive: $(kill -0 $E2E_APP 2>&1 && echo yes); log: $(tail -3 $E2E_T/macsettings.log | tr '\n' ' '))"
echo "== rdwm state -> UI (wallpaper via rdwmctl)"
goto wallpaper
wait_for "thumbnails load lazily" '.loaded_thumbs>=2' 15
IMG=$(ls "$E2E_T"/cfg/omarchy/backgrounds/* | sed -n 3p)
click_xy $(jq -r "[.thumbs[]|select(.id==\"$IMG\")][0].rect|\"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)\"" "$DUMP")
wait_file "wallpaper image written to config" "image = \"$IMG\""
sleep 1
check "rdwm shows it" test "$(rdwmctl state | jq -r .state.wallpaper.image)" = "$IMG"
IMG2=$(ls "$E2E_T"/cfg/omarchy/backgrounds/* | sed -n 5p)
rdwmctl wallpaper "$IMG2" >/dev/null
wait_for "preview follows rdwmctl wallpaper" "(.preview|tostring|startswith(\"$IMG2\"))" 4
shot wallpaper-live
# mode segmented control
click_part r:wallpaper:mode 1 0.5
wait_file "wallpaper mode fit" 'mode = "fit"'

echo "== keyboard: rebind through the key recorder"
goto keyboard
shot keyboard
KID=$(jq -r '[.ctls[]|select(.label=="spawn true")][0].id' "$DUMP")
check "user bind listed ($KID)" test "$KID" != "null"
click_id "$KID"
sleep 0.3
xdotool key --window "$WID" F9
wait_file "bind rewritten to F9" 'key = "F9"'
check "args kept" grep -A4 'key = "F9"' "$E2E_CFG" | grep -q 'args = \["true"\]'
sleep 0.8
check "rdwm knows the new key" bash -c "rdwmctl config | jq -e '[.config.binds[]|select(.keys|ascii_downcase==\"f9\")]|length==1' >/dev/null"
echo "== recorder with modifier chips"
click_id "$KID"; sleep 0.3
click_id mod:super; click_id mod:shift
shot recorder
xdotool key --window "$WID" F7
wait_file "chips + F7 recorded" 'mods = \["super", "shift"\]' 
check "key F7 stored" grep -q 'key = "F7"' "$E2E_CFG"
echo "== rules: add"
goto rules
click_id t:rule:@add
wait_file "rule added" 'class = "NewApp"'
check "file still parses" rdwmctl check-config "$E2E_CFG"

echo "== keyboard navigation"
goto general
xdotool key --window "$WID" Tab; sleep 0.2
xdotool key --window "$WID" Tab; sleep 0.2
xdotool key --window "$WID" ctrl+f; sleep 0.2
xdotool key --window "$WID" g a p; sleep 0.4
check "search puts Tiling first ($(q '.panes|map(.id)|join(",")'))" test "$(q '.panes[0].id')" = "tiling"
xdotool key --window "$WID" Escape; sleep 0.3
check "Escape cleared the search (all panes back)" test "$(q '.panes|length')" -ge 22

echo "== offline: rdwm stopped, the file is still edited"
kill $E2E_RDWM; wait $E2E_RDWM 2>/dev/null; sleep 1.5
goto windows
click_id r:general:lock_fullscreen
wait_file "edit saved without rdwm" "lock_fullscreen = false"
wait_for "notice says rdwm is not running" '(.notice|tostring|test("not running"))' 3

echo "== idle cost"
APP=$(pgrep -n -x macsettings)
for p in general appearance desktop wallpaper tiling windows rules keyboard autostart displays; do goto $p; done
sleep 8
T0=$(cpu_ticks $APP); sleep 10; T1=$(cpu_ticks $APP)
RSS=$(awk '/VmRSS/{print int($2/1024)}' /proc/$APP/status)
THREADS=$(ls /proc/$APP/task | wc -l)
echo "  idle ticks over 10 s: $((T1-T0)); RSS ${RSS} MB; threads $THREADS"
check "idle CPU under 1% (<= 10 ticks / 10 s)" test $((T1-T0)) -le 10
echo "$RSS" > "$E2E_T/rss"; [ -n "${SHOTS:-}" ] && echo "RSS ${RSS} MB, idle ticks $((T1-T0))" > "$SHOTS/idle.txt"
check "RSS under 40 MB after visiting every pane" test "$RSS" -lt 40

echo "== close keys"
xdotool key --window "$WID" ctrl+w; sleep 0.8
check "Ctrl+W closes the window" bash -c "! kill -0 $APP 2>/dev/null"
e2e_start_app windows; APP=$E2E_APP; WID=$(xdotool search --sync --class MacSettings | head -1)
xdotool key --window "$WID" Escape; sleep 0.3; xdotool key --window "$WID" Escape; sleep 0.8
check "Escape closes the window" bash -c "! kill -0 $APP 2>/dev/null"

echo
echo "passed $PASS, failed $FAIL"
[ "$FAIL" -eq 0 ]
