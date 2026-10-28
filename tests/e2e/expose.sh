#!/bin/bash
# decexpose (window overview) end to end against a private rdwm on a private Xvfb display.
#   tests/e2e/expose.sh               (uses ../rdwm/target/{release,debug}/rdwm, or RDWM=/path/to/rdwm)
# Everything is private, scaffolding as in switch.sh: Xvfb :97 (override with E2E_DISPLAY), own XDG_RUNTIME_DIR (so rdwm's
# IPC socket and decexpose's lock file are private), own HOME. Never touches the real session; only PIDs started here
# are killed. Four coloured fakewin windows are tiled by rdwm; decexpose is started by hand (no key binding needed).
# Screenshots land in $SHOTS (default $TMP/shots; keep with KEEP_TMP=1 or SHOTS=dir).
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
TARGET=${CARGO_TARGET_DIR:-$HERE/target}
PROFILE=${PROFILE:-release}
BIN=$TARGET/$PROFILE
RDWM=${RDWM:-$(ls "$HERE"/../rdwm/target/release/rdwm "$HERE"/../rdwm/target/debug/rdwm "$HOME"/Projects/rdwm/target/release/rdwm 2>/dev/null | head -1)}
[ -x "$RDWM" ] || { echo "rdwm binary not found (set RDWM)"; exit 77; }
RDWMCTL=${RDWMCTL:-$(dirname "$RDWM")/rdwmctl}
for t in Xvfb xdotool import xprop jq convert; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline $([ "$PROFILE" = release ] && echo --release) --bins --examples) || { echo "build failed"; exit 2; }

PASS=0; FAIL=0; PIDS=(); EXPID=
TMP=$(mktemp -d "${TMPDIR:-/tmp}/decexpose-e2e.XXXXXX")
SHOTS=${SHOTS:-$TMP/shots}; mkdir -p "$SHOTS"
cleanup() {
    for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done
    sleep 0.2
    for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done
    wait 2>/dev/null
    [ -n "${KEEP_TMP:-}" ] && echo "kept $TMP" || { [ "$SHOTS" = "$TMP/shots" ] && rm -rf "$TMP" || find "$TMP" -mindepth 1 -maxdepth 1 ! -path "$SHOTS" -exec rm -rf {} +; }
}
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "got '$2', want '$3'"; fi; }
wait_for() { local t=$1; shift; local end=$(($(date +%s%N) / 1000000 + t * 1000)); while [ $(($(date +%s%N) / 1000000)) -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 0.05; done; return 1; }
ok_within() { local n=$1 t=$2; shift 2; if wait_for "$t" "$@"; then pass "$n"; else fail "$n" "timeout: $*"; fi; }

N=${E2E_DISPLAY:-97}
[ "$N" = 1 ] && { echo "refusing the live display"; exit 2; }
SW=1280; SH=800
Xvfb :$N -screen 0 ${SW}x${SH}x24 +extension RANDR +extension SHAPE +extension Composite -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X$N || { echo "Xvfb failed"; cat "$TMP/xvfb.log"; exit 2; }
export DISPLAY=:$N
unset WAYLAND_DISPLAY RDWM_SOCK
export XDG_RUNTIME_DIR=$TMP/run XDG_CONFIG_HOME=$TMP/cfg HOME=$TMP/home XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata XDG_CACHE_HOME=$TMP/cache
mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME" "$HOME" "$XDG_DATA_HOME/applications" "$TMP/nodata" "$XDG_CACHE_HOME"
chmod 700 "$XDG_RUNTIME_DIR"
export DECEBAL_CONFIG=$TMP/decebal-cfg DECEBAL_BACKEND=x11
mkdir -p "$DECEBAL_CONFIG"
export PATH=$BIN:$(dirname "$RDWM"):$PATH
FAKE=$BIN/examples/fakewin

cat > "$TMP/rdwm.toml" <<TOML
[general]
layouts = ["tile", "floating"]
TOML
xdotool mousemove 2 2
"$RDWM" --backend x11 --config "$TMP/rdwm.toml" >"$TMP/rdwm.log" 2>&1 &
PIDS+=($!)
wait_for 5 bash -c "xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window" || { echo "rdwm did not start"; cat "$TMP/rdwm.log"; exit 2; }
wait_for 10 test -S "$XDG_RUNTIME_DIR/rdwm.sock" || { echo "rdwm has no IPC socket"; exit 2; }

ctl() { "$RDWMCTL" "$@"; }
state() { ctl state 2>/dev/null; }
focused() { state | jq -r '.state.focused // empty'; }
view() { state | jq -r '.state.monitors[0].view'; }
nwin() { state | jq '.state.windows | length'; }
shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
win_of() { xdotool search --name "^$1\$" 2>/dev/null | head -1; }
pixel() { convert "$1" -crop 1x1+"$2"+"$3" +repage -depth 8 -format '%[hex:u.p{0,0}]' info: 2>/dev/null; }
# Fraction of the screenshot $1 covered by colour $2 (RRGGBB, within 3 %): the marker colour goes into the red channel.
frac() { convert "$1" -fuzz 3% -fill '#fe0000' -opaque "#$2" -fuzz 0 -fill black +opaque '#fe0000' -channel R -separate +channel -format '%[fx:mean]' info: 2>/dev/null; }
present() { awk -v f="$(frac "$1" "$2")" 'BEGIN { exit !(f > 0.01) }'; }
absent() { awk -v f="$(frac "$1" "$2")" 'BEGIN { exit !(f < 0.002) }'; }
# Bounding box (WxH+X+Y) of colour $2 in screenshot $1.
box() { convert "$1" -fuzz 3% -fill '#fe0000' -opaque "#$2" -fuzz 0 -fill black +opaque '#fe0000' -trim -format '%wx%h+%[fx:page.x]+%[fx:page.y]' info: 2>/dev/null; }
click_box() { # click_box SHOT COLOR: click the middle of that colour's thumbnail
    local b; b=$(box "$1" "$2"); local w h x y
    IFS='x+' read -r w h x y <<<"$b"
    xdotool mousemove $((x + w / 2 - 5)) $((y + h / 2 - 5)); sleep 0.1
    xdotool mousemove $((x + w / 2)) $((y + h / 2)); sleep 0.15
    xdotool click 1
}

fake() { # fake CLASS TITLE COLOR -> starts a test window, waits until rdwm manages it
    local before; before=$(nwin)
    "$FAKE" "$1" "$2" "$3" >/dev/null 2>&1 &
    PIDS+=($!)
    wait_for 5 bash -c "[ \"\$($RDWMCTL state | jq '.state.windows | length')\" -gt $before ]"
    sleep 0.2
}

ex_start() { # ex_start [args...]: run decexpose in the background, wait until its overlay and labels are up
    "$BIN/decexpose" "$@" >"$TMP/decexpose.out" 2>&1 &
    EXPID=$!; PIDS+=($EXPID)
    wait_for 5 ex_win || return 1
    sleep 0.5
}
ex_win() { for w in $(xdotool search --class decexpose 2>/dev/null); do xdotool getwindowgeometry --shell "$w" >/dev/null 2>&1 && { echo "$w"; return 0; }; done; return 1; }
ex_running() { [ -n "$EXPID" ] && [ -d "/proc/$EXPID" ] && ! grep -q '^State:.*Z' "/proc/$EXPID/status"; }
ex_gone() { ! ex_running; }
focus_is() { [ "$(focused)" = "$1" ]; }

fake alpha "Alpha Window" cc3333
fake beta  "Beta Window"  33cc66
fake gamma "Gamma Window" 3366cc
fake delta "Delta Window" cccc33
A=$(win_of "Alpha Window"); B=$(win_of "Beta Window"); G=$(win_of "Gamma Window"); D=$(win_of "Delta Window")
[ -n "$A" ] && [ -n "$B" ] && [ -n "$G" ] && [ -n "$D" ] || { echo "test windows did not appear"; exit 2; }
echo "windows: A=$A B=$B G=$G D=$D focused=$(focused)"
eq "newest window is focused" "$(focused)" "$D"

# 1. the overview covers the screen and every window's colour shows up in a thumbnail
ex_start || fail "decexpose did not start"
W=$(ex_win); read -r X Y WW HH < <(xdotool getwindowgeometry --shell "$W" | awk -F= '/^X=/{x=$2}/^Y=/{y=$2}/^WIDTH=/{w=$2}/^HEIGHT=/{h=$2}END{print x,y,w,h}')
eq "overlay covers the screen" "${WW}x${HH}+$X+$Y" "${SW}x${SH}+0+0"
shot overview
for c in cc3333 33cc66 3366cc cccc33; do present "$SHOTS/overview.png" $c && pass "thumbnail with colour $c" || fail "thumbnail with colour $c" "fraction $(frac "$SHOTS/overview.png" $c)"; done
BG=$(pixel "$SHOTS/overview.png" 4 4)
[ -n "$BG" ] && [ "$BG" != "FFFFFF" ] && [ "$BG" != "CC3333" ] && pass "the background is dimmed ($BG)" || fail "the background is dimmed" "$BG"
# thumbnails are smaller than the real windows (tiled: about half the screen each)
read -r bw bh _ <<<"$(box "$SHOTS/overview.png" cc3333 | tr 'x+' '  ')"
[ "$bw" -lt 620 ] && [ "$bw" -gt 100 ] && pass "thumbnail is scaled down (${bw}x${bh})" || fail "thumbnail is scaled down" "${bw}x${bh}"
# text: the labels under the thumbnails are drawn (many distinct colours near the bottom of a thumbnail)
read -r _ _ bx by <<<"$(box "$SHOTS/overview.png" cc3333 | tr 'x+' '  ')"
NCOL=$(convert "$SHOTS/overview.png" -crop "${bw}x30+${bx}+$((by + bh + 2))" +repage -format '%k' info: 2>/dev/null)
[ "${NCOL:-0}" -gt 3 ] && pass "labels are drawn ($NCOL colours)" || fail "labels are drawn" "$NCOL colours"

# 2. a click on a thumbnail switches to that window
click_box "$SHOTS/overview.png" cc3333
ok_within "a click closes the overview" 3 ex_gone
ok_within "the clicked window (Alpha) is focused" 3 focus_is "$A"
# with focus-follows-mouse the pointer has to end up inside the chosen window, or it would take the focus back
sleep 0.3
read -r WX WY WWID WHEI < <(xdotool getwindowgeometry --shell "$A" | awk -F= '/^X=/{x=$2}/^Y=/{y=$2}/^WIDTH=/{w=$2}/^HEIGHT=/{h=$2}END{print x,y,w,h}')
read -r MX MY < <(xdotool getmouselocation | sed 's/x:\([0-9]*\) y:\([0-9]*\).*/\1 \2/')
{ [ "$MX" -ge "$WX" ] && [ "$MX" -lt "$((WX + WWID))" ] && [ "$MY" -ge "$WY" ] && [ "$MY" -lt "$((WY + WHEI))" ]; } && pass "the pointer was moved into the chosen window" || fail "the pointer was moved into the chosen window" "pointer $MX,$MY window $WX,$WY ${WWID}x$WHEI"
eq "and the focus stayed there" "$(focused)" "$A"

# 3. Escape cancels and restores the previous focus even when the pointer is over another window
ctl action focuswindow "$B" >/dev/null; sleep 0.2
xdotool mousemove 1000 700; sleep 0.2          # over the bottom-right (tiled) window
ctl action focuswindow "$B" >/dev/null; sleep 0.2
BEFORE=$(focused)
ex_start || fail "decexpose did not start (escape)"
xdotool key Escape
ok_within "Escape closes the overview" 3 ex_gone
sleep 0.4
eq "Escape leaves the focus where it was" "$(focused)" "$BEFORE"
xdotool mousemove 2 2

# 4. typing filters: only the matching window is left
ex_start || fail "decexpose did not start (filter)"
xdotool type --delay 40 gam
sleep 0.4
shot filter
present "$SHOTS/filter.png" 3366cc && pass "filter keeps the matching window" || fail "filter keeps the matching window"
for c in cc3333 33cc66 cccc33; do absent "$SHOTS/filter.png" $c && pass "filter hides colour $c" || fail "filter hides colour $c" "fraction $(frac "$SHOTS/filter.png" $c)"; done
# the search pill is drawn at the top
NCOL=$(convert "$SHOTS/filter.png" -crop 360x34+460+18 +repage -format '%k' info: 2>/dev/null)
[ "${NCOL:-0}" -gt 3 ] && pass "search pill is drawn" || fail "search pill is drawn" "$NCOL colours"
xdotool key BackSpace; sleep 0.2; xdotool key BackSpace; sleep 0.2; xdotool key BackSpace; sleep 0.2
shot unfiltered
present "$SHOTS/unfiltered.png" cc3333 && pass "Backspace brings the others back" || fail "Backspace brings the others back"
xdotool type --delay 40 gamma; sleep 0.2
xdotool key Return
ok_within "Return commits the filtered window" 3 ex_gone
ok_within "Gamma is focused" 3 focus_is "$G"

# 5. w closes the selected window (the focused one is selected first); the overview stays up with the others
ctl action focuswindow "$G" >/dev/null; sleep 0.2
N0=$(nwin)
ex_start || fail "decexpose did not start (close)"
xdotool key w
ok_within "w closed the selected window" 3 bash -c "[ \"\$($RDWMCTL state | jq '.state.windows | length')\" -lt $N0 ]"
sleep 0.3
ex_running && pass "the overview stays open after closing" || fail "the overview stays open after closing"
shot closed
absent "$SHOTS/closed.png" 3366cc && pass "its thumbnail is gone" || fail "its thumbnail is gone"
present "$SHOTS/closed.png" cc3333 && pass "the others stay" || fail "the others stay"
xdotool key Escape
ok_within "exit" 3 ex_gone
[ -z "$(win_of 'Gamma Window')" ] && pass "the window really went" || fail "the window really went"

# 6. --all: a window on tag 2 gets a card and its workspace chip lights up; chips and cells switch the view
ctl action view 2 >/dev/null; sleep 0.2
fake epsilon "Epsilon Window" cc33cc
E=$(win_of "Epsilon Window")
ctl action view 1 >/dev/null; sleep 0.3
eq "back on tag 1" "$(view)" "1"
ex_start --all || fail "decexpose --all did not start"
shot all
chip_x() { echo $((402 + ($1 - 1) * 54 + 8)); }
P1=$(pixel "$SHOTS/all.png" "$(chip_x 1)" 37); P2=$(pixel "$SHOTS/all.png" "$(chip_x 2)" 37); P5=$(pixel "$SHOTS/all.png" "$(chip_x 5)" 37)
echo "      chips: 1=$P1 2=$P2 5=$P5"
[ "$P2" != "$P5" ] && [ "$P1" != "$P5" ] && [ "$P1" = "$P2" ] && pass "occupied chips 1 and 2 are highlighted, 5 is not" || fail "occupied chips are highlighted" "$P1 $P2 $P5"
absent "$SHOTS/all.png" cc33cc && pass "the window on tag 2 is a card, not a thumbnail" || fail "the window on tag 2 is a card"
present "$SHOTS/all.png" cc3333 && pass "windows of the current tag show thumbnails" || fail "windows of the current tag show thumbnails"
# click chip 2 -> view 2
xdotool mousemove $(($(chip_x 2) + 14)) 30; sleep 0.1
xdotool mousemove $(($(chip_x 2) + 16)) 37; sleep 0.15
xdotool click 1
ok_within "a chip click closes the overview" 3 ex_gone
ok_within "the chip switched the view to tag 2" 3 bash -c "[ \"\$($RDWMCTL state | jq '.state.monitors[0].view')\" = 2 ]"
ctl action view 1 >/dev/null; sleep 0.3
# End selects the last cell (the window on tag 2); Return commits: view first, then focus
ex_start --all || fail "decexpose --all did not start (commit)"
xdotool key End; sleep 0.2
xdotool key Return
ok_within "Return closes the overview" 3 ex_gone
ok_within "committing a window of tag 2 focuses it" 3 focus_is "$E"
eq "and switched the view to tag 2" "$(view)" "2"
ctl action view 1 >/dev/null; sleep 0.3

# 7. running it again closes it (toggle), and the focus stays
BEFORE=$(focused)
ex_start || fail "decexpose did not start (toggle)"
FIRST=$EXPID
"$BIN/decexpose" >/dev/null 2>&1
ok_within "a second launch closes the first" 3 ex_gone
EXPID=$FIRST; sleep 0.3
eq "toggling leaves the focus alone" "$(focused)" "$BEFORE"

# 8. start-up: ten windows -> first frame within ~120 ms
for i in 1 2 3 4 5 6; do fake extra "Extra $i" 666666; done
eq "ten windows" "$(nwin)" "10"
DECEXPOSE_TIMING=1 "$BIN/decexpose" >"$TMP/timing.out" 2>&1 &
EXPID=$!; PIDS+=($EXPID)
wait_for 5 ex_win; sleep 0.5
MS=$(grep -o 'painted [0-9]*' "$TMP/timing.out" | grep -o '[0-9]*')
xdotool key Escape; ok_within "exit" 3 ex_gone
echo "      first frame after ${MS:-?} ms"
[ -n "$MS" ] && [ "$MS" -lt 120 ] && pass "first frame within 120 ms (${MS} ms, 10 windows)" || fail "first frame within 120 ms" "${MS:-none} ms"

echo; echo "passed $PASS, failed $FAIL (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
