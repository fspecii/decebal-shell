#!/usr/bin/env bash
# This desktop test must not retint apps through an inherited session bus.
export DECEBAL_SYSTEM_AUTOMATIC=0
# The showcase themes (themes/<id>/ with optional assets) end to end, on a private Xvfb with a private rdwm, HOME and runtime dir:
# validate, install, apply (dark and light), check rdwm's config (style, wallpaper, borders), that the bar and dock survive, and take
# one screenshot per theme (two fake windows, decsettings, bar, dock) into $SHOTS.
#   tests/e2e/showcase-themes.sh     (SHOTS=dir keeps the PNGs; E2E_DISPLAY=:173 by default; PROFILE=release by default)
# Only PIDs started here are killed; the live session is never touched (DISPLAY :0 and :1 are refused).
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
PROFILE=${PROFILE:-release}
BIN=${CARGO_TARGET_DIR:-$HERE/target}/$PROFILE
RDWM_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
[ -x "$RDWM_DIR/rdwm" ] || { echo "rdwm not found (set RDWM_BIN_DIR)"; exit 77; }
for t in Xvfb xdotool import jq convert; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
if [ "$PROFILE" = release ]; then (cd "$HERE" && cargo build -q --offline --release --bins --examples) || { echo "build failed"; exit 2; }
else (cd "$HERE" && cargo build -q --offline --bins --examples) || { echo "build failed"; exit 2; }; fi
N=${E2E_DISPLAY:-:173}
case "$N" in :0|:1) echo "refusing the live display"; exit 2;; esac
THEME_IDS=${THEME_IDS:-"gruvbox-dark everforest synthwave frutiger-aero gnome-adwaita-like"}

PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d /tmp/decebal-showcase-e2e.XXXXXX)
SHOTS=${SHOTS:-$TMP/shots}; mkdir -p "$SHOTS"
cleanup() { for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done; sleep 0.3; for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done; wait 2>/dev/null
  [ "$SHOTS" = "$TMP/shots" ] && rm -rf "$TMP" || find "$TMP" -mindepth 1 -maxdepth 1 ! -path "$SHOTS" -exec rm -rf {} +; }
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "got '$2', want '$3'"; fi; }
has() { if printf '%s' "$2" | grep -q -- "$3"; then pass "$1"; else fail "$1" "no '$3' in: $(printf '%s' "$2" | head -c 300)"; fi; }
wait_for() { local t=$1; shift; local end=$(($(date +%s%N) / 1000000 + t * 1000)); while [ $(($(date +%s%N) / 1000000)) -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 0.05; done; return 1; }

export HOME=$TMP/home XDG_CONFIG_HOME=$TMP/cfg XDG_RUNTIME_DIR=$TMP/run XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata XDG_STATE_HOME=$TMP/state
export DECEBAL_CONFIG=$TMP/cfg/omarchy DECEBAL_BACKEND=x11 DISPLAY=$N RDWMCTL=$RDWM_DIR/rdwmctl DECSETTINGS_CONFIG=$TMP/cfg/rdwm/config.toml
export DECEBAL_COLOR_SCHEME=dark DECEBAL_SYSTEM_THEMES=$TMP/nosystem
unset WAYLAND_DISPLAY RDWM_SOCK
mkdir -p "$HOME" "$XDG_CONFIG_HOME/rdwm" "$DECEBAL_CONFIG" "$XDG_RUNTIME_DIR" "$XDG_DATA_HOME/applications" "$TMP/nodata"
chmod 700 "$XDG_RUNTIME_DIR"
export PATH="$RDWM_DIR:$BIN:$PATH"
CFG=$DECSETTINGS_CONFIG
printf '# test config\n[general]\ngap_inner = 6\n\n[decoration]\nenabled = true\n' > "$CFG"
FAKE=$BIN/examples/fakewin
for a in Terminal:2b2b2b Files:e8e8e8; do n=${a%%:*}; c=${a##*:}; printf '[Desktop Entry]\nType=Application\nName=%s\nExec=%s %s "%s" %s\nStartupWMClass=%s\n' "$n" "$FAKE" "$n" "$n" "$c" "$n" > "$XDG_DATA_HOME/applications/$n.desktop"; done
printf '{"version":1,"pinned":["launchpad","Terminal","Files","trash"],"order":["launchpad","Terminal","Files","trash"]}\n' > "$DECEBAL_CONFIG/dock-pinned-macos.json"

Xvfb $N -screen 0 1600x900x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X${N#:} || { echo "Xvfb failed"; exit 2; }
xdotool mousemove 1500 880
rdwm --backend x11 --config "$CFG" >"$TMP/rdwm.log" 2>&1 & PIDS+=($!)
wait_for 5 bash -c "xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window" || { echo "rdwm did not start"; cat "$TMP/rdwm.log"; exit 2; }
decbar >"$TMP/decbar.log" 2>&1 & BAR=$!; PIDS+=($BAR)
decdock >"$TMP/decdock.log" 2>&1 & DOCK=$!; PIDS+=($DOCK)
"$FAKE" Terminal "Terminal" 2b2b2b >/dev/null 2>&1 & PIDS+=($!)
"$FAKE" Files "Files" e8e8e8 >/dev/null 2>&1 & PIDS+=($!)
decsettings --config "$CFG" >"$TMP/decsettings.log" 2>&1 & PIDS+=($!)
sleep 3

shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
rdcfg() { rdwmctl config | jq -r "$1"; }
alive() { kill -0 "$1" 2>/dev/null; }

for id in $THEME_IDS; do
  echo "== $id"
  PKG=$HERE/themes/$id
  has "validates" "$(decthemectl theme validate "$PKG" 2>&1)" "^ok:"
  has "installs" "$(decthemectl theme install "$PKG" 2>&1)" "installed $id"
  before_wallpaper=$(awk '/^\[wallpaper\]/{f=1;next}/^\[/{f=0}f' "$CFG")
  has "applies" "$(decthemectl theme apply "$id" 2>&1)" "applied $id"
  sleep 2
  tget() { awk -v sec="[$1]" -v key="$2" '$0==sec{f=1;next}/^\[/{f=0}f&&$1==key{gsub(/"/,"",$3);print $3;exit}' "$3"; }
  eq "rdwm titlebar style" "$(tget decoration style "$CFG")" "$(tget decoration style "$PKG/theme.toml")"
  if grep -q '^\[wallpaper\]' "$PKG/theme.toml"; then
    has "legacy package wallpaper applies" "$(grep -A4 '^\[wallpaper' "$CFG")" "wallpapers/"
  else
    eq "independent wallpaper preferences preserved" "$(awk '/^\[wallpaper\]/{f=1;next}/^\[/{f=0}f' "$CFG")" "$before_wallpaper"
  fi
  eq "rdwm focused border" "$(tget colors focused "$CFG" | tr 'A-F' 'a-f')" "$(tget rdwm focused_border "$PKG/theme.toml")"
  alive $BAR && pass "decbar is alive" || fail "decbar died"
  alive $DOCK && pass "decdock is alive" || fail "decdock died"
  shot "$id-dark"
  if grep -q '^\[colors.light\]' "$PKG/theme.toml"; then
    has "applies in light" "$(decthemectl apply "$id" --mode light 2>&1)" "light"
    sleep 2; shot "$id-light"
    decthemectl apply "$id" --mode dark >/dev/null 2>&1; sleep 1
  fi
  has "undo" "$(decthemectl theme undo 2>&1)" "."
done
echo; echo "passed $PASS, failed $FAIL (screenshots in $SHOTS)"
[ $FAIL -eq 0 ]
