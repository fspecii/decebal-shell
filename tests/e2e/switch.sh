#!/bin/bash
# macswitch (Alt+Tab) end to end against a private rdwm on a private Xvfb display.
#   tests/e2e/switch.sh               (uses ../rdwm/target/{release,debug}/rdwm, or RDWM=/path/to/rdwm)
# Everything is private: Xvfb :96 (override with E2E_DISPLAY), own XDG_RUNTIME_DIR (so rdwm's IPC socket and
# macswitch's lock/MRU files are private), own HOME. Never touches the real session; only PIDs started
# here are killed. rdwm itself starts macswitch from its Alt+Tab binding, like the real setup.
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

PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d "${TMPDIR:-/tmp}/macswitch-e2e.XXXXXX")
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
ok_within() { local n=$1 t=$2; shift 2; if wait_for "$t" "$@"; then pass "$n"; else fail "$n" "timeout: $*"; type dbg >/dev/null 2>&1 && dbg; fi; }

N=${E2E_DISPLAY:-96}
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
export MACSHELL_CONFIG=$TMP/macshell-cfg MACSHELL_BACKEND=x11
mkdir -p "$MACSHELL_CONFIG"
export PATH=$BIN:$(dirname "$RDWM"):$PATH
FAKE=$BIN/examples/fakewin

cat > "$TMP/rdwm.toml" <<TOML
[general]
layouts = ["tile", "floating"]

[[bind]]
mods = ["alt"]
key = "Tab"
action = "spawn"
args = ["$BIN/macswitch"]

[[bind]]
mods = ["alt", "shift"]
key = "Tab"
action = "spawn"
args = ["$BIN/macswitch", "--back"]
TOML
xdotool mousemove 2 2
"$RDWM" --backend x11 --config "$TMP/rdwm.toml" >"$TMP/rdwm.log" 2>&1 &
PIDS+=($!)
wait_for 5 bash -c "xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window" || { echo "rdwm did not start"; cat "$TMP/rdwm.log"; exit 2; }
wait_for 10 test -S "$XDG_RUNTIME_DIR/rdwm.sock" || { echo "rdwm has no IPC socket"; exit 2; }

ctl() { "$RDWMCTL" "$@"; }
state() { ctl state 2>/dev/null; }
focused() { state | jq -r '.state.focused // .focused // empty'; }
view() { state | jq -r '.state.monitors[0].view'; }
shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
win_of() { xdotool search --name "^$1\$" 2>/dev/null | head -1; }
sw_win() { for w in $(xdotool search --class macswitch 2>/dev/null); do xdotool getwindowgeometry --shell "$w" >/dev/null 2>&1 && { echo "$w"; return 0; }; done; return 1; }
sw_running() { pgrep -u "$(id -u)" -x macswitch >/dev/null && pgrep -x macswitch | while read -r p; do grep -qs "$XDG_RUNTIME_DIR" "/proc/$p/environ" && echo "$p"; done | grep -q .; }
sw_gone() { ! sw_running; }
geom() { xdotool getwindowgeometry --shell "$1" | tr '\n' ' '; }
pixel() { convert "$1" -crop 1x1+"$2"+"$3" +repage -depth 8 -format '%[hex:u.p{0,0}]' info: 2>/dev/null; }

fake() { # fake CLASS TITLE COLOR -> starts a test window, waits until rdwm manages it
    local before; before=$(state | jq '.state.windows | length')
    "$FAKE" "$1" "$2" "$3" >/dev/null 2>&1 &
    PIDS+=($!)
    wait_for 5 bash -c "[ \"\$($RDWMCTL state | jq '.state.windows | length')\" -gt $before ]"
    sleep 0.2
}
id_of() { win_of "$1"; }

fake alpha  "Alpha Window"  cc3333
fake beta   "Beta Window"   33cc66
fake gamma  "Gamma Window"  3366cc
A=$(id_of "Alpha Window"); B=$(id_of "Beta Window"); G=$(id_of "Gamma Window")
[ -n "$A" ] && [ -n "$B" ] && [ -n "$G" ] || { echo "test windows did not appear"; exit 2; }
echo "windows: A=$A B=$B G=$G focused=$(focused)"
eq "newest window is focused" "$(focused)" "$G"

# Expected switching order, kept in step with what macswitch does: focused first, then the order macswitch
# last left (the chosen window moves to the front), then creation order.
ORDER=("$G" "$B" "$A")   # rdwm reports real focus recency: newest first, then Beta (focused before Gamma), then Alpha
choose() { # choose INDEX: the window at INDEX becomes the first
    local c=${ORDER[$1]} o=()
    for w in "${ORDER[@]}"; do [ "$w" != "$c" ] && o+=("$w"); done
    ORDER=("$c" "${o[@]}")
}
focus_is() { [ "$(focused)" = "$1" ]; }
dbg() { echo "      order=(${ORDER[*]}) focused=$(focused) saved=$(tr '\n' ' ' < "$XDG_RUNTIME_DIR/macswitch-mru" 2>/dev/null)"; }

sync_order() { mapfile -t ORDER < <(state | jq -r '.state.mru[]'); }   # rdwm's real focus recency: what macswitch lists
alt_tab() { # alt_tab [extra key presses...]: hold Alt, tap Tab (rdwm spawns macswitch), wait until it is up, then the extras
    sync_order
    xdotool keydown alt
    xdotool key Tab
    wait_for 5 sw_win || return 1
    sleep 0.25   # the grab and first paint
    local k; for k in "$@"; do xdotool key $k; sleep 0.08; done
}
release() { xdotool keyup alt; }

# 1. one Alt+Tab goes to the previous window (second in the list), and the overlay is really there
alt_tab || fail "Alt+Tab did not start macswitch"
W=$(sw_win); read -r X Y WW HH < <(xdotool getwindowgeometry --shell "$W" | awk -F= '/^X=/{x=$2}/^Y=/{y=$2}/^WIDTH=/{w=$2}/^HEIGHT=/{h=$2}END{print x,y,w,h}')
shot overlay
echo "overlay: ${WW}x${HH}+$X+$Y"
CX=$((X + WW / 2)); CY=$((Y + HH / 2))
[ $((CX - SW / 2)) -ge -3 ] && [ $((CX - SW / 2)) -le 3 ] && [ $((CY - SH / 2)) -ge -3 ] && [ $((CY - SH / 2)) -le 3 ] && pass "overlay is centred" || fail "overlay is centred" "centre $CX,$CY"
BGPIX=$(pixel "$SHOTS/overlay.png" 5 5)
CARDPIX=$(pixel "$SHOTS/overlay.png" $((X + WW / 2)) $((Y + 6)))
[ -n "$CARDPIX" ] && [ "$CARDPIX" != "$BGPIX" ] && pass "card pixels differ from the desktop ($CARDPIX vs $BGPIX)" || fail "card pixels differ from the desktop" "$CARDPIX vs $BGPIX"
# selected cell (index 1) is highlighted against cell 0 and 2: sample the cell edge area just inside the cell
cell_x() { echo $((X + (WW - (3 * 88 + 2 * 8)) / 2 + $1 * 96)); }
P0=$(pixel "$SHOTS/overlay.png" $(($(cell_x 0) + 3)) $((Y + 20 + 44)))
P1=$(pixel "$SHOTS/overlay.png" $(($(cell_x 1) + 3)) $((Y + 20 + 44)))
P2=$(pixel "$SHOTS/overlay.png" $(($(cell_x 2) + 3)) $((Y + 20 + 44)))
[ "$P0" = "$P2" ] && [ "$P1" != "$P0" ] && pass "only the selected cell is highlighted" || fail "only the selected cell is highlighted" "$P0 $P1 $P2"
# the title area below the cells has drawn text (several distinct colours there)
NCOL=$(convert "$SHOTS/overlay.png" -crop $((WW - 40))x30+$((X + 20))+$((Y + HH - 44)) +repage -format '%k' info: 2>/dev/null)
[ "${NCOL:-0}" -gt 2 ] && pass "title text is drawn ($NCOL colours)" || fail "title text is drawn" "$NCOL colours"
release
ok_within "macswitch exits after Alt is released" 3 sw_gone
choose 1
ok_within "Alt+Tab focused the previous window" 3 focus_is "${ORDER[0]}"

# 2. alternating: Alt+Tab again comes back (MRU), two Tabs reach the third
alt_tab; release; ok_within "exit" 3 sw_gone; choose 1
ok_within "Alt+Tab again returns to the other window" 3 focus_is "${ORDER[0]}"
alt_tab Tab; release; ok_within "exit" 3 sw_gone; choose 2
ok_within "Alt+Tab Tab picks the third window" 3 focus_is "${ORDER[0]}"

# 3. Alt+Shift+Tab starts on the last entry; Shift+Tab goes back one
xdotool keydown alt keydown shift; xdotool key Tab; xdotool keyup shift
wait_for 5 sw_win || fail "Alt+Shift+Tab did not start macswitch"
sleep 0.25; release; ok_within "exit" 3 sw_gone; choose 2
ok_within "Alt+Shift+Tab picks the last window" 3 focus_is "${ORDER[0]}"
alt_tab; xdotool key shift+Tab; sleep 0.1; xdotool key shift+Tab; sleep 0.1; release; ok_within "exit" 3 sw_gone
# index 1, then shift+Tab twice: 1 -> 0 -> 2 (wrap)
choose 2
ok_within "Shift+Tab steps back and wraps" 3 focus_is "${ORDER[0]}"

# 4. Escape cancels: nothing moves, the overlay is gone even while Alt is still held
BEFORE=$(focused)
alt_tab; xdotool key Escape
ok_within "Escape closes the overlay" 3 sw_gone
release; sleep 0.3
eq "Escape leaves the focus alone" "$(focused)" "$BEFORE"

# 5. arrows and Return: Right Right wraps to the first? (index 1 -> 2), Return commits without letting go of Alt
alt_tab Right Return
ok_within "Return closes the overlay" 3 sw_gone
release; choose 2
ok_within "Right + Return commits that window" 3 focus_is "${ORDER[0]}"

# 6. a second launch while it is up moves the selection (no key involved)
alt_tab
macswitch >/dev/null 2>&1       # next
sleep 0.2
release; ok_within "exit" 3 sw_gone; choose 2
ok_within "second launch sends next" 3 focus_is "${ORDER[0]}"
alt_tab
macswitch --back >/dev/null 2>&1   # previous: from index 1 to 0, i.e. the window that is focused already
sleep 0.2
release; ok_within "exit" 3 sw_gone; choose 0
ok_within "second launch --back sends previous" 3 focus_is "${ORDER[0]}"

# 7. a click on a cell commits it
alt_tab
W=$(sw_win); read -r X Y WW HH < <(xdotool getwindowgeometry --shell "$W" | awk -F= '/^X=/{x=$2}/^Y=/{y=$2}/^WIDTH=/{w=$2}/^HEIGHT=/{h=$2}END{print x,y,w,h}')
xdotool mousemove $((X + 5)) $((Y + 5)); sleep 0.1
xdotool mousemove $(($(cell_x 2) + 44)) $((Y + 20 + 44)); sleep 0.15
xdotool click 1
ok_within "a click closes the overlay" 3 sw_gone
release; choose 2
ok_within "a click on cell 3 commits that window" 3 focus_is "${ORDER[0]}"
xdotool mousemove 2 2

# 8. a window on another tag is listed (dimmed) and Alt+Tab to it switches the view
ctl action view 2 >/dev/null; sleep 0.2
fake delta "Delta Window" cccc33
D=$(id_of "Delta Window")
ctl action view 1 >/dev/null; sleep 0.3
eq "back on tag 1" "$(view)" "1"
xdotool keydown alt; xdotool key Tab
wait_for 5 sw_win || fail "Alt+Shift+Tab did not start macswitch (tags)"
sleep 0.25
shot tags
# the last cell (the window on tag 2) is drawn dimmed: it differs from the plain ones, and the title says where it is
W=$(sw_win); read -r X Y WW HH < <(xdotool getwindowgeometry --shell "$W" | awk -F= '/^X=/{x=$2}/^Y=/{y=$2}/^WIDTH=/{w=$2}/^HEIGHT=/{h=$2}END{print x,y,w,h}')
echo "overlay with 4 windows: ${WW}x${HH}+$X+$Y"
[ "$WW" -gt 300 ] && pass "four windows -> wider card" || fail "four windows -> wider card" "$WW"
release
ok_within "exit" 3 sw_gone
ok_within "Alt+Tab reached the window on tag 2 (recently used there)" 3 focus_is "$D"
eq "the view followed to tag 2" "$(view)" "2"
shot after-tag-switch

# 9. q closes the selected window
ctl action view 1 >/dev/null; sleep 0.2
alt_tab q
release; ok_within "exit" 3 sw_gone
ok_within "q closed a window" 3 bash -c "[ \"\$($RDWMCTL state | jq '.state.windows | length')\" -lt 4 ]"

# 10. start-up: with no Alt held the grab commits right after the first paint, so the whole run is start -> paint -> commit -> exit
BEFORE=$(focused)
T0=$(date +%s%N)
"$BIN/macswitch" >/dev/null 2>&1
MS=$((($(date +%s%N) - T0) / 1000000))
[ "$MS" -lt 100 ] && pass "start, paint, commit and exit took ${MS} ms (< 100)" || fail "start-up time" "${MS} ms"
[ "$(focused)" != "$BEFORE" ] && pass "a quick tap (no Alt held at the grab) switches at once" || fail "a quick tap switches at once"

# 11. with a compositor (picom) the card is a real ARGB window: rounded corners are see-through
if command -v picom >/dev/null; then
    picom --config /dev/null --backend xrender >"$TMP/picom.log" 2>&1 &
    PICOM=$!; PIDS+=($PICOM)
    sleep 1.5
    if kill -0 $PICOM 2>/dev/null; then
        BEFORE=$(focused)
        alt_tab || fail "Alt+Tab did not start macswitch (picom)"
        sleep 0.3
        shot overlay-picom
        W=$(sw_win); read -r X Y WW HH < <(xdotool getwindowgeometry --shell "$W" | awk -F= '/^X=/{x=$2}/^Y=/{y=$2}/^WIDTH=/{w=$2}/^HEIGHT=/{h=$2}END{print x,y,w,h}')
        sleep 0.3; shot overlay-picom
        CORNER=$(pixel "$SHOTS/overlay-picom.png" "$X" "$Y"); OUTSIDE=$(pixel "$SHOTS/overlay-picom.png" $((X - 30)) $((Y + 5)))
        CARD=$(pixel "$SHOTS/overlay-picom.png" $((X + WW / 2)) $((Y + 6)))
        echo "      picom: corner=$CORNER outside-left=$OUTSIDE card=$CARD"
        [ "$CORNER" = "$OUTSIDE" ] && pass "rounded corner is see-through (real alpha)" || fail "rounded corner is see-through" "$CORNER vs $OUTSIDE"
        [ -n "$CARD" ] && [ "$CARD" != "$OUTSIDE" ] && pass "card visible under a compositor" || fail "card visible under a compositor"
        release; ok_within "exit (picom)" 3 sw_gone
        ok_within "commit works under a compositor" 3 bash -c "[ \"\$($RDWMCTL state | jq -r '.state.focused')\" != \"$BEFORE\" ]"
    else
        echo "  skip compositor phase (picom did not start: $(head -c 200 "$TMP/picom.log"))"
    fi
fi

echo; echo "passed $PASS, failed $FAIL (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
