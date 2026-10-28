#!/bin/bash
# decclip (clipboard history daemon + picker) end to end on a private Xvfb, no window manager.
#   tests/e2e/clip.sh                 (E2E_DISPLAY=92 by default; SHOTS=dir keeps the screenshots)
# Everything is private: own Xvfb display, own XDG_RUNTIME_DIR / XDG_STATE_HOME / HOME / config. xclip is the copy source,
# examples/clipowner plays a password manager (a window with a given WM_CLASS that owns the clipboard), examples/fakewin
# receives the pasted Ctrl+V. Only PIDs started here are killed (xclip's own background owners exit with the server).
# Never touches the live display (:1) or Hyprland: WAYLAND_DISPLAY is unset and DISPLAY points at the private server.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
TARGET=${CARGO_TARGET_DIR:-$HERE/target}
BIN=$TARGET/${PROFILE:-release}
for t in Xvfb xdotool xclip xprop import jq; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline $([ "${PROFILE:-release}" = release ] && echo --release) --bin decclip --example fakewin --example clipowner) || { echo "build failed"; exit 2; }

N=${E2E_DISPLAY:-92}
[ "$N" = 1 ] && { echo "refusing the live display"; exit 2; }
[ -e "/tmp/.X11-unix/X$N" ] && { echo "display :$N is in use; set E2E_DISPLAY"; exit 2; }
PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d "${TMPDIR:-/tmp}/decclip-e2e.XXXXXX")
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

Xvfb :$N -screen 0 800x600x24 +extension RANDR +extension SHAPE +extension Composite -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X$N || { echo "Xvfb failed"; cat "$TMP/xvfb.log"; exit 2; }
export DISPLAY=:$N
unset WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE
export XDG_RUNTIME_DIR=$TMP/run XDG_CONFIG_HOME=$TMP/cfg HOME=$TMP/home XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata XDG_CACHE_HOME=$TMP/cache XDG_STATE_HOME=$TMP/state
mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME" "$HOME" "$TMP/nodata" "$XDG_CACHE_HOME" "$XDG_STATE_HOME"
chmod 700 "$XDG_RUNTIME_DIR"
export DECEBAL_CONFIG=$TMP/decebal-cfg DECEBAL_BACKEND=x11 DECEBAL_COLOR_SCHEME=dark
mkdir -p "$DECEBAL_CONFIG"
export PATH=$BIN:$TARGET/${PROFILE:-release}/examples:/usr/bin:/bin
HFILE=$XDG_STATE_HOME/decebal/clipboard-history

shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
texts() { jq -c '[.entries[].text]' "$HFILE" 2>/dev/null; }
count() { jq '.entries | length' "$HFILE" 2>/dev/null; }
clip() { xclip -o -selection clipboard 2>/dev/null; }
copy() { printf '%s' "$1" | xclip -selection clipboard; }
top_is() { [ "$(jq -r '.entries[0].text' "$HFILE" 2>/dev/null)" = "$1" ]; }
picker_win() { xdotool search --name '^Clipboard$' 2>/dev/null | head -1 | grep .; }
picker_gone() { ! picker_win; }
daemon_up() { xprop -root _DECCLIP_DAEMON 2>/dev/null | grep -q 'window id'; }
open_picker() { decclip >"$TMP/pick.out" 2>&1 & PICK_PID=$!; PIDS+=($PICK_PID); wait_for 3 picker_win || { fail "the picker did not open"; return 1; }; sleep 0.1; }
picker_exited() { ! kill -0 "$PICK_PID" 2>/dev/null; }

# --- 0. the daemon
decclip daemon >"$TMP/daemon.out" 2>&1 &
DPID=$!; PIDS+=($DPID)
wait_for 5 daemon_up && pass "the daemon is up (root property _DECCLIP_DAEMON)" || { fail "daemon did not start"; cat "$TMP/daemon.out"; exit 1; }
decclip daemon >"$TMP/daemon2.out" 2>&1; eq "a second daemon returns at once" "$?" 0
kill -0 $DPID 2>/dev/null && pass "...and the first one keeps running" || fail "first daemon died"

# --- 1. recording
copy "alpha one"; wait_for 3 top_is "alpha one"
printf 'multi\nline\nentry' | xclip -selection clipboard; wait_for 3 top_is $'multi\nline\nentry'
copy "beta two"; wait_for 3 top_is "beta two"
eq "three copies, newest first" "$(texts)" '["beta two","multi\nline\nentry","alpha one"]'
eq "history file mode" "$(stat -c %a "$HFILE")" 600
copy "alpha one"; wait_for 3 top_is "alpha one"; sleep 0.2
eq "re-copying moves the entry to the top without a duplicate" "$(texts)" '["alpha one","beta two","multi\nline\nentry"]'
copy "   "; sleep 0.4
eq "whitespace-only text is not recorded" "$(count)" 3
printf 'primary only' | xclip -selection primary; sleep 0.4
eq "PRIMARY is ignored" "$(count)" 3
head -c 150000 /dev/zero | tr '\0' 'x' | xclip -selection clipboard; sleep 0.5
eq "text over 100 KB is not recorded" "$(count)" 3
copy "alpha one"; wait_for 3 top_is "alpha one"
# put the order back to: beta two (top), then multi, alpha
copy "multi"$'\n'"line"$'\n'"entry"; wait_for 3 top_is $'multi\nline\nentry'
copy "alpha one"; wait_for 3 top_is "alpha one"
copy "beta two"; wait_for 3 top_is "beta two"
eq "order before the picker tests" "$(texts)" '["beta two","alpha one","multi\nline\nentry"]'

# --- 2. the source exits, the clipboard stays
printf '%s' "survivor text" | xclip -selection clipboard -loops 2
wait_for 3 top_is "survivor text" && pass "xclip source recorded" || fail "xclip source not recorded" "$(texts)"
sleep 0.3
eq "the clipboard is served after xclip (-loops 2) is gone" "$(clip)" "survivor text"
clipowner Writer "app text" >/dev/null 2>&1 &
APP=$!; PIDS+=($APP)
wait_for 3 top_is "app text" && pass "an application's copy is recorded" || fail "application copy not recorded" "$(texts)"
kill $APP; wait $APP 2>/dev/null
sleep 0.3
eq "after the application is killed the daemon serves its text" "$(clip)" "app text"
eq "TARGETS offers UTF8_STRING" "$(xclip -o -selection clipboard -t TARGETS 2>/dev/null | grep -c '^UTF8_STRING$')" 1
eq "STRING is served too" "$(xclip -o -selection clipboard -t STRING 2>/dev/null)" "app text"
eq "an unsupported target is refused" "$(xclip -o -selection clipboard -t image/png 2>&1 | grep -c 'not available')" 1

# --- 3. password managers
n0=$(count)
for spec in "1Password|pw-one" "Bitwarden|pw-two" "KeePassXC|pw-three" "org.keepassxc.KeePassXC|pw-four"; do
    c=${spec%%|*}; t=${spec##*|}
    clipowner "$c" "$t" >/dev/null 2>&1 &
    P=$!; PIDS+=($P)
    sleep 0.5
    eq "class $c is skipped" "$(grep -c "$t" "$HFILE")" 0
    kill $P; wait $P 2>/dev/null
done
clipowner Generic "pw-hint" --hint >/dev/null 2>&1 &
P=$!; PIDS+=($P)
sleep 0.5
eq "x-kde-passwordManagerHint is skipped" "$(grep -c pw-hint "$HFILE")" 0
kill $P; wait $P 2>/dev/null
sleep 0.3
eq "history unchanged by the secrets" "$(count)" "$n0"
[ -z "$(clip)" ] && pass "a vanished secret is not resurrected" || fail "clipboard after a secret's owner vanished" "$(clip)"
clipowner Firefox "public text" >/dev/null 2>&1 &
P=$!; PIDS+=($P)
wait_for 3 top_is "public text" && pass "an ordinary class is recorded" || fail "ordinary class not recorded"
kill $P; wait $P 2>/dev/null
# restore the order for the picker tests: public text is gone from the interesting part
copy "beta two"; wait_for 3 top_is "beta two"
copy "alpha one"; wait_for 3 top_is "alpha one"
copy "beta two"; wait_for 3 top_is "beta two"

# --- 4. the picker: filter, Return pastes into the previously focused window
FAKEWIN_LOG=$TMP/keys.log fakewin target Target 336699 >/dev/null 2>&1 &
PIDS+=($!)
wait_for 3 bash -c 'xdotool search --name "^Target$" | head -1 | grep -q .'
TARGETWIN=$(xdotool search --name '^Target$' | head -1)
xdotool windowfocus "$TARGETWIN"; sleep 0.1
T0=$(now_ms)
open_picker || exit 1
echo "      picker window found $(($(now_ms) - T0)) ms after launch (includes xdotool polling)"
shot picker-open
eq "the picker is a single window" "$(xdotool search --name '^Clipboard$' | wc -l)" 1
GEO=$(xdotool getwindowgeometry --shell "$(picker_win)" | tr '\n' ' ')
echo "      $GEO"
xdotool type --delay 30 "ALPHA"; sleep 0.15
shot picker-filtered
xdotool key Return
wait_for 3 picker_exited && pass "Return closes the picker" || fail "picker still running after Return"
eq "Return made the filtered entry the clipboard" "$(clip)" "alpha one"
sleep 0.3
eq "the entry moved to the top of the history" "$(texts | jq -r '.[0]')" "alpha one"
eq "Ctrl+V reached the previously focused window (2 key presses)" "$(wc -l < "$TMP/keys.log" 2>/dev/null)" 2
tail -1 "$TMP/keys.log" | grep -q 'state 4$' && pass "the second press (v) carried the Control modifier" || fail "v with Control" "$(cat "$TMP/keys.log")"
eq "no picker window left" "$(xdotool search --name '^Clipboard$' | wc -l)" 0

# Shift+Return: copy only, no keys
: > "$TMP/keys.log"
xdotool windowfocus "$TARGETWIN"; sleep 0.1
open_picker || exit 1
xdotool key Down; xdotool key shift+Return
wait_for 3 picker_exited
eq "Shift+Return copies the selected entry" "$(clip)" "beta two"
eq "Shift+Return sends no keys" "$(wc -c < "$TMP/keys.log")" 0

# Escape and a click outside change nothing
open_picker || exit 1
xdotool key Escape
wait_for 3 picker_exited && pass "Escape closes" || fail "Escape did not close"
eq "Escape leaves the clipboard alone" "$(clip)" "beta two"
open_picker || exit 1
xdotool mousemove 5 5 click 1
wait_for 3 picker_exited && pass "a click outside closes" || fail "click outside did not close"
eq "...and changes nothing" "$(clip)" "beta two"

# the hotkey again closes it (toggle)
open_picker || exit 1
decclip
wait_for 3 picker_exited && pass "running decclip again closes the picker" || fail "toggle"

# Alt+number
before=$(texts)
open_picker || exit 1
xdotool key alt+3
wait_for 3 picker_exited
want=$(echo "$before" | jq -r '.[2]')
sleep 0.3
eq "Alt+3 pastes the third entry" "$(clip)" "$want"

# Delete removes the entry from the file, the picker stays open
n1=$(count)
victim=$(texts | jq -r '.[1]')
open_picker || exit 1
xdotool key Down Delete; sleep 0.2
picker_win >/dev/null && pass "Delete keeps the picker open" || fail "picker closed on Delete"
eq "Delete removed one entry" "$(count)" $((n1 - 1))
eq "...the selected one" "$(texts | jq --arg v "$victim" 'index($v)')" null
shot picker-after-delete
xdotool key Escape; wait_for 3 picker_exited

# --- 5. without the daemon the picker still works from the file
kill $DPID; wait $DPID 2>/dev/null
sleep 0.2
before=$(texts)
want=$(echo "$before" | jq -r '.[1]')
open_picker || exit 1
xdotool key Down Return
wait_for 3 picker_exited
sleep 0.3
eq "no daemon: Return still sets the clipboard (helper process)" "$(clip)" "$want"
eq "no daemon: the entry moved to the top in the file" "$(texts | jq -r '.[0]')" "$want"
eq "no daemon: the history file stayed mode 600" "$(stat -c %a "$HFILE")" 600

# --- 6. clear
decclip daemon >"$TMP/daemon.out" 2>&1 &
DPID=$!; PIDS+=($DPID)
sleep 0.6
decclip clear
eq "clear empties the file" "$(count)" 0
[ -z "$(clip)" ] && pass "clear empties the clipboard, whoever holds it" || fail "clipboard after clear" "$(clip)"
copy "after clear"; wait_for 3 top_is "after clear" && pass "recording continues after clear" || fail "recording after clear"
decclip clear >/dev/null
eq "clear is idempotent" "$(count)" 0

# --- 7. cost
CPU=$(awk '{print $14 + $15}' "/proc/$DPID/stat" 2>/dev/null)
RSS=$(awk '/VmRSS/{print $2}' "/proc/$DPID/status" 2>/dev/null)
echo "      daemon: cpu ${CPU:-?} ticks total, rss ${RSS:-?} kB"
[ -n "$CPU" ] && [ "$CPU" -le 10 ] && pass "the daemon is idle-cheap (cpu ticks $CPU)" || fail "daemon cpu" "${CPU:-gone}"

echo
echo "$PASS passed, $FAIL failed"
[ "$FAIL" = 0 ]
