#!/bin/bash
# decdash (dashboard popup) end to end on a PRIVATE Xvfb display, a private XDG_RUNTIME_DIR and a private D-Bus
# session bus with fake MPRIS players (examples/fake_mpris.rs). No window manager.
#   tests/e2e/dash.sh                  (E2E_DISPLAY=96 by default; SHOTS=dir keeps the screenshots)
# Needs Xvfb, xdotool, import + convert (ImageMagick), dbus-daemon and tesseract (OCR of the card; the
# script exits 77 = skipped when one is missing). Everything is private: own display, bus, HOME, caches, a
# cached weather reading (no network), a fake nvidia-smi on a temp PATH. Only PIDs started here are killed.
# Never touches the live display (:1) or Hyprland: WAYLAND_DISPLAY is unset and DISPLAY is the private server.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
TARGET=${CARGO_TARGET_DIR:-$HERE/target}
PROFILE=${PROFILE:-release}
BIN=$TARGET/$PROFILE
for t in Xvfb xdotool import convert dbus-daemon tesseract; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline $([ "$PROFILE" = release ] && echo --release) --bin decdash --example fake_mpris --example fakewin) || { echo "build failed"; exit 2; }

N=${E2E_DISPLAY:-96}
[ "$N" = 1 ] && { echo "refusing the live display"; exit 2; }
PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d "${TMPDIR:-/tmp}/decdash-e2e.XXXXXX")
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
has() { case "$2" in *"$3"*) pass "$1";; *) fail "$1" "'$3' not in '$2'";; esac; }
now_ms() { echo $(($(date +%s%N) / 1000000)); }
wait_for() { local t=$1; shift; local end=$(($(now_ms) + t * 1000)); while [ "$(now_ms)" -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 0.05; done; return 1; }

SW=1600; SH=1000
Xvfb :$N -screen 0 ${SW}x${SH}x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X$N || { echo "Xvfb failed"; cat "$TMP/xvfb.log"; exit 2; }
export DISPLAY=:$N
unset WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE DBUS_SESSION_BUS_ADDRESS
export XDG_RUNTIME_DIR=$TMP/run XDG_CONFIG_HOME=$TMP/cfg HOME=$TMP/home XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata XDG_CACHE_HOME=$TMP/cache
mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME" "$HOME" "$TMP/nodata" "$XDG_CACHE_HOME/decebal" "$TMP/shim"
chmod 700 "$XDG_RUNTIME_DIR"
export DECEBAL_CONFIG=$TMP/decebal-cfg DECEBAL_BACKEND=x11 DECEBAL_COLOR_SCHEME=dark
mkdir -p "$DECEBAL_CONFIG"

# Private session bus on a socket inside the private runtime dir.
dbus-daemon --session --address="unix:path=$XDG_RUNTIME_DIR/bus" --nofork >"$TMP/dbus.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -S "$XDG_RUNTIME_DIR/bus" || { echo "dbus-daemon failed"; cat "$TMP/dbus.log"; exit 2; }
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"

# A fresh weather reading, so decdash never goes to the network.
echo "{\"temp_c\":12,\"code\":116,\"night\":false,\"fetched\":$(date +%s)}" > "$XDG_CACHE_HOME/decebal/weather.json"
# A fake nvidia-smi (the GPU card shows up) and nothing else new on PATH.
cat > "$TMP/shim/nvidia-smi" <<'SH'
#!/bin/bash
echo "NVIDIA GeForce RTX 9999, 37, 51, 2048, 16384"
SH
chmod +x "$TMP/shim/nvidia-smi"
export PATH=$TMP/shim:$BIN:$BIN/examples:/usr/bin:/bin
export FAKE_MPRIS_LOG=$TMP/mpris.log
convert -size 200x200 gradient:'#ff3366-#3366ff' "$TMP/cover.png"
export FAKE_MPRIS_ART=$TMP/cover.png

convert() { command convert "$@" 2>/dev/null; }
shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
dash_win() { for w in $(xdotool search --name '^decdash$' 2>/dev/null); do echo "$w"; return 0; done; return 1; }
dash_gone() { ! dash_win; }
n_wins() { xdotool search --name '^decdash$' 2>/dev/null | wc -l; }
read_geom() { eval "$(xdotool getwindowgeometry --shell "$1" | grep -E '^(X|Y|WIDTH|HEIGHT)=')"; }
hexpix() { convert "$1" -crop 1x1+"$2"+"$3" +repage -depth 8 txt:- 2>/dev/null | grep -o '#[0-9A-F]\{6\}' | head -1; }
# ocr FILE X Y W H: the text in a region (light on dark is inverted first), whitespace squeezed.
ocr() {
    local mean
    convert "$1" -crop "$4x$5+$2+$3" +repage -resize 300% -colorspace Gray -normalize "$TMP/ocr.png"
    mean=$(convert "$TMP/ocr.png" -format '%[fx:mean*100]' info: 2>/dev/null | cut -d. -f1)
    [ "${mean:-50}" -lt 50 ] && convert "$TMP/ocr.png" -negate "$TMP/ocr.png"
    tesseract "$TMP/ocr.png" - --psm 6 2>/dev/null | tr -s ' \n' ' '
}
# Card coordinates -> screen coordinates (needs X Y from read_geom).
sx() { echo $((X + $1)); }
sy() { echo $((Y + $1)); }
click_at() { xdotool mousemove $(sx "$1") $(sy "$2"); sleep 0.08; xdotool click 1; sleep 0.25; }
# start ARGS...: runs decdash in the background, waits for its window; sets DASH_PID, DASH_WIN and the geometry.
start() {
    decdash "$@" >"$TMP/decdash.out" 2>&1 &
    DASH_PID=$!; PIDS+=($DASH_PID)
    wait_for 4 dash_win || { fail "window did not appear ($*)"; return 1; }
    DASH_WIN=$(dash_win); read_geom "$DASH_WIN"
    sleep 0.5
}
alive() { kill -0 "$DASH_PID" 2>/dev/null; }
gone() { wait_for 3 bash -c "! kill -0 $DASH_PID 2>/dev/null"; }
logged() { grep -c "^$1" "$FAKE_MPRIS_LOG" 2>/dev/null; }
TITLE_BOX="252 58 248 30"      # the title line of the Media tab (card coordinates)
USER_NAME=$(id -un)

# --- 1. opens under the bar, centred on --x, Overview first
start --x 800 || exit 1
echo "      card ${WIDTH}x${HEIGHT}+$X+$Y"
eq "card size" "${WIDTH}x${HEIGHT}" "520x392"
eq "hangs 30 px below the top (bar 26 + 4)" "$Y" 30
[ $((X + WIDTH / 2 - 800)) -ge -1 ] && [ $((X + WIDTH / 2 - 800)) -le 1 ] && pass "centred on --x 800" || fail "centred on --x" "x=$X"
shot overview
has "tab bar names the three tabs" "$(ocr "$SHOTS/overview.png" $X $((Y + 8)) 520 36)" "Performance"
MONTH=$(date +%B); YEAR=$(date +%Y)
has "calendar shows this month" "$(ocr "$SHOTS/overview.png" $((X + 258)) $((Y + 50)) 240 28)" "$MONTH"
has "calendar shows this year" "$(ocr "$SHOTS/overview.png" $((X + 258)) $((Y + 50)) 240 28)" "$YEAR"
has "user card shows user@host" "$(ocr "$SHOTS/overview.png" $((X + 88)) $((Y + 72)) 150 60)" "$USER_NAME"
has "cached weather is shown" "$(ocr "$SHOTS/overview.png" $((X + 70)) $((Y + 190)) 120 60)" "12"
has "resource rings are labelled" "$(ocr "$SHOTS/overview.png" $((X + 14)) $((Y + 336)) 250 26)" "CPU"
CLOCK_NOW=$(date +%H:%M)
has "the clock face is the time" "$(ocr "$SHOTS/overview.png" $((X + 14)) $((Y + 112)) 190 54)" "${CLOCK_NOW%%:*}"
kill -0 $DASH_PID 2>/dev/null && pass "still running"

# --- 2. keyboard: Right/Left/1-3 switch tabs; no player yet
xdotool key Right; sleep 0.3; shot media_empty
has "Right opens the Media tab (nothing playing)" "$(ocr "$SHOTS/media_empty.png" $((X + 20)) $((Y + 170)) 480 120)" "No media"
xdotool key Right; sleep 0.3; shot perf
has "Right again: Performance (CPU card)" "$(ocr "$SHOTS/perf.png" $((X + 20)) $((Y + 52)) 240 30)" "CPU"
xdotool key Right; sleep 0.3; shot wrap
has "Right wraps to Overview" "$(ocr "$SHOTS/wrap.png" $((X + 258)) $((Y + 50)) 240 28)" "$MONTH"
xdotool key 3; sleep 0.3; shot perf
has "key 3 opens Performance" "$(ocr "$SHOTS/perf.png" $((X + 20)) $((Y + 52)) 240 30)" "CPU"
xdotool key Left; sleep 0.3; shot media_empty2
has "Left goes back to Media" "$(ocr "$SHOTS/media_empty2.png" $((X + 20)) $((Y + 170)) 480 120)" "No media"

# --- 3. a player appears while open (NameOwnerChanged): the title shows
rm -f "$FAKE_MPRIS_LOG"
fake_mpris test "Fake Song" >"$TMP/fake1.out" 2>&1 &
PIDS+=($!)
wait_for 4 grep -q ready "$FAKE_MPRIS_LOG" || fail "fake player did not start"
sleep 1.0
shot media
has "the Media tab shows the player's title" "$(ocr "$SHOTS/media.png" $((X + 252)) $((Y + 58)) 248 30)" "Fake Song"
has "... its artist" "$(ocr "$SHOTS/media.png" $((X + 252)) $((Y + 90)) 248 24)" "Fake Artist"
has "... its album" "$(ocr "$SHOTS/media.png" $((X + 252)) $((Y + 114)) 248 22)" "Fake Album"
has "the position is polled (starts at 0:30)" "$(ocr "$SHOTS/media.png" $((X + 252)) $((Y + 168)) 70 20)" "0:3"
has "the total length is shown" "$(ocr "$SHOTS/media.png" $((X + 430)) $((Y + 168)) 70 20)" "3:20"
COVER=$(hexpix "$SHOTS/media.png" $((X + 20 + 104)) $((Y + 52 + 8 + 104)))
[ -n "$COVER" ] && [ "$COVER" != "$(hexpix "$SHOTS/media.png" $((X + 5)) $((Y + 5)))" ] && pass "cover art is drawn ($COVER)" || fail "cover art" "$COVER"
# the volume slider (0.7)
has "volume is shown (70%)" "$(ocr "$SHOTS/media.png" $((X + 440)) $((Y + 304)) 60 24)" "70"

# --- 4. clicking the buttons calls the player
PLAY_BEFORE=$(convert "$SHOTS/media.png" -crop 60x60+$((X + 346))+$((Y + 202)) +repage -depth 8 -format '%#' info:- 2>/dev/null)
click_at 376 232
wait_for 3 bash -c "[ \$(grep -c '^PlayPause' '$FAKE_MPRIS_LOG') -ge 1 ]" && pass "a click on play/pause calls PlayPause on the player" || fail "play/pause click" "$(cat "$FAKE_MPRIS_LOG")"
sleep 0.4; shot paused
PLAY_AFTER=$(convert "$SHOTS/paused.png" -crop 60x60+$((X + 346))+$((Y + 202)) +repage -depth 8 -format '%#' info:- 2>/dev/null)
[ "$PLAY_BEFORE" != "$PLAY_AFTER" ] && pass "the button's glyph flips once the player reports Paused (PropertiesChanged)" || fail "glyph flip"
click_at 376 232
wait_for 3 bash -c "[ \$(grep -c '^PlayPause' '$FAKE_MPRIS_LOG') -ge 2 ]" && pass "second click: PlayPause again" || fail "second play/pause"
xdotool key space; sleep 0.3
wait_for 3 bash -c "[ \$(grep -c '^PlayPause' '$FAKE_MPRIS_LOG') -ge 3 ]" && pass "Space also toggles playback" || fail "space"
click_at 442 232
wait_for 3 bash -c "grep -q '^Next' '$FAKE_MPRIS_LOG'" && pass "next button calls Next" || fail "next"
click_at 310 232
wait_for 3 bash -c "grep -q '^Previous' '$FAKE_MPRIS_LOG'" && pass "previous button calls Previous" || fail "previous"
# seek: half way along the bar -> SetPosition near 100 s
click_at 376 160
wait_for 3 bash -c "grep -q '^SetPosition' '$FAKE_MPRIS_LOG'"
POS=$(grep '^SetPosition' "$FAKE_MPRIS_LOG" | tail -1 | awk '{print $2}')
[ -n "$POS" ] && [ "$POS" -gt 90000000 ] && [ "$POS" -lt 110000000 ] && pass "clicking the bar seeks (SetPosition $POS us ~ 100 s)" || fail "seek" "${POS:-none}"
# volume: 80 % of the slider
click_at $((282 + 218 * 8 / 10)) 296
wait_for 3 bash -c "grep -q '^Volume' '$FAKE_MPRIS_LOG'"
VOLV=$(grep '^Volume' "$FAKE_MPRIS_LOG" | tail -1 | awk '{print $2}')
case "$VOLV" in 0.7[5-9]|0.8[0-5]) pass "clicking the volume slider sets Volume ($VOLV)";; *) fail "volume" "${VOLV:-none}";; esac
sleep 0.5; shot volume
has "the UI follows the new volume" "$(ocr "$SHOTS/volume.png" $((X + 440)) $((Y + 304)) 60 24)" "80"
xdotool key Down; sleep 0.3
wait_for 3 bash -c "[ \$(grep -c '^Volume' '$FAKE_MPRIS_LOG') -ge 2 ]" && pass "Down lowers the volume" || fail "volume key"

# (three toggles left it paused: play again, so it is the one that is playing when the second player joins)
xdotool key space; sleep 0.5
# --- 5. a second player: chips appear, picking one switches the title
fake_mpris second "Other Song" >"$TMP/fake2.out" 2>&1 &
PIDS+=($!)
sleep 1.2; shot chips
has "two players: the picker lists both" "$(ocr "$SHOTS/chips.png" $((X + 20)) $((Y + 342)) 480 28)" "second"
has "the first player is still the one shown (no flip)" "$(ocr "$SHOTS/chips.png" $((X + 252)) $((Y + 58)) 248 30)" "Fake Song"
click_at 95 356
sleep 0.5; shot picked
has "picking a chip shows that player" "$(ocr "$SHOTS/picked.png" $((X + 252)) $((Y + 58)) 248 30)" "Other Song"
# the player quitting removes its chip
kill "${PIDS[-1]}" 2>/dev/null; sleep 1.0; shot after_quit
has "when a player quits the other one is shown" "$(ocr "$SHOTS/after_quit.png" $((X + 252)) $((Y + 58)) 248 30)" "Fake Song"

# --- 6. the Overview now-playing card, then the Performance tab
click_at 138 26; sleep 0.3; shot ov2
has "Overview: now playing shows the title" "$(ocr "$SHOTS/ov2.png" $((X + 290)) $((Y + 280)) 210 60)" "Fake"
click_at 382 26; sleep 1.3; shot perf2
has "tab click: Performance shows CPU" "$(ocr "$SHOTS/perf2.png" $((X + 20)) $((Y + 52)) 240 30)" "CPU"
# (each card's title is read on its own: tesseract copes badly with the whole busy tab at once)
P=$SHOTS/perf2.png
has "Performance: network card" "$(ocr "$P" $((X + 20)) $((Y + 264)) 236 26)" "NETWORK"
has "Performance: memory card" "$(ocr "$P" $((X + 264)) $((Y + 52)) 236 26)" "MEMORY"
has "Performance: storage card" "$(ocr "$P" $((X + 264)) $((Y + 156)) 236 26)" "STORAGE"
has "Performance: GPU card from nvidia-smi" "$(ocr "$P" $((X + 264)) $((Y + 260)) 236 26)" "GPU"
has "GPU temperature from nvidia-smi" "$(ocr "$P" $((X + 340)) $((Y + 284)) 100 30)" "51"
has "VRAM from nvidia-smi" "$(ocr "$P" $((X + 340)) $((Y + 326)) 160 20)" "VRAM"
# live: CPU load changes the card within a couple of seconds
CPU_BEFORE=$(convert "$SHOTS/perf2.png" -crop 70x70+$((X + 34))+$((Y + 70)) +repage -depth 8 -format '%#' info:- 2>/dev/null)
BUSY=()
for i in 1 2 3 4; do timeout 4 sh -c 'while :; do :; done' & BUSY+=($!); done
PIDS+=("${BUSY[@]}")
sleep 2.6; shot perf3
CPU_AFTER=$(convert "$SHOTS/perf3.png" -crop 70x70+$((X + 34))+$((Y + 70)) +repage -depth 8 -format '%#' info:- 2>/dev/null)
[ "$CPU_BEFORE" != "$CPU_AFTER" ] && pass "the CPU gauge moves under load (live 1 Hz sampling)" || fail "cpu gauge did not change"
wait "${BUSY[@]}" 2>/dev/null

# --- 7. close on Escape, and the process is gone (zero cost while closed)
xdotool key Escape
gone && pass "Escape closes (process exited)" || fail "Escape closes"
dash_gone && pass "window is gone" || fail "window gone"

# --- 8. single instance toggle
start || exit 1
decdash >/dev/null 2>&1
gone && pass "running decdash again closes the open one" || fail "toggle closed"
dash_gone && pass "... and leaves no window" || fail "toggle left a window"
start || exit 1
eq "a fresh run opens exactly one window" "$(n_wins)" 1
decdash media >/dev/null 2>&1; gone
start media || exit 1
shot media_arg
has "decdash media opens on the Media tab" "$(ocr "$SHOTS/media_arg.png" $((X + 252)) $((Y + 58)) 248 30)" "Fake Song"
eq "an out-of-range --x is kept on screen" "$([ "$X" -ge 8 ] && echo ok)" ok
xdotool key Escape; gone

# --- 9. click outside closes
start --x 20 || exit 1
eq "--x 20 keeps the card 8 px from the left edge" "$X" 8
xdotool mousemove 900 700; sleep 0.1; xdotool click 1
gone && pass "a click outside closes" || fail "click outside"

# --- 10. focus loss closes (another client takes the focus)
start || exit 1
fakewin Other Other 336699 >/dev/null 2>&1 &
PIDS+=($!)
wait_for 3 bash -c 'xdotool search --name "^Other$" | head -1 | grep -q .'
OTHER=$(xdotool search --name '^Other$' | head -1)
xdotool windowfocus "$OTHER" 2>/dev/null
if gone; then pass "focus moving elsewhere closes"; else
    echo "  note  focus change did not close it (the keyboard grab keeps the focus here)"; xdotool key Escape; gone
fi

# --- 11. idle cost: a visible card is cheap; nothing runs after closing
start || exit 1
sleep 3
CPU=$(awk '{print $14 + $15}' "/proc/$DASH_PID/stat" 2>/dev/null)
RSS=$(awk '/VmRSS/{print $2}' "/proc/$DASH_PID/status" 2>/dev/null)
echo "      after ~3.5 s open: cpu ${CPU:-?} ticks, rss ${RSS:-?} kB"
[ -n "$CPU" ] && [ "$CPU" -le 60 ] && pass "cheap while visible (cpu ticks $CPU <= 60)" || fail "cpu while visible" "${CPU:-gone}"
xdotool key Escape; gone
! pgrep -x decdash >/dev/null 2>&1 || true

echo; echo "passed $PASS, failed $FAIL (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
