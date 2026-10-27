#!/bin/bash
# machud (volume / brightness / mic on-screen display) end to end on a private Xvfb, no window manager.
#   tests/e2e/hud.sh                  (E2E_DISPLAY=98 by default; SHOTS=dir keeps the screenshots)
# Everything is private: own Xvfb display, own XDG_RUNTIME_DIR / HOME / config, wpctl + brightnessctl are shim
# scripts in a temp PATH (the real audio stack and backlight are never read). Only PIDs started here are killed.
# Never touches the live display (:1) or Hyprland: WAYLAND_DISPLAY is unset and DISPLAY points at the private server.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
TARGET=${CARGO_TARGET_DIR:-$HERE/target}
BIN=$TARGET/${PROFILE:-release}
for t in Xvfb xdotool import convert jq; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline $([ "${PROFILE:-release}" = release ] && echo --release) --bin machud --example fakewin) || { echo "build failed"; exit 2; }

N=${E2E_DISPLAY:-98}
[ "$N" = 1 ] && { echo "refusing the live display"; exit 2; }
PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d "${TMPDIR:-/tmp}/machud-e2e.XXXXXX")
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
now_ms() { echo $(($(date +%s%N) / 1000000)); }
wait_for() { local t=$1; shift; local end=$(($(now_ms) + t * 1000)); while [ "$(now_ms)" -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 0.02; done; return 1; }

SW=800; SH=600
Xvfb :$N -screen 0 ${SW}x${SH}x24 +extension RANDR +extension SHAPE +extension Composite -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X$N || { echo "Xvfb failed"; cat "$TMP/xvfb.log"; exit 2; }
export DISPLAY=:$N
unset WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE
export XDG_RUNTIME_DIR=$TMP/run XDG_CONFIG_HOME=$TMP/cfg HOME=$TMP/home XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata XDG_CACHE_HOME=$TMP/cache
mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME" "$HOME" "$TMP/nodata" "$XDG_CACHE_HOME" "$TMP/state" "$TMP/shim"
chmod 700 "$XDG_RUNTIME_DIR"
export MACSHELL_CONFIG=$TMP/macshell-cfg MACSHELL_BACKEND=x11 MACSHELL_COLOR_SCHEME=dark
mkdir -p "$MACSHELL_CONFIG"
export MACHUD_BACKLIGHT_DIR=$TMP/nobacklight

# Shims: state in $TMP/state/{vol,mute,mic,micmute,bright}; SHIM_FAIL=1 makes every tool fail.
cat > "$TMP/shim/wpctl" <<'SH'
#!/bin/bash
[ -n "${SHIM_FAIL:-}" ] && exit 1
S=${SHIM_STATE:?}
[ "$1" = get-volume ] || exit 1
case "$2" in
@DEFAULT_AUDIO_SINK@) v=$(<"$S/vol"); m=$(<"$S/mute");;
@DEFAULT_AUDIO_SOURCE@) v=$(<"$S/mic"); m=$(<"$S/micmute");;
*) exit 1;;
esac
[ "$m" = 1 ] && echo "Volume: $v [MUTED]" || echo "Volume: $v"
SH
cat > "$TMP/shim/brightnessctl" <<'SH'
#!/bin/bash
[ -n "${SHIM_FAIL:-}" ] && exit 1
S=${SHIM_STATE:?}
[ "$1" = -m ] && echo "fake_backlight,backlight,$(<"$S/bright"),$(<"$S/bright")%,100"
SH
cat > "$TMP/shim/pactl" <<'SH'
#!/bin/bash
exit 1
SH
chmod +x "$TMP/shim/"*
export SHIM_STATE=$TMP/state
setv() { echo "$1" > "$SHIM_STATE/vol"; echo "${2:-0}" > "$SHIM_STATE/mute"; }
setmic() { echo "$1" > "$SHIM_STATE/mic"; echo "${2:-0}" > "$SHIM_STATE/micmute"; }
setb() { echo "$1" > "$SHIM_STATE/bright"; }
setv 0.50; setmic 0.25; setb 70
# Only the shims and the freshly built binaries: no real wpctl / pactl / brightnessctl can be found.
export PATH=$TMP/shim:$BIN:/usr/bin:/bin

shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
hud_win() { for w in $(xdotool search --name '^machud$' 2>/dev/null); do echo "$w"; return 0; done; return 1; }
hud_gone() { ! hud_win; }
n_wins() { xdotool search --name '^machud$' 2>/dev/null | wc -l; }
read_geom() { eval "$(xdotool getwindowgeometry --shell "$1" | grep -E '^(X|Y|WIDTH|HEIGHT)=')"; }
hexpix() { convert "$1" -crop 1x1+"$2"+"$3" +repage -depth 8 txt:- 2>/dev/null | grep -o '#[0-9A-F]\{6\}' | head -1; }
dist() { local a=${1#\#} b=${2#\#}; local s=0 i x y; for i in 0 2 4; do x=$((16#${a:$i:2})); y=$((16#${b:$i:2})); d=$((x > y ? x - y : y - x)); s=$((s + d)); done; echo $s; }
# lit_segments FILE: segments of the bar (16, 10 px wide, 2 px apart, middle line 28 px above the card's bottom edge) whose
# middle pixel is far from the card colour. Needs X Y WIDTH HEIGHT of the card (see read_geom).
lit_segments() {
    local f=$1 bg n=0 i px cy=$((Y + HEIGHT - 28))
    bg=$(hexpix "$f" $((X + WIDTH / 2)) $((Y + 4)))
    for i in $(seq 0 15); do
        px=$(hexpix "$f" $((X + 30 + 12 * i)) "$cy")
        [ "$(dist "$px" "$bg")" -gt 350 ] && n=$((n + 1))
    done
    echo $n
}
# show NAME ARGS...: runs machud in the background, waits for its window, screenshots it; sets HUD_PID and geometry
show() {
    local name=$1; shift
    machud "$@" >"$TMP/machud.out" 2>&1 &
    HUD_PID=$!; PIDS+=($HUD_PID)
    wait_for 3 hud_win || { fail "$name: window did not appear"; return 1; }
    sleep 0.12
    shot "$name"
    HUD_WIN=$(hud_win); read_geom "$HUD_WIN"
}
wait_exit() { wait_for 4 bash -c "! kill -0 $HUD_PID 2>/dev/null"; }

# --- 1. volume 50%: appears, centred, 10% above the bottom, half the bar lit
T0=$(now_ms)
show volume50 volume || exit 1
TAPPEAR=$(($(now_ms) - T0))
echo "      card ${WIDTH}x${HEIGHT}+$X+$Y, window found after ${TAPPEAR} ms (includes xdotool)"
eq "card size" "${WIDTH}x${HEIGHT}" "240x148"
[ $((X + WIDTH / 2 - SW / 2)) -ge -1 ] && [ $((X + WIDTH / 2 - SW / 2)) -le 1 ] && pass "horizontally centred" || fail "horizontally centred" "x=$X"
BOTTOM_GAP=$((SH - Y - HEIGHT))
[ "$BOTTOM_GAP" -ge $((SH / 10 - 2)) ] && [ "$BOTTOM_GAP" -le $((SH / 10 + 2)) ] && pass "bottom edge 10% of the screen above the bottom (gap $BOTTOM_GAP of $SH)" || fail "bottom position" "gap $BOTTOM_GAP"
BG=$(hexpix "$SHOTS/volume50.png" 5 5); CARD=$(hexpix "$SHOTS/volume50.png" $((X + WIDTH / 2)) $((Y + 4)))
[ -n "$CARD" ] && [ "$CARD" != "$BG" ] && pass "card pixels differ from the desktop ($CARD vs $BG)" || fail "card visible" "$CARD vs $BG"
eq "50% lights 8 of 16 segments" "$(lit_segments "$SHOTS/volume50.png")" 8
NCOL=$(convert "$SHOTS/volume50.png" -crop 200x20+$((X + 20))+$((Y + 90)) +repage -format '%k' info: 2>/dev/null)
[ "${NCOL:-0}" -gt 3 ] && pass "label text is drawn ($NCOL colours)" || fail "label text is drawn" "$NCOL colours"
NICON=$(convert "$SHOTS/volume50.png" -crop 60x60+$((X + 90))+$((Y + 20)) +repage -format '%k' info: 2>/dev/null)
[ "${NICON:-0}" -gt 3 ] && pass "icon is drawn ($NICON colours)" || fail "icon is drawn" "$NICON colours"

# --- 2. click-through and no focus / grab: a window underneath the card gets the click, the focus stays where it was
FAKEWIN_LOG=$TMP/clicks.log "$TARGET/${PROFILE:-release}/examples/fakewin" under "Under" 336699 >/dev/null 2>&1 &
PIDS+=($!)
wait_for 3 bash -c 'xdotool search --name "^Under$" | head -1 | grep -q .' || fail "helper window did not appear"
UNDER=$(xdotool search --name '^Under$' | head -1)
xdotool windowmove "$UNDER" 100 250
xdotool windowfocus "$UNDER" 2>/dev/null
sleep 0.1
FOCUS0=$(xdotool getwindowfocus 2>/dev/null)
# the HUD is on top of the helper window; run it again to be sure it is there and raised after the helper was mapped
setv 0.50
show clickthrough volume || exit 1
CX=$((X + WIDTH / 2)); CY=$((Y + HEIGHT / 2))
xdotool mousemove "$CX" "$CY"; sleep 0.1
OVER=$(xdotool getmouselocation --shell | awk -F= '/^WINDOW=/{print $2}')
[ "$OVER" != "$HUD_WIN" ] && pass "the pointer over the card is not 'in' the HUD window (window under pointer: $OVER)" || fail "pointer passes through the HUD"
xdotool click 1; sleep 0.15
[ -s "$TMP/clicks.log" ] && pass "a click on the card reached the window underneath" || fail "click reached the window underneath" "log: $(cat "$TMP/clicks.log" 2>/dev/null)"
hud_win >/dev/null && pass "a click does not dismiss or grab (card still up)" || fail "card vanished on click"
eq "keyboard focus unchanged" "$(xdotool getwindowfocus 2>/dev/null)" "$FOCUS0"
xdotool mousemove 2 2
wait_exit; ok=$?
xdotool windowclose "$UNDER" 2>/dev/null

# --- 3. disappears after the timeout (1.4 s), and the process is gone (zero idle cost)
rm -f "$XDG_RUNTIME_DIR"/machud.*
T0=$(now_ms)
machud volume >/dev/null 2>&1 &
HUD_PID=$!; PIDS+=($HUD_PID)
wait_for 3 hud_win || fail "window did not appear (timeout test)"
TUP=$(now_ms)
wait_for 4 hud_gone
TDOWN=$(($(now_ms) - T0))
echo "      visible from ~$((TUP - T0)) ms to $TDOWN ms"
[ "$TDOWN" -ge 1300 ] && [ "$TDOWN" -le 2000 ] && pass "disappears after about 1.4 s ($TDOWN ms)" || fail "disappearance time" "$TDOWN ms"
wait_exit && pass "the process exits with the window" || fail "process still running"
[ -s "$XDG_RUNTIME_DIR/machud.msg" ] && fail "mailbox left behind" || pass "no stale mailbox"

# --- 4. a second invocation updates the same window and extends the timer; no second window
setv 0.50
T0=$(now_ms)
machud volume >/dev/null 2>&1 &
HUD_PID=$!; PIDS+=($HUD_PID)
wait_for 3 hud_win || fail "window did not appear (extend test)"
W1=$(hud_win)
while [ $(($(now_ms) - T0)) -lt 900 ]; do sleep 0.02; done
setv 0.25
T1=$(now_ms)
machud volume >/dev/null 2>&1
T2=$(($(now_ms) - T1))
[ "$T2" -lt 300 ] && pass "the second invocation returns at once (${T2} ms)" || fail "second invocation is quick" "${T2} ms"
sleep 0.12
eq "still exactly one HUD window" "$(n_wins)" 1
eq "same window" "$(hud_win)" "$W1"
shot extended
read_geom "$W1"
eq "the bar now shows 25% (4 of 16)" "$(lit_segments "$SHOTS/extended.png")" 4
while [ $(($(now_ms) - T0)) -lt 1700 ]; do sleep 0.02; done
hud_win >/dev/null && pass "still up at 1.7 s: past the first invocation's timeout" || fail "timer was not extended"
wait_for 4 hud_gone
TDOWN=$(($(now_ms) - T0))
[ "$TDOWN" -ge 2200 ] && [ "$TDOWN" -le 3100 ] && pass "gone 1.4 s after the second call (${TDOWN} ms from the first)" || fail "extended disappearance time" "${TDOWN} ms"
wait_exit
eq "no HUD window left" "$(n_wins)" 0
CPU=$(awk '{print $14 + $15}' "/proc/$HUD_PID/stat" 2>/dev/null)
[ -z "$CPU" ] && pass "(process gone; idle CPU not sampled)" || echo "      cpu ticks $CPU"

# --- 5. idle cost while visible: sample the running process
setv 0.50
machud volume >/dev/null 2>&1 &
HUD_PID=$!; PIDS+=($HUD_PID)
wait_for 3 hud_win; sleep 0.9
CPU=$(awk '{print $14 + $15}' "/proc/$HUD_PID/stat" 2>/dev/null)
RSS=$(awk '/VmRSS/{print $2}' "/proc/$HUD_PID/status" 2>/dev/null)
echo "      after 0.9 s: cpu ${CPU:-?} ticks, rss ${RSS:-?} kB"
[ -n "$CPU" ] && [ "$CPU" -le 8 ] && pass "cheap while visible (cpu ticks $CPU <= 8)" || fail "cpu while visible" "${CPU:-gone}"
wait_exit

# --- 6. muted: crossed speaker, empty bar, label differs
setv 0.60 1
show muted volume || exit 1
eq "muted shows an empty bar" "$(lit_segments "$SHOTS/muted.png")" 0
IC_M=$(convert "$SHOTS/muted.png" -crop 60x60+$((X + 90))+$((Y + 20)) +repage -depth 8 -format '%#' info: 2>/dev/null)
wait_exit
setv 0.60 0
show unmuted volume || exit 1
eq "unmuted 60% shows 10 segments" "$(lit_segments "$SHOTS/unmuted.png")" 10
IC_U=$(convert "$SHOTS/unmuted.png" -crop 60x60+$((X + 90))+$((Y + 20)) +repage -depth 8 -format '%#' info: 2>/dev/null)
[ "$IC_M" != "$IC_U" ] && pass "muted and unmuted icons differ" || fail "icons differ"
wait_exit

# --- 7. brightness and mic
show brightness brightness || exit 1
eq "brightness 70% lights 11 segments" "$(lit_segments "$SHOTS/brightness.png")" 11
wait_exit
setb 100
show brightness100 brightness || exit 1
eq "brightness 100% lights all 16" "$(lit_segments "$SHOTS/brightness100.png")" 16
wait_exit
show mic mic || exit 1
eq "mic 25% lights 4 segments" "$(lit_segments "$SHOTS/mic.png")" 4
wait_exit
setmic 0.25 1
show micmuted mic || exit 1
eq "muted mic shows an empty bar" "$(lit_segments "$SHOTS/micmuted.png")" 0
wait_exit
setmic 0.25 0

# --- 8. unreadable values: nothing is shown and the exit status is 0
export SHIM_FAIL=1
T0=$(now_ms)
for k in volume mic brightness; do
    machud $k >/dev/null 2>&1; rc=$?
    eq "machud $k with nothing readable exits 0" "$rc" 0
done
sleep 0.2
eq "and shows no window" "$(n_wins)" 0
[ $(($(now_ms) - T0)) -lt 1500 ] && pass "gives up quickly" || fail "gives up quickly"
# sysfs fallback for brightness when brightnessctl fails
mkdir -p "$TMP/bl/intel_backlight"; echo 30 > "$TMP/bl/intel_backlight/brightness"; echo 100 > "$TMP/bl/intel_backlight/max_brightness"
export MACHUD_BACKLIGHT_DIR=$TMP/bl
show sysfs brightness || exit 1
eq "brightness from /sys/class/backlight: 30% lights 5 segments" "$(lit_segments "$SHOTS/sysfs.png")" 5
wait_exit
unset SHIM_FAIL
export MACHUD_BACKLIGHT_DIR=$TMP/nobacklight
machud nonsense >/dev/null 2>&1; eq "an unknown argument exits 2" "$?" 2
machud --version | grep -q '^machud ' && pass "--version" || fail "--version"

# --- 9. start-up: from exec to a painted card (the card is only a few ms of work; xdotool polling dominates this figure)
setv 0.50
T0=$(now_ms)
machud volume >/dev/null 2>&1 &
HUD_PID=$!; PIDS+=($HUD_PID)
wait_for 3 hud_win; echo "      first window seen after $(($(now_ms) - T0)) ms"
sleep 0.1; shot startup
read_geom "$(hud_win)"
eq "painted by 100 ms after it appeared" "$(lit_segments "$SHOTS/startup.png")" 8
wait_exit

# --- 10. light theme follows the shared theme
echo '{"theme":{"name":"macos","mode":"light"}}' > "$MACSHELL_CONFIG/macshell.json"
show light volume || exit 1
LBG=$(hexpix "$SHOTS/light.png" $((X + WIDTH / 2)) $((Y + 4)))
DBG=$(hexpix "$SHOTS/volume50.png" $((X + WIDTH / 2)) $((Y + 4)))
echo "      card colour dark $DBG, light $LBG"
[ "$(dist "$LBG" "$DBG")" -gt 150 ] && pass "the light theme gives a different (lighter) card" || fail "light theme card colour"
eq "light theme still shows 8 segments" "$(lit_segments "$SHOTS/light.png")" 8
wait_exit
rm -f "$MACSHELL_CONFIG/macshell.json"

# --- 11. with a compositor (picom) the card is a real ARGB window: rounded corners are see-through
if command -v picom >/dev/null; then
    picom --config /dev/null --backend xrender >"$TMP/picom.log" 2>&1 &
    PICOM=$!; PIDS+=($PICOM)
    sleep 1.5
    if kill -0 $PICOM 2>/dev/null; then
        show picom volume || exit 1
        sleep 0.3; shot picom; read_geom "$HUD_WIN"
        CORNER=$(hexpix "$SHOTS/picom.png" "$X" "$Y"); OUTSIDE=$(hexpix "$SHOTS/picom.png" $((X - 30)) $((Y + 5)))
        CARDP=$(hexpix "$SHOTS/picom.png" $((X + WIDTH / 2)) $((Y + 4)))
        echo "      picom: corner=$CORNER outside=$OUTSIDE card=$CARDP"
        [ "$CORNER" = "$OUTSIDE" ] && pass "rounded corner is see-through (real alpha)" || fail "rounded corner is see-through" "$CORNER vs $OUTSIDE"
        [ -n "$CARDP" ] && [ "$CARDP" != "$OUTSIDE" ] && pass "card visible under a compositor" || fail "card visible under a compositor"
        eq "bar fill under a compositor" "$(lit_segments "$SHOTS/picom.png")" 8
        wait_exit
    else
        echo "  skip compositor phase (picom did not start: $(head -c 200 "$TMP/picom.log"))"
    fi
fi

echo; echo "passed $PASS, failed $FAIL (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
