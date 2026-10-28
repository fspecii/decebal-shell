#!/usr/bin/env bash
# Font presets end to end on a PRIVATE Xvfb: apply several presets with decthemectl, check that the stored selection,
# rdwm's titlebar font and the running programs follow, and screenshot the bar, the dashboard and the Theme pane
# (Fonts section) so the glyph baseline, clipping and the CJK / emoji / icon fallbacks can be looked at.
#   tests/e2e/fonts.sh          (SHOTS=dir keeps the PNGs; E2E_DISPLAY=:93; PROFILE=release for release binaries)
# Everything is private: own HOME/XDG dirs and XDG_RUNTIME_DIR, only PIDs started here are killed, never DISPLAY=:1.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
PROFILE=${PROFILE:-debug}
BIN=${CARGO_TARGET_DIR:-$HERE/target}/$PROFILE
RDWM_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
[ -x "$RDWM_DIR/rdwm" ] || { echo "rdwm not found (set RDWM_BIN_DIR)"; exit 77; }
for t in Xvfb xdotool import jq convert dbus-daemon; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline $([ "$PROFILE" = release ] && echo --release) --bins --examples) || { echo "build failed"; exit 2; }
N=${E2E_DISPLAY:-:93}
[ "$N" = ":1" ] && { echo "refusing the live display"; exit 2; }

PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d /tmp/decebal-fonts-e2e.XXXXXX)
SHOTS=${SHOTS:-$TMP/shots}; mkdir -p "$SHOTS"
cleanup() { for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done; sleep 0.3; for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done; wait 2>/dev/null
  [ "$SHOTS" = "$TMP/shots" ] && rm -rf "$TMP" || find "$TMP" -mindepth 1 -maxdepth 1 ! -path "$SHOTS" -exec rm -rf {} +; }
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "got '$2', want '$3'"; fi; }
wait_for() { local t=$1; shift; local end=$(($(date +%s%N) / 1000000 + t * 1000)); while [ $(($(date +%s%N) / 1000000)) -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 0.05; done; return 1; }

# fontconfig: the system fonts plus the real user's font folder (read only), cache in the private dir, so Inter etc. are seen
REALHOME=$HOME
mkdir -p "$TMP/fc"
cat > "$TMP/fc/fonts.conf" <<EOF
<?xml version="1.0"?><!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig><include ignore_missing="yes">/etc/fonts/fonts.conf</include><dir>$REALHOME/.local/share/fonts</dir><cachedir>$TMP/fc/cache</cachedir></fontconfig>
EOF
export FONTCONFIG_FILE=$TMP/fc/fonts.conf
export HOME=$TMP/home XDG_CONFIG_HOME=$TMP/cfg XDG_RUNTIME_DIR=$TMP/run XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata XDG_CACHE_HOME=$TMP/cache
export DECEBAL_CONFIG=$TMP/cfg/omarchy DECEBAL_BACKEND=x11 DISPLAY=$N RDWMCTL=$RDWM_DIR/rdwmctl DECSETTINGS_CONFIG=$TMP/cfg/rdwm/config.toml
export DECEBAL_COLOR_SCHEME=dark
unset WAYLAND_DISPLAY RDWM_SOCK HYPRLAND_INSTANCE_SIGNATURE DBUS_SESSION_BUS_ADDRESS
mkdir -p "$HOME" "$XDG_CONFIG_HOME/rdwm" "$DECEBAL_CONFIG" "$XDG_RUNTIME_DIR" "$XDG_DATA_HOME/applications" "$TMP/nodata" "$XDG_CACHE_HOME/decebal"
chmod 700 "$XDG_RUNTIME_DIR"
export PATH="$RDWM_DIR:$BIN:$BIN/examples:$PATH"
CFG=$DECSETTINGS_CONFIG
printf '# test config\n[general]\ngap_inner = 6\n\n[decoration]\nenabled = true\n\n[bar]\nshow = false\n' > "$CFG"
echo "{\"temp_c\":12,\"code\":116,\"night\":false,\"fetched\":$(date +%s)}" > "$XDG_CACHE_HOME/decebal/weather.json"
FAKE=$BIN/examples/fakewin

Xvfb $N -screen 0 1600x900x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X${N#:} || { echo "Xvfb failed"; exit 2; }
dbus-daemon --session --address="unix:path=$XDG_RUNTIME_DIR/bus" --nofork >"$TMP/dbus.log" 2>&1 & PIDS+=($!)
wait_for 5 test -S "$XDG_RUNTIME_DIR/bus" || { echo "dbus failed"; exit 2; }
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
xdotool mousemove 800 450
rdwm --backend x11 --config "$CFG" >"$TMP/rdwm.log" 2>&1 & PIDS+=($!)
wait_for 5 bash -c "xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window" || { echo "rdwm did not start"; cat "$TMP/rdwm.log"; exit 2; }
rdwmctl wallpaper --color "#46607a" >/dev/null 2>&1
decbar >"$TMP/decbar.log" 2>&1 & BARPID=$!; PIDS+=($BARPID)
decdock >"$TMP/decdock.log" 2>&1 & PIDS+=($!)
"$FAKE" Alpha "Alpha 日本語 😀 Window" cc3333 & PIDS+=($!)
sleep 2

shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
# crop: the bar strip and the top-left of the screen where the focused window's titlebar is
strip() { import -window root -crop "${2:-1600x34+0+0}" +repage "$SHOTS/$1.png" 2>/dev/null; }
rdcfg() { rdwmctl config | jq -r "$1"; }
sel() { jq -r "$1" "$DECEBAL_CONFIG/decebal.json" 2>/dev/null; }
rss_of() { awk '/VmRSS/ {print $2}' /proc/$1/status 2>/dev/null; }

echo "== presets"
decthemectl font list | grep -q "monospace-ui" && pass "font list shows the presets" || fail "font list"
for p in system inter segoe cantarell rubik jetbrains ibm-plex fira noto monospace-ui; do
  out=$(decthemectl font $p 2>&1)
  eq "font $p stored" "$(sel .theme.font.preset)" "$p"
  eq "font $p resolved" "$(decthemectl get --json | jq -r .fonts.preset)" "$p"
  sleep 1.3
  strip "bar-$p"
  case $p in system|inter|segoe|jetbrains|monospace-ui) shot "full-$p" ;; esac
  echo "      $p -> $(decthemectl font status | sed -n 2p | sed 's/^ *//')"
done

echo "== rdwm titlebar font follows the title role"
decthemectl font segoe >/dev/null
f=$(rdcfg '.config.decoration.font // ""')
want=$(decthemectl font status | awk '/^title/ {getline; print}' | sed 's/^ *using //; s/ >.*//; s/ *(not installed.*//')
if [ -n "$want" ]; then eq "decoration.font = first installed title font" "$f" "$want"; else pass "no named title font to export (generic only)"; fi
decthemectl font system >/dev/null
eq "decoration.font removed for a generic-only preset" "$(rdcfg '.config.decoration.font // "none"' | sed 's/^$/none/')" none

echo "== per-role chains, scale, reset, back-compat"
decthemectl font inter >/dev/null
decthemectl font set mono "Liberation Mono,JetBrains Mono" >/dev/null 2>&1
eq "mono chain stored as a list" "$(sel '.theme.font.mono | join("|")')" "Liberation Mono|JetBrains Mono"
eq "preset kept next to the chain" "$(sel .theme.font.preset)" inter
decthemectl font set ui "Inter,Noto Sans CJK,Noto Color Emoji" >/dev/null 2>&1
eq "ui chain resolved with fallbacks" "$(decthemectl get --json | jq -r '.fonts.ui[0:3] | join("|")')" "Inter|Noto Sans CJK|Noto Color Emoji"
decthemectl font scale 1.2 >/dev/null
eq "scale lives inside the font object" "$(sel .theme.font.scale)" 1.2
sleep 1.3; strip bar-chain
decthemectl font reset >/dev/null
eq "reset empties the font object" "$(sel '.theme.font // "none"')" none
# a decebal.json from before font roles: font is a plain string, fontScale top level
printf '{"theme": {"name": "macos", "font": "Liberation Sans", "fontScale": 1.1}}\n' > "$DECEBAL_CONFIG/decebal.json"
eq "old font string is the ui font" "$(decthemectl get --json | jq -r '.fonts.ui[0]')" "Liberation Sans"
eq "old fontScale still read" "$(decthemectl get --json | jq -r '.font_scale*10|round')" 11
sleep 1.3; strip bar-legacy
decthemectl font inter >/dev/null
decthemectl font bogus >/dev/null 2>&1 && fail "unknown preset rejected" || pass "unknown preset rejected"

echo "== dashboard"
for p in inter monospace-ui segoe; do
  decthemectl font $p >/dev/null; sleep 1.2
  decdash --x 800 >"$TMP/decdash.out" 2>&1 & DP=$!; PIDS+=($DP)
  wait_for 4 bash -c "xdotool search --name '^decdash\$' | grep -q ." || fail "dash window ($p)"
  sleep 1.0; shot "dash-overview-$p"
  xdotool key 3; sleep 0.6; shot "dash-perf-$p"
  kill $DP 2>/dev/null; wait $DP 2>/dev/null; sleep 0.3
done

echo "== settings UI: Fonts section"
decthemectl font reset >/dev/null; decthemectl apply macos >/dev/null
export DECSETTINGS_DEBUG_LAYOUT=$TMP/dump.json
decsettings --config "$CFG" appearance/fonts >"$TMP/ms.log" 2>&1 & MS=$!; PIDS+=($MS)
wait_for 8 test -s "$DECSETTINGS_DEBUG_LAYOUT" || fail "settings dump never written"
sleep 1
WID=$(xdotool search --sync --class DecSettings 2>/dev/null | head -1)
[ -n "$WID" ] && pass "settings window up" || fail "no settings window"
q() { jq -r "$1" "$DECSETTINGS_DEBUG_LAYOUT" 2>/dev/null; }
. "$HERE/tests/e2e/settings_ui.sh"
reveal th:fontpreset >/dev/null; shot settings-fonts-top
if [ -n "$(rect_of th:fontpreset)" ]; then
  pass "the preset picker is on the pane"
  click_ctl th:fontpreset; sleep 0.4
  shot settings-fonts-popup
  xdotool type --delay 80 "ibm"; sleep 0.2; xdotool key Return; sleep 1.5
  eq "choosing IBM Plex in the picker stores it" "$(sel .theme.font.preset)" ibm-plex
  reveal th:font:mono >/dev/null; shot settings-fonts-ibm
  click_ctl th:font:mono; sleep 0.4
  xdotool type --delay 80 "Liberation Mono"; sleep 0.2; xdotool key Return; sleep 1.5
  eq "role picker stores the family" "$(sel '.theme.font.mono | if type=="array" then join("|") else . end')" "Liberation Mono"
  reveal th:fontreset >/dev/null; shot settings-fonts-mono
  click_ctl th:fontreset; sleep 1.2
  eq "Reset fonts empties the choice" "$(sel '.theme.font // "none"')" none
else
  fail "no th:fontpreset control in the layout dump"
fi
kill $MS 2>/dev/null

echo "== memory"
bar_pid=$BARPID
echo "      decbar RSS $(rss_of "$bar_pid") kB"
[ "$(rss_of "$bar_pid")" -lt 90000 ] && pass "decbar stays small" || fail "decbar RSS" "$(rss_of "$bar_pid") kB"

echo "$PASS passed, $FAIL failed (screenshots: $SHOTS)"
[ "$FAIL" = 0 ]
