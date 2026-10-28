#!/bin/bash
# declock (screen locker) end to end on a private Xvfb, no window manager.
#   tests/e2e/lock.sh                 (E2E_DISPLAY=97 by default; SHOTS=dir keeps the screenshots)
# Two binaries are used:
#   * the SHIPPED declock ($MS_BIN_DIR, default target/release): real PAM. Exactly ONE wrong password is typed to it
#     (nothing can unlock it by accident, and the real password is never typed); it also proves the shipped binary
#     carries no test bypass.
#   * a TEST-AUTH build (cargo feature `test-auth`, separate target dir, fake authenticator, password hunter2) for
#     everything that needs a successful unlock or several failures (back-off, freeze, Escape, dots, ...).
# Everything is private: own Xvfb, own XDG_RUNTIME_DIR / HOME / config. Never touches the live display: WAYLAND_DISPLAY
# is unset and DISPLAY points at the private server. Only PIDs started here are killed.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
TARGET=${CARGO_TARGET_DIR:-$HERE/target}
BIN=${MS_BIN_DIR:-$TARGET/${PROFILE:-release}}
TESTTARGET=${DECLOCK_TEST_TARGET:-$TARGET/test-auth}
for t in Xvfb xdotool import convert compare display awk; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
if [ -z "${MS_BIN_DIR:-}" ]; then
    (cd "$HERE" && cargo build -q --offline $([ "${PROFILE:-release}" = release ] && echo --release) --bin declock --example fakewin) || { echo "build failed"; exit 2; }
fi
if [ -z "${DECLOCK_TEST_BIN:-}" ]; then
    # fast to build: no fat LTO for this throw-away binary
    (cd "$HERE" && CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo build -q --offline --release --features test-auth --bin declock --target-dir "$TESTTARGET") || { echo "test-auth build failed"; exit 2; }
fi
TESTBIN=${DECLOCK_TEST_BIN:-$TESTTARGET/release/declock}
FAKEWIN=${FAKEWIN:-$TARGET/${PROFILE:-release}/examples/fakewin}

N=${E2E_DISPLAY:-97}
[ "$N" = 0 ] || [ "$N" = 1 ] && { echo "refusing the live display"; exit 2; }
PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d "${TMPDIR:-/tmp}/declock-e2e.XXXXXX")
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

command -v magick >/dev/null && convert() { magick "$@"; }  # ImageMagick 7 deprecates `convert`
lt() { awk -v a="$1" -v b="$2" 'BEGIN{exit !(a<b)}'; }
SW=1280; SH=800
Xvfb :$N -screen 0 ${SW}x${SH}x24 +extension RANDR -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X$N || { echo "Xvfb failed"; cat "$TMP/xvfb.log"; exit 2; }
export DISPLAY=:$N
unset WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE XSS_SLEEP_LOCK_FD
export XDG_RUNTIME_DIR=$TMP/run XDG_CONFIG_HOME=$TMP/cfg HOME=$TMP/home XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata XDG_CACHE_HOME=$TMP/cache XDG_STATE_HOME=$TMP/state
mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME" "$HOME" "$TMP/nodata" "$XDG_CACHE_HOME" "$XDG_STATE_HOME"
chmod 700 "$XDG_RUNTIME_DIR"
export DECEBAL_CONFIG=$TMP/decebal-cfg DECEBAL_BACKEND=x11 DECEBAL_COLOR_SCHEME=dark
mkdir -p "$DECEBAL_CONFIG"
echo '{"theme":{"name":"macos","mode":"dark","accent":"#ff2200"}}' > "$DECEBAL_CONFIG/decebal.json"
# a colourful "desktop" so the blurred backdrop is visibly a copy of it
convert -size ${SW}x${SH} plasma:fractal "$TMP/desk.png" 2>/dev/null || convert -size ${SW}x${SH} gradient:red-blue "$TMP/desk.png"
desk() { display -window root "$TMP/desk.png" 2>/dev/null; }
desk; sleep 0.3
export PATH=$BIN:/usr/bin:/bin

shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
lock_win() { for w in $(xdotool search --name '^declock$' 2>/dev/null); do echo "$w"; return 0; done; return 1; }
lock_gone() { ! lock_win; }
alive() { kill -0 "$1" 2>/dev/null; }
rss() { awk '/VmRSS/{print $2}' "/proc/$1/status" 2>/dev/null; }
ticks() { awk '{print $14 + $15}' "/proc/$1/stat" 2>/dev/null; }
hexpix() { convert "$1" -crop 1x1+"$2"+"$3" +repage -depth 8 txt:- 2>/dev/null | grep -o '#[0-9A-F]\{6\}' | head -1; }
dist() { local a=${1#\#} b=${2#\#}; local s=0 i x y; for i in 0 2 4; do x=$((16#${a:$i:2})); y=$((16#${b:$i:2})); d=$((x > y ? x - y : y - x)); s=$((s + d)); done; echo $s; }
# panel geometry for SH=800: scale 0.75 -> panel 420x405 at (430,178); field 255x40 centred at y=178+0.75*425
FX=$((640 - 128)); FY=$((178 + 298)); FW=256; FH=41  # field_diff compares the inside only (the border changes colour with the state)
region_diff() { compare -fuzz 8% -metric AE "$1" "$2" -crop "${5}x${6}+${3}+${4}" null: 2>&1 | grep -o '^[0-9]*' | head -1; }
field_diff() { region_diff "$SHOTS/$1.png" "$SHOTS/$2.png" $((FX + 8)) $((FY + 8)) $((FW - 16)) $((FH - 16)); }
status_diff() { region_diff "$SHOTS/$1.png" "$SHOTS/$2.png" 430 $((FY + FH)) 420 40; }
type_str() { xdotool type --delay 15 -- "$1"; }
start() { # start BIN NAME [env...]: runs declock in the background; sets LOCK_PID
    local b=$1 name=$2; shift 2
    desk; sleep 0.15
    env "$@" "$b" >"$TMP/$name.out" 2>&1 &
    LOCK_PID=$!; PIDS+=($LOCK_PID)
    wait_for 5 lock_win
}
settle() { sleep ${1:-0.25}; }

echo "== shipped binary"
"$BIN/declock" --version | grep -q '^declock ' && pass "--version" || fail "--version"
"$BIN/declock" --help | grep -q 'Usage: declock' && pass "--help" || fail "--help"
"$BIN/declock" --bogus >/dev/null 2>&1; eq "unknown option exits 2" "$?" 2
"$BIN/declock" --dim 7 >/dev/null 2>&1; eq "bad --dim exits 2" "$?" 2
"$BIN/declock" --bg paint >/dev/null 2>&1; eq "bad --bg exits 2" "$?" 2
if strings -a "$BIN/declock" | grep -q 'DECLOCK_FAKE_AUTH\|DECLOCK_TEST_PASSWORD'; then fail "the shipped binary contains the test authenticator"; else pass "the shipped binary has no fake-auth bypass"; fi
strings -a "$BIN/declock" | grep -q pam_authenticate && pass "the shipped binary calls pam_authenticate" || fail "pam_authenticate not linked"

echo "== real PAM: a wrong password must not unlock"
convert "$TMP/desk.png" -crop 10x10+5+5 +repage -depth 8 -format '%#' info: > "$TMP/desk.sig" 2>/dev/null
T0=$(now_ms)
start "$BIN/declock" real || { fail "lock window did not appear"; cat "$TMP/real.out"; exit 1; }
PAM_PID=$LOCK_PID
TUP=$(($(now_ms) - T0)); echo "      window after ${TUP} ms"
W=$(lock_win)
eval "$(xdotool getwindowgeometry --shell "$W" | grep -E '^(X|Y|WIDTH|HEIGHT)=')"
eq "covers the whole screen" "${X},${Y},${WIDTH}x${HEIGHT}" "0,0,${SW}x${SH}"
settle 0.6; shot idle
# the backdrop is a blurred, darkened copy of the desktop, not the desktop itself and not flat
[ "$(convert "$SHOTS/idle.png" -crop 200x200+20+20 +repage -format '%k' info:)" -gt 20 ] && pass "backdrop keeps the desktop's colours (blurred copy)" || fail "backdrop looks flat"
D1=$(convert "$TMP/desk.png" -crop 200x200+20+20 +repage -colorspace gray -format '%[fx:mean]' info:); D2=$(convert "$SHOTS/idle.png" -crop 200x200+20+20 +repage -colorspace gray -format '%[fx:mean]' info:)
lt "$D2" "$(awk -v d="$D1" 'BEGIN{print d*0.8}')" && pass "and it is dimmed ($D2 vs $D1)" || fail "backdrop is not dimmed" "$D2 vs $D1"
SHARP=$(convert "$SHOTS/idle.png" -crop 300x200+20+20 +repage -colorspace gray -define convolve:scale='!' -morphology Convolve Laplacian:0 -format '%[fx:standard_deviation]' info: 2>/dev/null)
SHARP0=$(convert "$TMP/desk.png" -crop 300x200+20+20 +repage -colorspace gray -define convolve:scale='!' -morphology Convolve Laplacian:0 -format '%[fx:standard_deviation]' info: 2>/dev/null)
lt "$SHARP" "$(awk -v d="$SHARP0" 'BEGIN{print d*0.3}')" && pass "and blurred (edge energy $SHARP vs $SHARP0)" || fail "backdrop is not blurred" "$SHARP vs $SHARP0"
# the panel: clock, avatar, name, field are drawn in the middle
NCOL=$(convert "$SHOTS/idle.png" -crop 420x120+430+178 +repage -format '%k' info:)
[ "$NCOL" -gt 30 ] && pass "clock drawn ($NCOL colours)" || fail "clock drawn" "$NCOL"
AV=$(hexpix "$SHOTS/idle.png" $((640 - 24)) $((397 + 20)))
[ "$(dist "$AV" '#FF2200')" -lt 30 ] && pass "avatar disc uses the theme accent ($AV)" || fail "avatar uses the accent" "$AV"
# idle cost
sleep 2.5
R=$(rss "$PAM_PID"); C0=$(ticks "$PAM_PID"); sleep 3; C1=$(ticks "$PAM_PID")
echo "      idle: RSS ${R} kB, cpu ticks over 3 s: $((C1 - C0)) (CLK_TCK=$(getconf CLK_TCK))"
[ "${R:-999999}" -lt 40000 ] && pass "idle RSS under 40 MB (${R} kB)" || fail "idle RSS" "${R} kB"
[ $((C1 - C0)) -le 9 ] && pass "idle CPU is negligible ($((C1 - C0)) ticks in 3 s)" || fail "idle CPU" "$((C1 - C0)) ticks"
# one wrong password through real PAM
xdotool key Caps_Lock; settle 0.3; shot caps; xdotool key Caps_Lock; settle 0.2
[ "$(status_diff idle caps)" -gt 40 ] && pass "Caps Lock hint appears" || fail "Caps Lock hint" "$(status_diff idle caps)"
type_str "wrong-password-for-the-test"; settle 0.3; shot typing
[ "$(field_diff idle typing)" -gt 300 ] && pass "typing shows dots in the field" || fail "dots in the field" "$(field_diff idle typing)"
xdotool key Return
wait_for 8 bash -c "import -window root '$SHOTS/wrong.png' && [ \$(compare -metric AE '$SHOTS/idle.png' '$SHOTS/wrong.png' -crop 420x40+430+$((FY + FH)) null: 2>&1 | grep -o '^[0-9]*') -gt 40 ]"
sleep 0.9; shot wrong
alive "$PAM_PID" && pass "still running after a wrong password" || fail "declock exited after a wrong password"
lock_win >/dev/null && pass "still locked (window present)" || fail "window gone after a wrong password"
[ "$(status_diff idle wrong)" -gt 40 ] && pass "wrong-password message shown" || fail "wrong-password message" "$(status_diff idle wrong)"
BORDER=$(hexpix "$SHOTS/wrong.png" 640 $((FY + 1)))
echo "      field border after the failure: $BORDER"
xdotool key Escape; settle 0.3; shot cleared
[ "$(field_diff idle cleared)" -lt 70 ] && pass "Escape clears the field and the message" || fail "Escape clears" "$(field_diff idle cleared)"
alive "$PAM_PID" && kill "$PAM_PID"; wait "$PAM_PID" 2>/dev/null
wait_for 3 lock_gone && pass "killing the locker removes its window (X cleans up)" || fail "window left after kill"

echo "== test-auth build (fake authenticator, password hunter2)"
FA=(DECLOCK_FAKE_AUTH=1 DECLOCK_TEST_PASSWORD=hunter2)
start "$TESTBIN" fake "${FA[@]}" || { fail "test lock did not appear"; cat "$TMP/fake.out"; exit 1; }
P=$LOCK_PID
grep -q 'fake authenticator' "$TMP/fake.out" && pass "the test build announces itself" || fail "test build banner"
settle 0.5; shot t_idle
# a second declock in the same session: exits 0 at once, first stays
env "${FA[@]}" "$TESTBIN" >/dev/null 2>&1; eq "second instance exits 0" "$?" 0
alive "$P" && pass "first instance unaffected" || fail "first instance died"
# a second declock with its own runtime dir (so the single-instance lock does not stop it): cannot grab, must fail and leave nothing
mkdir -p "$TMP/run2"; chmod 700 "$TMP/run2"
T0=$(now_ms)
env "${FA[@]}" XDG_RUNTIME_DIR=$TMP/run2 "$TESTBIN" >"$TMP/grabfail.out" 2>&1; rc=$?
T1=$(($(now_ms) - T0))
eq "a lock that cannot grab exits 1" "$rc" 1
[ "$T1" -ge 4000 ] && [ "$T1" -lt 9000 ] && pass "after retrying for about 5 s (${T1} ms)" || fail "grab retry time" "${T1} ms"
grep -q 'could not' "$TMP/grabfail.out" && pass "and says why" || fail "grab failure message" "$(cat "$TMP/grabfail.out")"
eq "exactly one lock window remains" "$(xdotool search --name '^declock$' | wc -l)" 1
# on top: a window mapped later does not cover the lock
"$FAKEWIN" under Under 336699 >/dev/null 2>&1 & PIDS+=($!)
wait_for 3 bash -c 'xdotool search --name "^Under$" | head -1 | grep -q .'
settle 0.5; shot t_ontop
OVER=$(hexpix "$SHOTS/t_ontop.png" 100 100)
[ "$(dist "$OVER" '#336699')" -gt 60 ] && pass "a window mapped afterwards stays under the lock ($OVER)" || fail "lock was covered" "$OVER"
# typing, Backspace, Escape
type_str "abcd"; settle 0.3; shot t_dots4
type_str "e"; settle 0.3; shot t_dots5
xdotool key BackSpace; settle 0.3; shot t_dots4b
[ "$(field_diff t_idle t_dots4)" -gt 100 ] && pass "dots appear" || fail "dots appear"
[ "$(field_diff t_dots4 t_dots5)" -gt 20 ] && pass "a fifth character adds a dot" || fail "fifth dot" "$(field_diff t_dots4 t_dots5)"
eq "Backspace removes it again" "$(field_diff t_dots4 t_dots4b)" 0
xdotool key Escape; settle 0.3; shot t_esc
[ "$(field_diff t_idle t_esc)" -lt 70 ] && pass "Escape empties the field" || fail "Escape empties the field" "$(field_diff t_idle t_esc)"
xdotool key ctrl+a; xdotool key super+x; xdotool key XF86AudioRaiseVolume; xdotool key F5; settle 0.3; shot t_ign
[ "$(field_diff t_idle t_ign)" -lt 70 ] && pass "shortcuts, F-keys and XF86 keys type nothing" || fail "ignored keys" "$(field_diff t_idle t_ign)"
type_str "x"; xdotool key ctrl+u; settle 0.3; shot t_cu
[ "$(field_diff t_idle t_cu)" -lt 70 ] && pass "Ctrl+U clears the field" || fail "Ctrl+U" "$(field_diff t_idle t_cu)"
# wrong password 1: shake, message, field cleared; typing again dismisses the message
type_str "nope"; xdotool key Return
for i in 1 2 3; do shot t_shake$i; done
sleep 0.8; shot t_wrong1
[ "$(status_diff t_idle t_wrong1)" -gt 40 ] && pass "'Wrong password' is shown" || fail "wrong password message"
alive "$P" && pass "still locked after 1 failure" || fail "unlocked by a wrong password"
SH_D=0; for i in 1 2 3; do d=$(field_diff t_wrong1 t_shake$i); [ "${d:-0}" -gt "$SH_D" ] && SH_D=$d; done
[ "$SH_D" -gt 20 ] && pass "the field shakes (a frame during the shake differs, $SH_D px)" || fail "shake" "$SH_D"
# failures 2 and 3: the third freezes input for 1 s
type_str "nope"; xdotool key Return; sleep 0.7
type_str "nope"; xdotool key Return
sleep 0.15; shot t_frozen
[ "$(status_diff t_wrong1 t_frozen)" -gt 40 ] && pass "after the third failure the message shows the countdown" || fail "freeze message"
# wait the 1 s freeze out, then the fourth failure freezes for 2 s
sleep 1.3
type_str "nope"; xdotool key Return
sleep 0.15
xdotool type --delay 5 -- hunter2 key Return; sleep 0.1
alive "$P" && pass "the RIGHT password is ignored during a freeze" || fail "unlocked during the freeze"
sleep 0.2; shot t_frozen2
[ "$(field_diff t_idle t_frozen2)" -lt 70 ] && pass "and nothing typed during the freeze is kept" || fail "typed text kept during freeze" "$(field_diff t_idle t_frozen2)"
sleep 2.0
# now the right password, normal flow: unlock, exit 0, window gone, grabs released
T0=$(now_ms)
type_str "hunter2"; xdotool key Return
wait_for 6 bash -c "! kill -0 $P 2>/dev/null"
TU=$(($(now_ms) - T0))
wait "$P"; eq "the right password unlocks: exit 0" "$?" 0
echo "      unlock took ${TU} ms"
lock_gone && pass "the lock window is gone" || fail "lock window still there"
xdotool key a >/dev/null 2>&1 && pass "keyboard is free again (XTEST key accepted)" || fail "keyboard grab left behind"
grep -rq hunter2 "$TMP"/*.out 2>/dev/null && fail "password appears in the program's output" || pass "the password is not printed anywhere"

echo "== xss-lock handshake"
mkfifo "$TMP/xss.fifo"
cat "$TMP/xss.fifo" > "$TMP/xss.eof" & CAT=$!; PIDS+=($CAT)
( env "${FA[@]}" XSS_SLEEP_LOCK_FD=9 "$TESTBIN" 9>"$TMP/xss.fifo" >"$TMP/xss.out" 2>&1 ) &
PIDS+=($!)
wait_for 5 lock_win && pass "lock is up" || fail "lock did not appear"
wait_for 3 bash -c "! kill -0 $CAT 2>/dev/null" && pass "XSS_SLEEP_LOCK_FD is closed once the screen is locked" || fail "XSS_SLEEP_LOCK_FD still open"
LP=$(pgrep -n -f "^$TESTBIN\$" 2>/dev/null | head -1)
# unlock to end the phase
settle 0.3; type_str "hunter2"; xdotool key Return; wait_for 4 lock_gone || fail "xss test: unlock failed"

echo "== wallpaper backdrop, themes and a picture"
mkdir -p "$XDG_STATE_HOME/omarchy/current"
convert -size 800x500 gradient:'#10c030-#2030f0' "png:$XDG_STATE_HOME/omarchy/current/background"
convert -size 128x128 xc:'#20c0d0' -fill white -draw 'circle 64,64 64,20' "png:$HOME/.face"
start "$TESTBIN" wall "${FA[@]}" DECLOCK_BG=wallpaper || fail "wallpaper lock did not appear"
P=$LOCK_PID; settle 0.5; shot t_wall
G=$(hexpix "$SHOTS/t_wall.png" 100 400)
echo "      wallpaper backdrop pixel: $G"
[ "$(dist "$G" "$(hexpix "$SHOTS/t_idle.png" 100 400)")" -gt 40 ] && pass "--bg wallpaper uses the wallpaper, not the screen" || fail "wallpaper backdrop"
FACE=$(hexpix "$SHOTS/t_wall.png" $((640 - 24)) $((397 + 20)))
[ "$(dist "$FACE" '#20C0D0')" -lt 70 ] && pass "~/.face is shown as the avatar ($FACE)" || fail "avatar picture" "$FACE"
type_str "hunter2"; xdotool key Return; wait_for 4 lock_gone; wait "$P"
rm -f "$HOME/.face"
echo '{"theme":{"name":"macos","mode":"light"}}' > "$DECEBAL_CONFIG/decebal.json"
start "$TESTBIN" light "${FA[@]}" DECLOCK_BG=color || fail "color lock did not appear"
P=$LOCK_PID; settle 0.5; shot t_light
alive "$P" && pass "--bg color with a light theme starts" || fail "light theme start"
type_str "hunter2"; xdotool key Return; wait_for 4 lock_gone; wait "$P"

echo; echo "passed $PASS, failed $FAIL (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
