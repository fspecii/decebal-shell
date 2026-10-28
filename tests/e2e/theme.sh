#!/usr/bin/env bash
# Theming end to end: private Xvfb + rdwm + decbar + decdock + decsettings, switch themes with decthemectl and
# through the settings UI, check that every program and rdwm follow, and save screenshots.
#   tests/e2e/theme.sh                 (SHOTS=dir keeps the PNGs; E2E_DISPLAY=:135 by default)
# Everything is private: own HOME/XDG dirs, XDG_RUNTIME_DIR passed explicitly, only PIDs started here are killed.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
BIN=${CARGO_TARGET_DIR:-$HERE/target}/release
RDWM_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
[ -x "$RDWM_DIR/rdwm" ] || { echo "rdwm not found (set RDWM_BIN_DIR)"; exit 77; }
for t in Xvfb xdotool import jq convert; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline --release --bins --examples) || { echo "build failed"; exit 2; }
N=${E2E_DISPLAY:-:135}
[ "$N" = ":1" ] && { echo "refusing the live display"; exit 2; }

PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d /tmp/decebal-theme-e2e.XXXXXX)
SHOTS=${SHOTS:-$TMP/shots}; mkdir -p "$SHOTS"
cleanup() { for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done; sleep 0.3; for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done; wait 2>/dev/null
  [ "$SHOTS" = "$TMP/shots" ] && rm -rf "$TMP" || find "$TMP" -mindepth 1 -maxdepth 1 ! -path "$SHOTS" -exec rm -rf {} +; }
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "got '$2', want '$3'"; fi; }
wait_for() { local t=$1; shift; local end=$(($(date +%s%N) / 1000000 + t * 1000)); while [ $(($(date +%s%N) / 1000000)) -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 0.05; done; return 1; }

export HOME=$TMP/home XDG_CONFIG_HOME=$TMP/cfg XDG_RUNTIME_DIR=$TMP/run XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata
export DECEBAL_CONFIG=$TMP/cfg/omarchy DECEBAL_BACKEND=x11 DISPLAY=$N RDWMCTL=$RDWM_DIR/rdwmctl DECSETTINGS_CONFIG=$TMP/cfg/rdwm/config.toml
export DECEBAL_COLOR_SCHEME=dark
unset WAYLAND_DISPLAY RDWM_SOCK
mkdir -p "$HOME" "$XDG_CONFIG_HOME/rdwm" "$DECEBAL_CONFIG" "$XDG_RUNTIME_DIR" "$XDG_DATA_HOME/applications" "$TMP/nodata"
chmod 700 "$XDG_RUNTIME_DIR"
export PATH="$RDWM_DIR:$BIN:$PATH"
CFG=$DECSETTINGS_CONFIG
printf '# test config\n[general]\ngap_inner = 6\n\n[decoration]\nenabled = true\n\n[bar]\nshow = false\n' > "$CFG"
FAKE=$BIN/examples/fakewin
for a in Alpha:cc3333 Beta:33cc66; do n=${a%%:*}; c=${a##*:}; printf '[Desktop Entry]\nType=Application\nName=%s\nExec=%s %s "%s Window" %s\nStartupWMClass=%s\n' "$n" "$FAKE" "$n" "$n" "$c" "$n" > "$XDG_DATA_HOME/applications/$n.desktop"; done
printf '{"version":1,"pinned":["launchpad","Alpha","Beta","trash"],"order":["launchpad","Alpha","Beta","trash"]}\n' > "$DECEBAL_CONFIG/dock-pinned-macos.json"

Xvfb $N -screen 0 1600x1000x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X${N#:} || { echo "Xvfb failed"; exit 2; }
xdotool mousemove 800 500
rdwm --backend x11 --config "$CFG" >"$TMP/rdwm.log" 2>&1 & PIDS+=($!)
wait_for 5 bash -c "xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window" || { echo "rdwm did not start"; cat "$TMP/rdwm.log"; exit 2; }
rdwmctl wallpaper --color "#46607a" >/dev/null 2>&1
decbar >"$TMP/decbar.log" 2>&1 & PIDS+=($!)
decdock >"$TMP/decdock.log" 2>&1 & PIDS+=($!)
"$FAKE" Alpha "Alpha Window" cc3333 & PIDS+=($!)
"$FAKE" Beta "Beta Window" 33cc66 & PIDS+=($!)
sleep 2
export DECSETTINGS_DEBUG_LAYOUT=$TMP/dump.json

shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
px() { import -window root -crop 1x1+$1+$2 +repage -depth 8 txt:- 2>/dev/null | tail -1 | grep -o '#[0-9A-F]\{6\}' | head -1 | tr 'A-F' 'a-f'; }
rdcfg() { rdwmctl config | jq -r "$1"; }
settle() { sleep 1.0; }
# eqpx DESC X Y WANT: the pixel must reach WANT within 6 s (programs poll the theme every 500 ms)
eqpx() { local end=$((SECONDS + 6)) got; while [ $SECONDS -lt $end ]; do got=$(px "$2" "$3"); [ "$got" = "$4" ] && { pass "$1"; return; }; sleep 0.3; done; fail "$1" "got '$got', want '$4'"; }
theme_name() { decthemectl get --json | jq -r .theme; }

echo "== start state"
eq "default theme is macos" "$(theme_name)" macos
eqpx "bar is the original #131313" 900 3 "#131313"
shot 00-start-macos-dark

echo "== decthemectl list / get / apply"
decthemectl list | grep -q windows11 && pass "list shows windows11" || fail "list"
decthemectl apply nord | grep -q "applied nord" && pass "apply nord" || fail "apply nord"
settle
eq "get says nord" "$(theme_name)" nord
eqpx "bar follows live (nord bar #242933)" 900 3 "#242933"
eq "rdwm focused titlebar colour = nord" "$(rdcfg .config.decoration.focused_bg)" "#3b4252"
eq "rdwm border colour focused = nord accent" "$(rdcfg .config.colors.focused)" "#88c0d0"
grep -q "# test config" "$CFG" && pass "comment in rdwm config survived" || fail "comment lost"
shot 01-nord-dark

echo "== light, windows, high contrast"
decthemectl apply "macOS Light" >/dev/null; settle
eqpx "light bar colour" 900 3 "#ececef"
eq "light titlebar" "$(rdcfg .config.decoration.focused_bg)" "#e6e6e9"
shot 02-macos-light
decthemectl apply "Windows 11" --mode dark >/dev/null; settle
eqpx "windows dark bar" 900 3 "#1c1c1c"
eq "windows titlebars classic" "$(rdcfg .config.decoration.style)" classic
shot 03-windows11-dark
decthemectl apply windows11 --mode light >/dev/null; settle
eqpx "windows light bar" 900 3 "#e9eef6"
shot 04-windows11-light
decthemectl apply high-contrast --mode dark >/dev/null; settle
eqpx "high contrast bar pure black" 900 3 "#000000"
decthemectl apply macos --mode dark --accent '#ff9f0a' >/dev/null; settle
eq "accent override reaches rdwm" "$(rdcfg .config.colors.focused)" "#ff9f0a"
decthemectl apply macos --reset >/dev/null; settle
eq "reset returns the blue accent" "$(rdcfg .config.colors.focused)" "#0a84ff"
eq "macOS style titlebars again" "$(rdcfg .config.decoration.style)" macos

echo "== bad input changes nothing"
before=$(cat "$DECEBAL_CONFIG/decebal.json")
decthemectl apply nonexistent >/dev/null 2>&1 && fail "unknown theme accepted" || pass "unknown theme refused"
decthemectl apply nord --accent bogus >/dev/null 2>&1 && fail "bad accent accepted" || pass "bad accent refused"
eq "selection untouched after refusals" "$(cat "$DECEBAL_CONFIG/decebal.json")" "$before"

echo "== user theme file, live edit"
mkdir -p "$XDG_CONFIG_HOME/decebal/themes"
printf 'name = "Mine"\ninherit = "nord"\n[dark]\nbar = "#102030"\naccent = "#ff00aa"\n' > "$XDG_CONFIG_HOME/decebal/themes/mine.toml"
decthemectl list | grep -q "mine" && pass "user theme listed" || fail "user theme not listed"
decthemectl apply mine >/dev/null; settle
eqpx "user theme bar" 900 3 "#102030"
printf 'name = "Mine"\ninherit = "nord"\n[dark]\nbar = "#304050"\naccent = "#ff00aa"\n' > "$XDG_CONFIG_HOME/decebal/themes/mine.toml"
settle
eqpx "editing the theme file restyles the running bar" 900 3 "#304050"
printf 'name = "Mine"\n[dark]\nbar = "nope"\n' > "$XDG_CONFIG_HOME/decebal/themes/mine.toml"
settle
eqpx "a broken theme file falls back to the default look" 900 3 "#131313"
eq "...and the bar did not crash" "$(kill -0 "${PIDS[2]}" 2>/dev/null && echo alive)" alive
printf 'name = "Mine"\n[dark]\nbar = "#405060"\n' > "$XDG_CONFIG_HOME/decebal/themes/mine.toml"
settle
eqpx "fixing the file recovers" 900 3 "#405060"
decthemectl apply macos >/dev/null; settle

echo "== popups in three themes"
for spec in "macos --mode dark:dark" "macos --mode light:light" "windows11 --mode dark:windows"; do
  args=${spec%%:*}; tag=${spec##*:}
  decthemectl apply $args >/dev/null; settle
  xdotool mousemove 14 12; sleep 0.1; xdotool click 1; sleep 0.6
  shot "10-$tag-apple-menu"
  xdotool key Escape; sleep 0.3
  xdotool mousemove 1500 12; sleep 0.1; xdotool click 1; sleep 0.8   # clock / control area
  shot "11-$tag-bar-popup"
  xdotool key Escape; sleep 0.3
done
decthemectl apply macos --mode dark >/dev/null; settle

echo "== spotlight and the dock menu"
for spec in "macos --mode dark:dark" "macos --mode light:light" "windows11 --mode dark:windows"; do
  args=${spec%%:*}; tag=${spec##*:}
  decthemectl apply $args >/dev/null; settle
  decspot >"$TMP/decspot.log" 2>&1 & SP=$!; sleep 1.5; xdotool type --delay 40 "set" ; sleep 0.8; shot "12-$tag-spotlight"; kill $SP 2>/dev/null; wait $SP 2>/dev/null
  # (declaunch closes at once on a bare Xvfb: it exits on FocusOut; its theme preview is the ignored unit test launchpad_in_three_themes)
  xdotool mousemove 696 950; sleep 0.2; xdotool click 3; sleep 0.8; shot "14-$tag-dock-menu"; xdotool key Escape; xdotool mousemove 800 400; xdotool click 1; sleep 0.3
done
decthemectl apply macos --mode dark >/dev/null; settle

echo "== settings UI"
decsettings --config "$CFG" theme >"$TMP/ms.log" 2>&1 & PIDS+=($!)
wait_for 8 test -s "$DECSETTINGS_DEBUG_LAYOUT" || fail "settings dump never written"
sleep 1
WID=$(xdotool search --sync --class DecSettings 2>/dev/null | head -1)
[ -n "$WID" ] && pass "settings window up" || fail "no settings window"
q() { jq -r "$1" "$DECSETTINGS_DEBUG_LAYOUT" 2>/dev/null; }
eq "the old name opens Appearance > Theme" "$(q '.pane + "/" + .tab')" appearance/theme
shot 20-settings-dark
. "$HERE/tests/e2e/settings_ui.sh"
open_section colours
click_ctl l:theme:dracula; settle
eq "clicking Dracula applies it" "$(theme_name)" dracula
eqpx "bar follows the UI choice" 900 3 "#1e1f29"
eq "rdwm titlebar follows the UI choice" "$(rdcfg .config.decoration.focused_bg)" "#343746"
click_ctl l:accent:#30d158; settle
eq "accent swatch applied (rdwm focused border)" "$(rdcfg .config.colors.focused)" "#30d158"
shot 21-settings-dracula-green
click_ctl l:theme:windows11; sleep 0.5
open_tab theme
click_ctl th:mode; settle
eq "selection json mode light" "$(jq -r .theme.mode "$DECEBAL_CONFIG/decebal.json")" light
eqpx "windows light bar in the running bar" 900 3 "#e9eef6"
shot 22-settings-windows-light
open_section colours
click_ctl ap:reset:colours; settle
eq "reset clears the accent override" "$(jq -r '.theme.accent // "none"' "$DECEBAL_CONFIG/decebal.json")" none

shot 24-settings-scrolled
echo "== external change while settings is open"
decthemectl apply solarized --mode dark >/dev/null; sleep 1.6
eq "settings window saw solarized (selected tile)" "$(q '[.layouts[]|select(.id=="l:theme:solarized")]|length')" 1
shot 23-settings-after-external

echo "== no crashes, idle CPU"
for p in "${PIDS[@]}"; do kill -0 "$p" 2>/dev/null || fail "process $p died"; done
grep -il "panicked" "$TMP"/*.log 2>/dev/null | head -3 | while read -r f; do fail "panic in $f"; done
echo; echo "passed $PASS, failed $FAIL (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
