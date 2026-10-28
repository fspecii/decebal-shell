#!/bin/bash
# dechot (hot corners daemon) end to end on a private Xvfb, no window manager.
#   tests/e2e/hot.sh                  (E2E_DISPLAY=93 by default)
# Everything is private: own Xvfb display, own XDG_RUNTIME_DIR / HOME / DECEBAL_CONFIG. The commands a corner runs are
# shell scripts in $TMP/shim that append their name and arguments to $TMP/fired.log; xdotool plays the pointer.
# Only PIDs started here are killed. Never touches the live display (:1) or Hyprland: WAYLAND_DISPLAY is unset and
# DISPLAY points at the private server.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
TARGET=${CARGO_TARGET_DIR:-$HERE/target}
BIN=$TARGET/${PROFILE:-release}
for t in Xvfb xdotool xrandr; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline $([ "${PROFILE:-release}" = release ] && echo --release) --bin dechot --example fakewin) || { echo "build failed"; exit 2; }

N=${E2E_DISPLAY:-93}
[ "$N" = 1 ] && { echo "refusing the live display"; exit 2; }
[ -e "/tmp/.X11-unix/X$N" ] && { echo "display :$N is in use; set E2E_DISPLAY"; exit 2; }
PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d "${TMPDIR:-/tmp}/dechot-e2e.XXXXXX")
cleanup() {
    for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done
    sleep 0.2
    for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done
    wait 2>/dev/null
    [ -n "${KEEP_TMP:-}" ] && echo "kept $TMP" || rm -rf "$TMP"
}
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "got '$2', want '$3'"; fi; }
now_ms() { echo $(($(date +%s%N) / 1000000)); }
wait_for() { local t=$1; shift; local end=$(($(now_ms) + t * 1000)); while [ "$(now_ms)" -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 0.02; done; return 1; }

Xvfb :$N -screen 0 800x600x24 +extension RANDR -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X$N || { echo "Xvfb failed"; cat "$TMP/xvfb.log"; exit 2; }
export DISPLAY=:$N
unset WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE
export XDG_RUNTIME_DIR=$TMP/run XDG_CONFIG_HOME=$TMP/cfg HOME=$TMP/home XDG_DATA_HOME=$TMP/data XDG_STATE_HOME=$TMP/state
mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME" "$HOME" "$TMP/shim"
chmod 700 "$XDG_RUNTIME_DIR"
export DECEBAL_CONFIG=$TMP/decebal-cfg
mkdir -p "$DECEBAL_CONFIG"
CFG=$DECEBAL_CONFIG/decebal.json
LOG=$TMP/fired.log; : > "$LOG"
# The shims stand for the real commands (decdash, rdwm-lock, ...); decexpose is added later on purpose.
for c in decdash decsession decspot declaunch rdwm-lock decnotify rdwmctl; do
    printf '#!/bin/sh\necho "%s $*" >> "%s"\n' "$c" "$LOG" > "$TMP/shim/$c"
    chmod +x "$TMP/shim/$c"
done
export PATH=$TMP/shim:$BIN:$TARGET/${PROFILE:-release}/examples:/usr/bin:/bin

cat > "$CFG" <<JSON
{"workspaces": "auto", "hot_corners": {"top_left": "dashboard", "top_right": "spotlight", "bottom_left": "lock", "bottom_right": "custom:echo custom-ran >> $LOG", "dwell_ms": 150}}
JSON

fired() { cat "$LOG"; }
nfired() { wc -l < "$LOG" | tr -d ' '; }
mv_to() { xdotool mousemove "$1" "$2"; }
wins() { xdotool search --name '^dechot corner$' 2>/dev/null | wc -l | tr -d ' '; }
ticks() { awk '{print $14 + $15}' /proc/$DPID/stat; }
at_least() { [ "$(nfired)" -ge "$1" ]; }

# --- 0. the daemon
dechot daemon >"$TMP/daemon.out" 2>&1 &
DPID=$!; PIDS+=($DPID)
wait_for 5 test "$(wins)" = 4 && pass "four corner windows (one monitor)" || { fail "corner windows missing" "$(wins)"; cat "$TMP/daemon.out"; exit 1; }
dechot daemon >"$TMP/daemon2.out" 2>&1; eq "a second daemon returns at once" "$?" 0
kill -0 $DPID 2>/dev/null && pass "...and the first one keeps running" || fail "first daemon died"
eq "dechot get shows the config" "$(dechot get | tr -s ' ' | tr '\n' ';')" "top-left dashboard;top-right spotlight;bottom-left lock;bottom-right custom:echo custom-ran >> $LOG;dwell_ms 150;"

# --- 1. each corner runs its command, once, after the dwell
mv_to 400 300; sleep 0.2
mv_to 0 0
sleep 0.05; eq "nothing before the dwell is over" "$(nfired)" 0
wait_for 2 at_least 1; sleep 0.3
eq "top-left runs decdash once" "$(fired)" "decdash "
mv_to 400 300; sleep 0.6
mv_to 799 0; wait_for 2 at_least 2; sleep 0.3
eq "top-right runs decspot" "$(fired | tail -1)" "decspot "
mv_to 400 300; sleep 0.6
mv_to 0 599; wait_for 2 at_least 3; sleep 0.3
eq "bottom-left runs rdwm-lock" "$(fired | tail -1)" "rdwm-lock "
mv_to 400 300; sleep 0.6
mv_to 799 599; wait_for 2 at_least 4; sleep 0.3
eq "bottom-right runs its custom shell command" "$(fired | tail -1)" "custom-ran"
eq "exactly four so far" "$(nfired)" 4

# --- 2. staying does not repeat; leave + cooldown + re-enter does
mv_to 400 300; sleep 0.6; : > "$LOG"
mv_to 0 0; wait_for 2 at_least 1; sleep 0.8
eq "staying in the corner fires once" "$(nfired)" 1
mv_to 400 300; sleep 0.05; mv_to 0 0; sleep 0.5
eq "leaving and coming straight back is inside the cooldown" "$(nfired)" 1
mv_to 400 300; sleep 0.6; mv_to 0 0; wait_for 2 at_least 2; sleep 0.2
eq "leave, wait out the cooldown, re-enter: fires again" "$(nfired)" 2

# --- 3. a quick pass-through does not fire
mv_to 400 300; sleep 0.6; : > "$LOG"
mv_to 799 0; sleep 0.04; mv_to 400 300; sleep 0.5
eq "pass-through (40 ms) does not fire" "$(nfired)" 0
for i in 1 2 3; do mv_to 799 0; sleep 0.03; mv_to 700 100; sleep 0.03; done; sleep 0.4
eq "repeated wiggles near the corner do not fire" "$(nfired)" 0

# --- 4. buttons: dragging into a corner must not fire
mv_to 400 300; sleep 0.3
xdotool mousedown 1; mv_to 799 0; sleep 0.5
eq "dragged into a corner: no fire" "$(nfired)" 0
xdotool mouseup 1; sleep 0.5
eq "...and releasing the button inside does not fire either" "$(nfired)" 0
mv_to 400 300; sleep 0.6
mv_to 799 0; sleep 0.05; xdotool mousedown 1; sleep 0.4
eq "a button pressed during the dwell cancels it" "$(nfired)" 0
xdotool mouseup 1; mv_to 400 300; sleep 0.6
mv_to 799 0; wait_for 2 at_least 1; sleep 0.2
eq "after the drag, a plain visit fires again" "$(fired)" "decspot "

# --- 5. config changes are picked up (dechot set -> inotify)
: > "$LOG"; mv_to 400 300; sleep 0.6
dechot set top-left tile-all >"$TMP/set.out" 2>&1; eq "dechot set succeeds" "$?" 0
sleep 0.3
mv_to 0 0; wait_for 2 at_least 1; sleep 0.2
eq "the new action runs without a restart" "$(fired)" "rdwmctl action tileall"
dechot set top-left bogus >/dev/null 2>&1; eq "an unknown action is refused (exit 2)" "$?" 2
dechot set middle lock >/dev/null 2>&1; eq "an unknown corner is refused (exit 2)" "$?" 2
eq "the file keeps its other keys" "$(grep -c workspaces "$CFG")" 1
: > "$LOG"; mv_to 400 300; sleep 0.6
dechot set top-left none >/dev/null; sleep 0.3
eq "an action set to none drops its window" "$(wins)" 3
mv_to 0 0; sleep 0.5
eq "...and nothing runs there" "$(nfired)" 0
dechot set top-left notifications >/dev/null; sleep 0.3
eq "an action set again brings the window back" "$(wins)" 4
sleep 0.4
eq "...and a pointer already resting there does not fire it" "$(nfired)" 0
mv_to 400 300; sleep 0.6; mv_to 0 0; wait_for 2 at_least 1; sleep 0.2
eq "notifications runs decnotify history" "$(fired)" "decnotify history"
# editing the file by hand (an editor's write-and-rename) works the same way
: > "$LOG"; mv_to 400 300; sleep 0.6
printf '{"hot_corners": {"top_right": "launchpad", "dwell_ms": 600}}\n' > "$CFG.new"; mv "$CFG.new" "$CFG"; sleep 0.3
eq "only the corner still configured has a window" "$(wins)" 1
mv_to 799 0; sleep 0.35
eq "a longer dwell_ms is respected (nothing after 350 ms)" "$(nfired)" 0
wait_for 2 at_least 1; eq "...and it fires after 600 ms" "$(fired)" "declaunch "
printf 'this is { not json' > "$CFG"; sleep 0.3
eq "an unreadable file turns the corners off" "$(wins)" 0
kill -0 $DPID 2>/dev/null && pass "...without killing the daemon" || fail "daemon died on a bad file"
printf '{"hot_corners": {"top_left": "custom:echo hand-edited >> %s", "bottom_right": "overview", "dwell_ms": 100}}\n' "$LOG" > "$CFG"; sleep 0.3
: > "$LOG"; mv_to 0 0; wait_for 2 at_least 1; sleep 0.2
eq "a repaired file works again" "$(fired)" "hand-edited"

# --- 6. an action whose program is missing does nothing; once it exists it runs
: > "$LOG"; mv_to 400 300; sleep 0.6
mv_to 799 599; sleep 0.5
eq "overview without decexpose in PATH does nothing" "$(nfired)" 0
kill -0 $DPID 2>/dev/null && pass "...and the daemon is fine" || fail "daemon died"
printf '#!/bin/sh\necho "decexpose $*" >> "%s"\n' "$LOG" > "$TMP/shim/decexpose"; chmod +x "$TMP/shim/decexpose"
mv_to 400 300; sleep 0.6; mv_to 799 599; wait_for 2 at_least 1; sleep 0.2
eq "overview runs decexpose when it is there" "$(fired)" "decexpose "

# --- 7. windows on top of the corner do not hide it
fakewin hot-test cover >/dev/null 2>&1 & FPID=$!; PIDS+=($FPID)
sleep 0.5
: > "$LOG"; mv_to 400 300; sleep 0.6; mv_to 0 0; wait_for 2 at_least 1; sleep 0.2
eq "a window mapped over the corner after the daemon: the corner still works" "$(fired)" "hand-edited"
kill $FPID 2>/dev/null

# --- 8. monitors: corners follow RandR monitors, seams between monitors are not corners
printf '{"hot_corners": {"top_left": "dashboard", "top_right": "spotlight", "bottom_left": "lock", "bottom_right": "launchpad", "dwell_ms": 100}}\n' > "$CFG"; sleep 0.3
# Xvfb ignores --setmonitor (its RandR is static), so this part only runs on a server that honours it.
xrandr --setmonitor left 300/79x600/158+0+0 none >/dev/null 2>&1; xrandr --setmonitor right 300/79x600/158+500+0 none >/dev/null 2>&1
if xrandr --listmonitors | grep -q ' left$'; then
    # two monitors with a gap between them, covering 0-299 and 500-799 (the first one probably shadows the "default" one)
    sleep 0.5
    : > "$LOG"; mv_to 400 300; sleep 0.6
    mv_to 299 0; wait_for 2 at_least 1; sleep 0.2
    eq "the right edge of the left monitor is a corner now (RRScreenChangeNotify rebuilt the windows)" "$(fired)" "decspot "
    mv_to 400 300; sleep 0.6; : > "$LOG"
    mv_to 400 0; sleep 0.5
    eq "the gap between the monitors is not" "$(nfired)" 0
    mv_to 500 599; wait_for 2 at_least 1; sleep 0.2
    eq "the bottom-left of the right monitor runs rdwm-lock" "$(fired)" "rdwm-lock "
    xrandr --delmonitor right >/dev/null 2>&1; xrandr --delmonitor left >/dev/null 2>&1; sleep 0.5
    # side by side without a gap: the seam is no corner
    xrandr --setmonitor left 400/100x600/158+0+0 none >/dev/null 2>&1; xrandr --setmonitor right 400/100x600/158+400+0 none >/dev/null 2>&1; sleep 0.5
    : > "$LOG"; mv_to 200 300; sleep 0.6
    mv_to 399 0; sleep 0.5; mv_to 400 0; sleep 0.5; mv_to 399 599; sleep 0.5; mv_to 400 599; sleep 0.5
    eq "the seam between touching monitors does not trigger" "$(nfired)" 0
    mv_to 0 0; wait_for 2 at_least 1; sleep 0.2
    eq "...while the outer corners do" "$(fired)" "decdash "
    xrandr --delmonitor right >/dev/null 2>&1; xrandr --delmonitor left >/dev/null 2>&1; sleep 0.5
    : > "$LOG"; mv_to 400 300; sleep 0.6; mv_to 299 0; sleep 0.5
    eq "after the monitors are gone the old corner is gone too" "$(nfired)" 0
else
    echo "  skip multi-monitor checks (this X server ignores xrandr --setmonitor; the geometry is covered by the unit tests)"
fi

# --- 9. idle costs nothing
sleep 0.5; T0=$(ticks); sleep 3; T1=$(ticks)
eq "no CPU while idle (clock ticks over 3 s)" "$((T1 - T0))" 0
eq "the daemon sleeps" "$(awk '{print $3}' /proc/$DPID/stat)" S

echo; echo "$PASS passed, $FAIL failed"
[ "$FAIL" = 0 ]
