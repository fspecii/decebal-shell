#!/usr/bin/env bash
# "Match wallpaper" end to end on a private Xvfb + rdwm: wallpapers of different dominant hues are set through rdwm,
# `decthemectl watch` follows them, the derived accent changes with the hue, the running decbar restyles itself, the
# feature is off by default and fully reversible, and the Theme pane renders the section (PNG screenshots in $SHOTS).
#   tests/e2e/wallpaper-colors.sh        (SHOTS=dir keeps the PNGs; PROFILE=debug|release; E2E_DISPLAY=:94 by default)
# Everything is private: own HOME/XDG dirs, only PIDs started here are killed, never the live display or rdwm.sock.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
PROFILE=${PROFILE:-release}
BIN=${CARGO_TARGET_DIR:-$HERE/target}/$PROFILE
RDWM_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
[ -x "$RDWM_DIR/rdwm" ] || { echo "rdwm not found (set RDWM_BIN_DIR)"; exit 77; }
for t in Xvfb xdotool import jq convert; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
PFLAG=--release; [ "$PROFILE" = debug ] && PFLAG=
(cd "$HERE" && cargo build -q --offline $PFLAG --bins --examples) || { echo "build failed"; exit 2; }
N=${E2E_DISPLAY:-:94}
case "$N" in :0|:1|:2|0|1|2) echo "refusing a live display"; exit 2;; esac

PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d /tmp/decebal-wallcolors-e2e.XXXXXX)
SHOTS=${SHOTS:-$TMP/shots}; mkdir -p "$SHOTS"
cleanup() { for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done; sleep 0.3; for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done; wait 2>/dev/null
  if [ "$SHOTS" = "$TMP/shots" ]; then rm -rf "$TMP"; else find "$TMP" -mindepth 1 -maxdepth 1 ! -path "$SHOTS" -exec rm -rf {} +; rmdir "$TMP" 2>/dev/null; fi; }
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "got '$2', want '$3'"; fi; }
ne() { if [ "$2" != "$3" ]; then pass "$1"; else fail "$1" "both '$2'"; fi; }
wait_for() { local t=$1; shift; local end=$(($(date +%s%N) / 1000000 + t * 1000)); while [ $(($(date +%s%N) / 1000000)) -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 0.05; done; return 1; }

export HOME=$TMP/home XDG_CONFIG_HOME=$TMP/cfg XDG_RUNTIME_DIR=$TMP/run XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata XDG_CACHE_HOME=$TMP/cache XDG_STATE_HOME=$TMP/state
export DECEBAL_CONFIG=$TMP/cfg/omarchy DECEBAL_BACKEND=x11 DISPLAY=$N RDWMCTL=$RDWM_DIR/rdwmctl DECSETTINGS_CONFIG=$TMP/cfg/rdwm/config.toml
export DECEBAL_COLOR_SCHEME=dark DECEBAL_THEMES=$TMP/cfg/decebal/themes
unset WAYLAND_DISPLAY RDWM_SOCK
mkdir -p "$HOME" "$XDG_CONFIG_HOME/rdwm" "$DECEBAL_CONFIG" "$XDG_RUNTIME_DIR" "$XDG_DATA_HOME/applications" "$TMP/nodata" "$XDG_CACHE_HOME" "$XDG_STATE_HOME"
chmod 700 "$XDG_RUNTIME_DIR"
export PATH="$RDWM_DIR:$BIN:$PATH"
CFG=$DECSETTINGS_CONFIG
printf '# test config\n[general]\ngap_inner = 6\n\n[decoration]\nenabled = true\n\n[bar]\nshow = false\n\n[wallpaper]\nimage = "/nonexistent"\nmode = "fill"\n' > "$CFG"
export DECSETTINGS_DEBUG_LAYOUT=$TMP/dump.json
SEL() { jq -r "$1" "$DECEBAL_CONFIG/decebal.json" 2>/dev/null; }

# wallpapers of different dominant hues (a gradient with a lighter patch, like a photo, not a flat fill)
mkdir -p "$TMP/walls"
mkwall() { convert -size 1600x1000 "radial-gradient:$2-$3" "$TMP/walls/$1.png"; }
mkwall red '#e0405a' '#401020'
mkwall green '#40c070' '#0c3018'
mkwall blue '#4070e0' '#101840'
mkwall orange '#f09030' '#401c08'
convert -size 1600x1000 xc:'#808080' "$TMP/walls/grey.png"
convert -size 1600x1000 xc:'#000000' "$TMP/walls/black.png"
convert -size 1600x1000 gradient:'#f0f4ff-#c0d0f0' "$TMP/walls/pale.png"

Xvfb $N -screen 0 1600x900x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X${N#:} || { echo "Xvfb failed"; exit 2; }
xdotool mousemove 800 450
rdwm --backend x11 --config "$CFG" >"$TMP/rdwm.log" 2>&1 & PIDS+=($!)
wait_for 5 bash -c "xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window" || { echo "rdwm did not start"; cat "$TMP/rdwm.log"; exit 2; }
# set the wallpaper the way a session does: at runtime and in rdwm's config (every colour change reloads rdwm's
# config, and a reload falls back to the file's wallpaper)
wp() { sed -i "s|^image = .*|image = \"$TMP/walls/$1.png\"|" "$CFG"; rdwmctl wallpaper "$TMP/walls/$1.png" >/dev/null 2>&1; }
wp blue
printf '{"workspaces": false}\n' > "$DECEBAL_CONFIG/decebal.json"
decbar >"$TMP/decbar.log" 2>&1 & PIDS+=($!)
sleep 2

shot() { sleep 1.3; import -window root "$SHOTS/$1.png" 2>/dev/null; }
px() { import -window root -crop 1x1+$1+$2 +repage -depth 8 txt:- 2>/dev/null | tail -1 | grep -o '#[0-9A-F]\{6\}' | head -1 | tr 'A-F' 'a-f'; }
# eqpx / nepx: programs poll the theme every 500 ms
eqpx() { local end=$((SECONDS + 8)) got; while [ $SECONDS -lt $end ]; do got=$(px "$2" "$3"); [ "$got" = "$4" ] && { pass "$1"; return; }; sleep 0.3; done; fail "$1" "got '$got', want '$4'"; }
nepx() { local end=$((SECONDS + 8)) got; while [ $SECONDS -lt $end ]; do got=$(px "$2" "$3"); [ "$got" != "$4" ] && [ -n "$got" ] && { pass "$1 ($got)"; return; }; sleep 0.3; done; fail "$1" "still '$got'"; }
# hue (degrees, 0-359) of #rrggbb
hue() { echo "$1" | awk '{ r=strtonum("0x" substr($0,2,2))/255; g=strtonum("0x" substr($0,4,2))/255; b=strtonum("0x" substr($0,6,2))/255;
  mx=r; if(g>mx)mx=g; if(b>mx)mx=b; mn=r; if(g<mn)mn=g; if(b<mn)mn=b; d=mx-mn; if(d==0){print -1; exit}
  if(mx==r)h=(g-b)/d; else if(mx==g)h=2+(b-r)/d; else h=4+(r-g)/d; h*=60; if(h<0)h+=360; printf "%d\n", h }'; }
accent() { decthemectl get --json | jq -r .colors.accent; }
# the accent must reach a hue in [lo,hi] within 8 s
accent_hue() { local end=$((SECONDS + 8)) a h; while [ $SECONDS -lt $end ]; do a=$(accent); h=$(hue "$a"); [ "$h" -ge "$2" ] && [ "$h" -le "$3" ] && { pass "$1 (accent $a hue $h)"; return; }; sleep 0.3; done; fail "$1" "accent $a hue $h, want $2-$3"; }

echo "== off by default"
[[ $(decthemectl dynamic status) == *off* ]] && pass "status says off" || fail "status"
eq "no dynamic key written" "$(SEL '.theme.dynamic // "none"')" none
eq "accent is the macOS blue" "$(accent)" "#0a84ff"
eqpx "bar is the original #131313" 900 3 "#131313"
[[ $(decthemectl dynamic refresh) == *off* ]] && pass "refresh is a no-op while off" || fail "refresh while off"
shot 00-off-blue-wallpaper

echo "== on: macOS theme follows the wallpaper"
t0=$(date +%s%N)
[[ $(decthemectl dynamic on --scheme tonal-spot --surfaces tint --strength 70) == *seed* ]] && pass "dynamic on" || fail "dynamic on"
echo "      (first read of a 1600x1000 PNG + apply: $((($(date +%s%N) - t0) / 1000000)) ms)"
eq "theme untouched" "$(decthemectl get --json | jq -r .theme)" macos
accent_hue "blue wallpaper gives a blue accent" 205 275
nepx "running decbar restyled itself (tinted bar)" 900 3 "#131313"
BLUE_BAR=$(px 900 3)
shot 10-macos-blue
t0=$(date +%s%N); decthemectl dynamic refresh >/dev/null; echo "      (refresh with a cached palette: $((($(date +%s%N) - t0) / 1000000)) ms)"
[ -s "$XDG_CACHE_HOME/decebal/wallpaper-colors.json" ] && pass "palette cached in XDG_CACHE_HOME" || fail "no cache file"

echo "== the watcher follows wallpaper changes"
decthemectl watch --interval 300 >"$TMP/watch.log" 2>&1 & WPID=$!; PIDS+=($WPID)
sleep 1
decthemectl watch >"$TMP/watch2.log" 2>&1
grep -q "already running" "$TMP/watch2.log" && pass "a second watcher refuses to start" || fail "second watcher" "$(cat "$TMP/watch2.log")"
wp red;    accent_hue "red wallpaper: accent turns red/pink" 330 360
nepx "bar tint moved with the wallpaper" 900 3 "$BLUE_BAR"; shot 11-macos-red
RED_ACC=$(accent)
wp green;  accent_hue "green wallpaper: accent turns green" 95 165;  shot 12-macos-green
GREEN_ACC=$(accent)
wp orange; accent_hue "orange wallpaper: accent turns orange" 20 55; shot 13-macos-orange
wp blue;   accent_hue "back to blue" 205 275
ne "red and green give different accents" "$RED_ACC" "$GREEN_ACC"
eq "rdwm's focused border follows too" "$(rdwmctl config | jq -r .config.colors.focused)" "$(accent)"
kill -0 $WPID 2>/dev/null && pass "watcher still running (idle)" || fail "watcher died"

echo "== any base theme"
for th in nord windows11 caelestia dracula; do
  decthemectl apply $th >/dev/null; wp red
  accent_hue "$th + red wallpaper" 330 360
  wp blue; accent_hue "$th + blue wallpaper" 205 275
done
decthemectl apply windows11 --mode light >/dev/null; wp green; accent_hue "windows11 light + green" 95 165; shot 20-windows-light-green
decthemectl apply macos --mode dark >/dev/null; wp red

echo "== schemes, accent-only, strength, contrast"
for s in tonal-spot vibrant expressive monochrome neutral content palette; do
  decthemectl dynamic scheme $s >/dev/null 2>&1 && pass "scheme $s accepted" || fail "scheme $s"
  sleep 0.4; echo "      $s -> $(accent)"
done
decthemectl dynamic scheme bogus >/dev/null 2>&1 && fail "bad scheme accepted" || pass "bad scheme refused"
decthemectl dynamic scheme monochrome >/dev/null; sleep 0.8
a=$(accent); r=${a:1:2}; g=${a:3:2}; b=${a:5:2}
[ "$r" = "$g" ] && [ "$g" = "$b" ] && pass "monochrome accent is grey ($a)" || fail "monochrome" "$a"
decthemectl dynamic scheme vibrant >/dev/null; decthemectl dynamic surfaces accent >/dev/null
eqpx "accent-only: the bar keeps the theme's own colour" 900 3 "#131313"
accent_hue "accent-only: accent still follows" 330 360
decthemectl dynamic surfaces tint >/dev/null; decthemectl dynamic strength 100 >/dev/null; decthemectl dynamic contrast 60 >/dev/null
nepx "tint at 100 %: the bar is tinted" 900 3 "#131313"
shot 30-macos-red-vibrant-tint100
decthemectl dynamic strength 150 >/dev/null 2>&1 && fail "strength 150 accepted" || pass "out-of-range strength refused"
decthemectl dynamic contrast 0 >/dev/null; decthemectl dynamic strength 50 >/dev/null; decthemectl dynamic scheme tonal-spot >/dev/null

echo "== grey, black and pale wallpapers fall back to the theme's own colours"
for w in grey black; do
  wp $w; sleep 2
  eq "$w wallpaper: macOS blue stays" "$(accent)" "#0a84ff"
done
[[ $(decthemectl dynamic status) == *"no colour"* ]] && pass "status explains it" || fail "status for grey"
decthemectl dynamic mode wallpaper >/dev/null
wp pale; sleep 2
eq "pale wallpaper + follow mode: the light variant" "$(decthemectl get --json | jq -r .dark)" false
wp blue; sleep 2
eq "blue wallpaper (dark picture): the dark variant" "$(decthemectl get --json | jq -r .dark)" true
decthemectl dynamic mode keep >/dev/null

echo "== a fixed source ignores the wallpaper"
decthemectl dynamic source "$TMP/walls/green.png" >/dev/null
accent_hue "source = green.png" 95 165
wp red; sleep 2
accent_hue "wallpaper changes do not matter" 95 165
decthemectl dynamic source '#2040ff' >/dev/null
accent_hue "source = a colour" 205 275
decthemectl dynamic source wallpaper >/dev/null
decthemectl dynamic preview --json | jq -e '.accent and .seed' >/dev/null && pass "preview prints the palette" || fail "preview"

echo "== the older form still works (Caelestia, full palette)"
out=$(decthemectl dynamic --wallpaper "$TMP/walls/green.png" --scheme vibrant --dark)
[[ $out == *"seed #"* ]] && pass "legacy form prints the seed" || fail "legacy form" "$out"
eq "legacy form selects Caelestia" "$(decthemectl get --json | jq -r .theme)" caelestia
eq "...as a full palette" "$(SEL .theme.dynamic.surfaces)" full
decthemectl apply macos >/dev/null
decthemectl dynamic source wallpaper >/dev/null; decthemectl dynamic scheme tonal-spot >/dev/null; decthemectl dynamic surfaces tint >/dev/null; decthemectl dynamic strength 50 >/dev/null

echo "== settings UI"
decthemectl dynamic off >/dev/null; wp red; sleep 1
decsettings --config "$CFG" theme >"$TMP/ms.log" 2>&1 & PIDS+=($!)
wait_for 8 test -s "$DECSETTINGS_DEBUG_LAYOUT" || fail "settings dump never written"
sleep 1
WID=$(xdotool search --sync --class DecSettings 2>/dev/null | head -1)
[ -n "$WID" ] && pass "settings window up" || fail "no settings window"
xdotool windowsize "$WID" 900 880 windowmove "$WID" 100 10 2>/dev/null; sleep 1
q() { jq -r "$1" "$DECSETTINGS_DEBUG_LAYOUT" 2>/dev/null; }
ctl() { jq -r "[.ctls[]|select(.id==\"$1\")][0].rect|\"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)\""  "$DECSETTINGS_DEBUG_LAYOUT"; }
seg() { jq -r "[.ctls[]|select(.id==\"$1\")][0].parts[$2]|\"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)\"" "$DECSETTINGS_DEBUG_LAYOUT"; }
click() { xdotool mousemove --window "$WID" $1 $2; sleep 0.1; xdotool click --window "$WID" 1; sleep 0.7; }
scroll_to() { # scroll until control $1 is on screen (y < 800), at most 30 wheel clicks
  local i y; for i in $(seq 1 30); do y=$(q "[.ctls[]|select(.id==\"$1\")][0].rect[1] // 99999"); y=${y%.*}; [ "$y" -lt 760 ] && [ "$y" -gt 40 ] && return 0; xdotool mousemove --window "$WID" 450 450; xdotool click --window "$WID" 5; sleep 0.1; done; return 1; }
scroll_to th:dynamic && pass "Match wallpaper switch reachable" || fail "switch not found"
sleep 0.4; shot 40-settings-section-off
click $(ctl th:dynamic); sleep 1.5
eq "the switch turns it on" "$(SEL .theme.dynamic.enabled)" true
accent_hue "UI: accent from the red wallpaper" 330 360
scroll_to th:dyn-refresh; sleep 0.5
for id in th:dyn-scheme th:dyn-surfaces th:dyn-strength th:dyn-contrast th:dyn-mode th:dyn-source th:dyn-refresh; do
  [ "$(q "[.ctls[]|select(.id==\"$id\")]|length")" = 1 ] && pass "control $id present" || fail "control $id missing"
done
shot 41-settings-section-on-red
click $(ctl th:dyn-refresh); sleep 0.5
click $(seg th:dyn-surfaces 2); sleep 1
echo "      surfaces after click: $(SEL .theme.dynamic.surfaces)"
shot 42-settings-section-clicked
wp green; sleep 3
accent_hue "settings open: a new wallpaper recolours the shell" 95 165
shot 43-settings-after-wallpaper-change

echo "== off again restores everything"
decthemectl dynamic off >/dev/null
eqpx "bar back to #131313" 900 3 "#131313"
eq "accent back to the macOS blue" "$(accent)" "#0a84ff"
eq "settings kept for next time" "$(SEL .theme.dynamic.scheme)" tonal-spot
shot 50-off-again

echo "== no crashes"
for p in "${PIDS[@]}"; do kill -0 "$p" 2>/dev/null || fail "process $p died"; done
grep -il "panicked" "$TMP"/*.log 2>/dev/null | head -3 | while read -r f; do fail "panic in $f"; done
echo; echo "passed $PASS, failed $FAIL (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
