#!/usr/bin/env bash
# This desktop test must not retint apps through an inherited session bus.
export DECEBAL_SYSTEM_AUTOMATIC=0
# The bundled pill-bar theme packages (caelestia-mocha rose-pine tokyo-night kanagawa material-you-dynamic) end to end, on a private
# Xvfb with a private rdwm, HOME and runtime dir: each is applied (dark, then light), the files it owns are checked (rdwm titlebar style and
# wallpaper, bar position, dock style, Match wallpaper for the dynamic one), undo restores everything, and a screenshot of every theme
# with decbar, decdock, two windows and decsettings is saved in $SHOTS (docs/themes/<id>.png when SHOTS=docs/themes).
#   tests/e2e/themes.sh        (SHOTS=dir keeps the PNGs; E2E_DISPLAY=:87 by default; PROFILE=release for a release build)
# Only PIDs started here are killed; the live session is never touched (DISPLAY :0 and :1 are refused).
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
PROFILE=${PROFILE:-debug}
BIN=${CARGO_TARGET_DIR:-$HERE/target}/$PROFILE
RDWM_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
[ -x "$RDWM_DIR/rdwm" ] || { echo "rdwm not found (set RDWM_BIN_DIR)"; exit 77; }
for t in Xvfb xdotool import jq convert; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
if [ "$PROFILE" = release ]; then (cd "$HERE" && cargo build -q --offline --release --bins --examples) || { echo "build failed"; exit 2; }
else (cd "$HERE" && cargo build -q --offline --bins --examples) || { echo "build failed"; exit 2; }; fi
N=${E2E_DISPLAY:-:87}
case "$N" in :0|:1) echo "refusing the live display"; exit 2;; esac

IDS="caelestia-mocha rose-pine tokyo-night kanagawa material-you-dynamic"
PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d "${TMPDIR:-/tmp}/decebal-themes-e2e.XXXXXX")
SHOTS=${SHOTS:-$TMP/shots}; mkdir -p "$SHOTS"
cleanup() { for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done; sleep 0.3; for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done; wait 2>/dev/null
  [ "$SHOTS" = "$TMP/shots" ] && rm -rf "$TMP" || find "$TMP" -mindepth 1 -maxdepth 1 ! -path "$SHOTS" -exec rm -rf {} +; }
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "got '$2', want '$3'"; fi; }
has() { if printf '%s' "$2" | grep -q -- "$3"; then pass "$1"; else fail "$1" "no '$3' in: $(printf '%s' "$2" | head -c 300)"; fi; }
wait_for() { local t=$1; shift; local end=$(($(date +%s%N) / 1000000 + t * 1000)); while [ $(($(date +%s%N) / 1000000)) -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 0.05; done; return 1; }

export HOME=$TMP/home XDG_CONFIG_HOME=$TMP/cfg XDG_RUNTIME_DIR=$TMP/run XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata XDG_STATE_HOME=$TMP/state XDG_CACHE_HOME=$TMP/cache
export DECEBAL_CONFIG=$TMP/cfg/omarchy DECEBAL_BACKEND=x11 DISPLAY=$N RDWMCTL=$RDWM_DIR/rdwmctl DECSETTINGS_CONFIG=$TMP/cfg/rdwm/config.toml
export DECEBAL_COLOR_SCHEME=dark DECEBAL_SYSTEM_THEMES=$TMP/system-themes
unset WAYLAND_DISPLAY RDWM_SOCK
mkdir -p "$HOME" "$XDG_CONFIG_HOME/rdwm" "$DECEBAL_CONFIG" "$XDG_RUNTIME_DIR" "$XDG_DATA_HOME/applications" "$TMP/nodata" "$DECEBAL_SYSTEM_THEMES"
chmod 700 "$XDG_RUNTIME_DIR"
export PATH="$RDWM_DIR:$BIN:$PATH"
CFG=$DECSETTINGS_CONFIG
# what `make install` does: the packages, with their wallpapers, in the system theme folder
for id in $IDS; do cp -r "$HERE/themes/$id" "$DECEBAL_SYSTEM_THEMES/$id"; done
printf '# test config\n[general]\ngap_inner = 6\n\n[decoration]\nenabled = true\n' > "$CFG"
printf '{"version":1,"pinned":["launchpad","trash"],"order":["launchpad","trash"]}\n' > "$DECEBAL_CONFIG/dock-pinned-macos.json"

Xvfb $N -screen 0 1600x900x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X${N#:} || { echo "Xvfb failed"; exit 2; }
xdotool mousemove 800 450
rdwm --backend x11 --config "$CFG" >"$TMP/rdwm.log" 2>&1 & PIDS+=($!)
wait_for 5 bash -c "xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window" || { echo "rdwm did not start"; cat "$TMP/rdwm.log"; exit 2; }
decbar >"$TMP/decbar.log" 2>&1 & PIDS+=($!)
decdock >"$TMP/decdock.log" 2>&1 & PIDS+=($!)
sleep 2
fakewin Terminal "Terminal" 1d2030 >/dev/null 2>&1 & PIDS+=($!)
sleep 0.4
fakewin Files "Files" 2b3550 >/dev/null 2>&1 & PIDS+=($!)
decsettings --config "$CFG" theme >"$TMP/decsettings.log" 2>&1 & PIDS+=($!)
sleep 2

# a value of rdwm's config.toml: tomlv SECTION KEY
tomlv() { awk -v sec="[$1]" -v key="$2" '/^\[/ { in_s = ($0 == sec) } in_s && $1 == key { sub(/^[^=]*= *"?/, ""); sub(/"? *(#.*)?$/, ""); print; exit }' "$CFG"; }
shell() { jq -r "$1" "$DECEBAL_CONFIG/decebal.json"; }
# theme id | rdwm [decoration] style | bar position | dock style | dock enabled
EXPECT="caelestia-mocha materia left mac auto
rose-pine macos top mac true
tokyo-night nord split mac true
kanagawa minimal right windows true
material-you-dynamic breeze bottom mac true"

echo "== every package validates"
for id in $IDS; do out=$(decthemectl theme validate "$HERE/themes/$id" 2>&1); has "$id validates" "$out" "^ok:"; done
decthemectl theme list | grep -c "caelestia-mocha\|rose-pine\|tokyo-night\|kanagawa\|material-you-dynamic" | { read n; eq "all five are listed" "$n" 5; }

echo "== apply each: files, then a screenshot"
printf '%s\n' "$EXPECT" | while read -r id deco pos dstyle den; do
  out=$(decthemectl theme apply "$id" 2>&1); has "$id applies" "$out" "applied $id"
  sleep 2.5
  eq "$id: titlebar style" "$(tomlv decoration style)" "$deco"
  eq "$id: bar position" "$(shell '.bar.position')" "$pos"
  eq "$id: dock style" "$(jq -r .style "$DECEBAL_CONFIG/dock-settings.json")" "$dstyle"
  eq "$id: dock enabled" "$(shell '.dock.enabled')" "$den"
  eq "$id: selected" "$(shell '.theme.name // .theme')" "$id"
  wp=$(tomlv wallpaper image); has "$id: wallpaper comes from the package" "$wp" "$id/wallpapers/"
  [ -f "$wp" ] && pass "$id: wallpaper file exists" || fail "$id: wallpaper file" "$wp"
  import -window root "$SHOTS/$id.png" 2>/dev/null
  decthemectl apply "$id" --mode light >/dev/null 2>&1; sleep 1.5
  eq "$id: light mode keeps the bar" "$(shell '.bar.position')" "$pos"
  import -window root "$SHOTS/$id-light.png" 2>/dev/null
  decthemectl apply "$id" --mode dark >/dev/null 2>&1; sleep 0.5
done 2>&1 | tee "$TMP/loop.out"
PASS=$((PASS + $(grep -c '^  ok' "$TMP/loop.out"))); FAIL=$((FAIL + $(grep -c '^  FAIL' "$TMP/loop.out")))

echo "== Match wallpaper follows the dynamic theme and goes off with the next one"
decthemectl theme apply material-you-dynamic >/dev/null 2>&1; sleep 1
eq "dynamic is on" "$(shell '.theme.dynamic.enabled')" true
eq "scheme is vibrant" "$(shell '.theme.dynamic.scheme')" vibrant
decthemectl theme apply rose-pine >/dev/null 2>&1; sleep 1
eq "dynamic is off again for the next theme" "$(shell '.theme.dynamic.enabled // false')" false

echo "== undo puts the previous theme back"
decthemectl theme apply kanagawa >/dev/null 2>&1; sleep 1
decthemectl theme undo >/dev/null 2>&1; sleep 1
eq "undo restores rose-pine" "$(shell '.theme.name // .theme')" rose-pine

echo
echo "$PASS passed, $FAIL failed (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
