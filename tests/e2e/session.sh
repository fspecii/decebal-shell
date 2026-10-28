#!/bin/bash
# decsession (session menu overlay) and decutil (record / idle-inhibit / gamemode / dnd) end to end.
#   tests/e2e/session.sh                (E2E_DISPLAY=97 by default; SHOTS=dir keeps the screenshots)
# Everything is private: own Xvfb display, own XDG_RUNTIME_DIR / HOME / config, an unreachable session bus (a second
# phase starts its own dbus-daemon + decnotify), and shim scripts for systemctl, loginctl, rdwm-lock, rdwmctl, ffmpeg, slop,
# xset, pgrep, pkill, picom, powerprofilesctl, notify-send, dunstctl and makoctl in a temp PATH, so the real ones are never
# found (nothing is suspended, locked, recorded, killed or toggled). Only PIDs started here are killed. Never touches the
# live display (:1) or Hyprland: WAYLAND_DISPLAY is unset and DISPLAY points at the private server.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
TARGET=${CARGO_TARGET_DIR:-$HERE/target}
BIN=$TARGET/${PROFILE:-release}
for t in Xvfb xdotool import convert xsetroot; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline $([ "${PROFILE:-release}" = release ] && echo --release) --bin decsession --bin decutil --bin decnotify) || { echo "build failed"; exit 2; }

N=${E2E_DISPLAY:-97}
[ "$N" = 1 ] && { echo "refusing the live display"; exit 2; }
PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d "${TMPDIR:-/tmp}/session-e2e.XXXXXX")
SHOTS=${SHOTS:-$TMP/shots}; mkdir -p "$SHOTS"
S=$TMP/state
mkdir -p "$S"
cleanup() {
    for f in picom.pid ffmpeg.pid; do [ -s "$S/$f" ] && PIDS+=("$(cat "$S/$f")"); done
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

SW=1024; SH=600
Xvfb :$N -screen 0 ${SW}x${SH}x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X$N || { echo "Xvfb failed"; cat "$TMP/xvfb.log"; exit 2; }
export DISPLAY=:$N
unset WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE
export XDG_RUNTIME_DIR=$TMP/run XDG_CONFIG_HOME=$TMP/cfg HOME=$TMP/home XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata XDG_CACHE_HOME=$TMP/cache XDG_STATE_HOME=$TMP/xstate
mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME" "$HOME" "$TMP/nodata" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$TMP/shim"
chmod 700 "$XDG_RUNTIME_DIR"
export DECEBAL_CONFIG=$TMP/decebal-cfg DECEBAL_BACKEND=x11 DECEBAL_COLOR_SCHEME=dark DECSESSION_TIMING=1
mkdir -p "$DECEBAL_CONFIG"
export USER=e2euser DECUTIL_RECORD_DIR=$TMP/videos
export DBUS_SESSION_BUS_ADDRESS=unix:path=$TMP/no-such-bus
export DECSESSION_POWER_STATE=$TMP/power_state
echo "freeze mem disk" > "$DECSESSION_POWER_STATE"
export SHIM_STATE=$S
xsetroot -solid '#808080'

# ---- shims -----------------------------------------------------------------------------------------------------------
# calls.log: every shim call; cmds.log: the session commands only.
for n in systemctl loginctl rdwm-lock rdwmctl; do
    cat > "$TMP/shim/$n" <<'SH'
#!/bin/bash
echo "$(basename "$0")${*:+ $*}" >> "${SHIM_STATE:?}/cmds.log"
exit 0
SH
done
for n in dunstctl makoctl; do printf '#!/bin/bash\nexit 1\n' > "$TMP/shim/$n"; done
cat > "$TMP/shim/notify-send" <<'SH'
#!/bin/bash
echo "$*" >> "${SHIM_STATE:?}/notify.log"
SH
cat > "$TMP/shim/slop" <<'SH'
#!/bin/bash
echo "slop $*" >> "${SHIM_STATE:?}/calls.log"
[ -n "${SLOP_FAIL:-}" ] && exit 1
echo "100 100 401 300"
SH
cat > "$TMP/shim/ffmpeg" <<'SH'
#!/bin/bash
S=${SHIM_STATE:?}
echo "ffmpeg $*" >> "$S/calls.log"
out="${@: -1}"
echo $$ > "$S/ffmpeg.pid"
echo header > "$out"
trap 'echo trailer >> "$out"; exit 0' INT TERM
while :; do sleep 0.05; done
SH
cat > "$TMP/shim/xset" <<'SH'
#!/bin/bash
S=${SHIM_STATE:?}
echo "xset $*" >> "$S/calls.log"
g() { cat "$S/xs_$1" 2>/dev/null || echo "$2"; }
case "$1" in
q) printf 'Keyboard Control:\n  auto repeat:  on\nScreen Saver:\n  prefer blanking:  yes    allow exposures:  yes\n  timeout:  %s    cycle:  %s\nDPMS (Energy Star):\n  Standby: 0    Suspend: 0    Off: %s\n  DPMS is %s\n' "$(g timeout 600)" "$(g cycle 600)" "$(g off 900)" "$(g dpms Enabled)";;
s) case "$2" in off) echo 0 > "$S/xs_timeout";; on|blank|noblank|expose|noexpose|default) ;; *) echo "$2" > "$S/xs_timeout"; [ -n "${3:-}" ] && echo "$3" > "$S/xs_cycle";; esac;;
dpms) echo "$4" > "$S/xs_off";;
-dpms) echo Disabled > "$S/xs_dpms";;
+dpms) echo Enabled > "$S/xs_dpms";;
esac
exit 0
SH
# picom: a fake compositor that just sleeps; pgrep / pkill only ever look at the pid it wrote itself.
cat > "$TMP/shim/picom" <<'SH'
#!/bin/bash
S=${SHIM_STATE:?}
echo "picom $*" >> "$S/calls.log"
echo "$*" > "$S/picom.args"; echo $$ > "$S/picom.pid"
exec sleep 600
SH
cat > "$TMP/shim/pgrep" <<'SH'
#!/bin/bash
S=${SHIM_STATE:?}
echo "pgrep $*" >> "$S/calls.log"
pid=$(cat "$S/picom.pid" 2>/dev/null)
[ -n "$pid" ] && kill -0 "$pid" 2>/dev/null || exit 1
case "$*" in
"-ax picom") echo "$pid picom $(cat "$S/picom.args")";;
"-x picom") echo "$pid";;
*) exit 1;;
esac
SH
cat > "$TMP/shim/pkill" <<'SH'
#!/bin/bash
S=${SHIM_STATE:?}
echo "pkill $*" >> "$S/calls.log"
[ "$*" = "-x picom" ] || exit 1
pid=$(cat "$S/picom.pid" 2>/dev/null)
[ -n "$pid" ] && kill "$pid" 2>/dev/null
rm -f "$S/picom.pid"
exit 0
SH
cat > "$TMP/shim/powerprofilesctl" <<'SH'
#!/bin/bash
S=${SHIM_STATE:?}
echo "powerprofilesctl $*" >> "$S/calls.log"
cur=$(cat "$S/profile" 2>/dev/null || echo balanced)
case "$1" in
list) for p in performance balanced power-saver; do [ "$p" = "$cur" ] && echo "* $p:" || echo "  $p:"; echo "    CpuDriver:	amd_pstate"; echo; done;;
get) echo "$cur";;
set) echo "$2" > "$S/profile";;
esac
exit 0
SH
chmod +x "$TMP/shim/"*
export PATH=$TMP/shim:$BIN:/usr/bin:/bin
for t in systemctl loginctl rdwm-lock rdwmctl ffmpeg slop xset pgrep pkill picom powerprofilesctl notify-send dunstctl makoctl; do
    [ "$(command -v $t)" = "$TMP/shim/$t" ] || { echo "$t resolves to $(command -v $t), not the shim"; exit 2; }
done

cmds() { cat "$S/cmds.log" 2>/dev/null; }
calls() { cat "$S/calls.log" 2>/dev/null; }
count() { grep -c -- "$2" "$1" 2>/dev/null || true; }
shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
hexpix() { convert "$1" -crop 1x1+"$2"+"$3" +repage -depth 8 txt:- 2>/dev/null | grep -o '#[0-9A-F]\{6\}' | head -1; }
dist() { local a=${1#\#} b=${2#\#}; local s=0 i x y; for i in 0 2 4; do x=$((16#${a:$i:2})); y=$((16#${b:$i:2})); d=$((x > y ? x - y : y - x)); s=$((s + d)); done; echo $s; }
red() { local a=${1#\#}; echo $((16#${a:0:2} - 16#${a:2:2})); }

# ============================================================= decsession =====================================================
sess_win() { xdotool search --name '^decsession$' 2>/dev/null | head -1 | grep .; }
proc_gone() { [ ! -d "/proc/$1" ] || grep -q '^State:.*Z' "/proc/$1/status" 2>/dev/null; }
sess_gone() { proc_gone "$SPID"; }
start_sess() {
    PATH=${SESS_PATH:-$PATH} decsession >"$TMP/sess.out" 2>&1 &
    SPID=$!; PIDS+=($SPID)
    wait_for 3 sess_win || { fail "decsession window did not appear"; return 1; }
    sleep 0.25
}
# press KEYS...: sends them one at a time
press() { for k in "$@"; do xdotool key "$k"; sleep 0.08; done; }
# expect_cmd NAME "line": the session command ran exactly once and the overlay is gone
expect_cmd() {
    wait_for 3 sess_gone || fail "$1: decsession still running"
    wait_for 2 grep -qx -- "$2" "$S/cmds.log"
    sleep 0.15
    eq "$1: runs '$2'" "$(cmds)" "$2"
    : > "$S/cmds.log"
}
expect_nothing() {
    wait_for 3 sess_gone || fail "$1: decsession still running"
    sleep 0.3
    eq "$1: runs nothing" "$(cmds)" ""
    : > "$S/cmds.log"
}
: > "$S/cmds.log"

echo "== decsession"
start_sess || exit 1
shot menu
SESS=$(sess_win)
eval "$(xdotool getwindowgeometry --shell "$SESS" | grep -E '^(X|Y|WIDTH|HEIGHT)=')"
eq "overlay covers the screen" "${WIDTH}x${HEIGHT}+$X+$Y" "${SW}x${SH}+0+0"
echo "      $(grep -h 'first frame' "$TMP/sess.out" | head -1)"
MS=$(sed -n 's/.*painted \([0-9]*\) ms.*/\1/p' "$TMP/sess.out" | head -1)
[ -n "$MS" ] && [ "$MS" -lt 80 ] && pass "first frame painted in ${MS} ms (< 80)" || fail "start-up time" "${MS:-no timing line}"
CORNER=$(hexpix "$SHOTS/menu.png" 3 3); DESK=#808080
[ "$(dist "$CORNER" "$DESK")" -gt 150 ] && pass "background is dimmed ($CORNER over $DESK)" || fail "background is dimmed" "$CORNER"
# six buttons (hibernate offered): 112 px squares, 28 px apart, centred; row top at (600 - 156) / 2 - 24
btn_x() { echo $(((SW - 812) / 2 + 56 + $1 * 140)); }
BY=$((198 + 56))
LIT=0; for i in 0 1 2 3 4 5; do p=$(hexpix "$SHOTS/menu.png" $(($(btn_x $i) - 45)) $BY); [ "$(dist "$p" "$CORNER")" -gt 60 ] && LIT=$((LIT + 1)); done
eq "six buttons are drawn" "$LIT" 6
GAPPX=$(hexpix "$SHOTS/menu.png" $(($(btn_x 0) + 70)) $BY)
[ "$(dist "$GAPPX" "$CORNER")" -lt 30 ] && pass "gap between buttons is plain dim" || fail "gap pixel" "$GAPPX"
NTEXT=$(convert "$SHOTS/menu.png" -crop 600x20+212+$((198 + 112 + 10)) +repage -format '%k' info: 2>/dev/null)
[ "${NTEXT:-0}" -gt 3 ] && pass "labels are drawn ($NTEXT colours)" || fail "labels are drawn" "$NTEXT"

# --- keys that run at once
press l;       expect_cmd "L" "rdwm-lock"
start_sess; press s;       expect_cmd "S" "systemctl suspend"
start_sess; press h;       expect_cmd "H" "systemctl hibernate"
# --- destructive: first press arms, second runs
start_sess; press r
sleep 0.2; shot armed
eq "R once: nothing yet, overlay up" "$(cmds)|$(proc_gone $SPID && echo gone || echo up)" "|up"
ARM=$(hexpix "$SHOTS/armed.png" $(($(btn_x 4) - 45)) $BY)
[ "$(red "$ARM")" -gt 60 ] && pass "armed button turns red ($ARM)" || fail "armed button colour" "$ARM"
press r;       expect_cmd "R R" "systemctl reboot"
start_sess; press p p;     expect_cmd "P P" "systemctl poweroff"
start_sess; press o o;     expect_cmd "O O" "loginctl terminate-user e2euser"
# --- re-arming another button does not run the first one; Escape cancels
start_sess; press p r Escape;     expect_nothing "P, R, Escape"
start_sess; press Escape;         expect_nothing "Escape"
# --- arrows and Return
start_sess; press Right Return Return;          expect_cmd "Right Return Return (log out)" "loginctl terminate-user e2euser"
start_sess; press Left Return Return;           expect_cmd "Left Return Return (shut down, wraps)" "systemctl poweroff"
start_sess; press Right Right Return;           expect_cmd "Right Right Return (suspend)" "systemctl suspend"
start_sess; press Return;                       expect_cmd "Return on the first button (lock)" "rdwm-lock"
start_sess; press Right Left Return;            expect_cmd "Right Left Return (lock)" "rdwm-lock"
# arming, then moving the selection disarms: Return on the new button only arms that one
start_sess; press p Left Return
eq "armed shutdown, then Left: nothing ran" "$(cmds)" ""
press Escape; expect_nothing "Escape after moving"
# --- clicks
start_sess
xdotool mousemove $(btn_x 0) $BY click 1;      expect_cmd "click Lock" "rdwm-lock"
start_sess
xdotool mousemove $(btn_x 5) $BY click 1; sleep 0.2
eq "click Shut Down once: nothing yet" "$(cmds)|$(proc_gone $SPID && echo gone || echo up)" "|up"
xdotool mousemove $(btn_x 5) $BY click 1;      expect_cmd "click Shut Down twice" "systemctl poweroff"
start_sess
xdotool mousemove $(btn_x 5) $BY click 1; sleep 0.1; xdotool mousemove $(btn_x 4) $BY; sleep 0.1; xdotool click 1; sleep 0.2
eq "click Shut Down, hover Restart, click: re-armed, nothing ran" "$(cmds)" ""
xdotool mousemove 10 10 click 1;               expect_nothing "click on the dimmed background"
# --- a second launch closes the first (toggle)
start_sess; decsession >/dev/null 2>&1; wait_for 3 sess_gone && pass "a second decsession closes the first" || fail "toggle"
eq "toggle runs nothing" "$(cmds)" ""
# --- the keyboard grab is released: typing reaches a plain window again
xdotool mousemove 900 500

# --- logout through rdwm when its socket exists
touch "$XDG_RUNTIME_DIR/rdwm.sock"
start_sess; press o o;     expect_cmd "log out under rdwm" "rdwmctl action quit"
rm -f "$XDG_RUNTIME_DIR/rdwm.sock"

# --- without hibernate in /sys/power/state: five buttons and H does nothing
echo "freeze mem" > "$DECSESSION_POWER_STATE"
start_sess; shot nohib
press h; sleep 0.2
eq "H does nothing without hibernate support" "$(cmds)|$(proc_gone $SPID && echo gone || echo up)" "|up"
LIT=0; for i in 0 1 2 3 4; do x=$((SW / 2 - 336 + 56 + i * 140)); p=$(hexpix "$SHOTS/nohib.png" $((x - 45)) $BY); [ "$(dist "$p" "$CORNER")" -gt 60 ] && LIT=$((LIT + 1)); done
eq "five buttons are drawn" "$LIT" 5
press Escape; expect_nothing "Escape"
echo "freeze mem disk" > "$DECSESSION_POWER_STATE"
# --- without rdwm-lock the lock falls back to loginctl
mkdir -p "$TMP/shim2"; for f in "$TMP/shim/"*; do [ "$(basename "$f")" = rdwm-lock ] || ln -sf "$f" "$TMP/shim2/"; done
ln -sf "$(command -v basename)" "$TMP/shim2/basename"   # shims need basename; /usr/bin (and its real rdwm-lock) stays out of this PATH
SESS_PATH=$TMP/shim2:$BIN
start_sess; press l;       expect_cmd "lock without rdwm-lock" "loginctl lock-session"
SESS_PATH=
decsession --version | grep -q '^decsession ' && pass "--version" || fail "--version"
decsession --nonsense >/dev/null 2>&1; eq "an unknown argument exits 2" "$?" 2

# ============================================================== decutil =======================================================
echo "== decutil record"
: > "$S/calls.log"; : > "$S/notify.log"
eq "status before: off" "$(decutil record status)" off
decutil record start; eq "record start exits 0" "$?" 0
eq "status: on" "$(decutil record status)" on
eq "one ffmpeg started" "$(count "$S/calls.log" '^ffmpeg ')" 1
FF=$(grep '^ffmpeg ' "$S/calls.log" | head -1)
case "$FF" in *"-f x11grab"*"-i :$N "*) pass "ffmpeg grabs the private display ($FF)";; *) fail "ffmpeg command" "$FF";; esac
OUTF=$(echo "$FF" | awk '{print $NF}')
case "$OUTF" in "$DECUTIL_RECORD_DIR"/recording-*.mp4) pass "output goes to the recordings dir";; *) fail "output path" "$OUTF";; esac
decutil record start >/dev/null; eq "start twice: still one ffmpeg" "$(count "$S/calls.log" '^ffmpeg ')" 1
FFPID=$(cat "$S/ffmpeg.pid")
kill -0 "$FFPID" 2>/dev/null && pass "the recorder is running detached" || fail "recorder running"
eq "no toast while recording" "$(cat "$S/notify.log")" ""
decutil record stop; eq "record stop exits 0" "$?" 0
wait_for 2 proc_gone "$FFPID" && pass "recorder was stopped" || fail "recorder still running"
eq "status: off" "$(decutil record status)" off
grep -q trailer "$OUTF" && pass "recorder got SIGINT and finished the file" || fail "file finalised" "$(cat "$OUTF" 2>/dev/null)"
grep -q "Recording saved" "$S/notify.log" && pass "a notification says it was saved ($(tail -1 "$S/notify.log"))" || fail "stop notification" "$(cat "$S/notify.log")"
grep -q "$(basename "$OUTF")" "$S/notify.log" && pass "the notification names the file" || fail "notification names the file"
decutil record stop >/dev/null; eq "stop again: no second notification" "$(count "$S/notify.log" 'Recording saved')" 1
sleep 1.1   # file names have one-second resolution
decutil record toggle >/dev/null; eq "toggle starts" "$(decutil record status)" on
decutil record toggle >/dev/null; eq "toggle stops" "$(decutil record status)" off
eq "two notifications now" "$(count "$S/notify.log" 'Recording saved')" 2
sleep 1.1
: > "$S/calls.log"
decutil record start --region >/dev/null
RG=$(grep '^ffmpeg ' "$S/calls.log" | head -1)
case "$RG" in *"-video_size 400x300"*"-i :$N+100,100 "*) pass "--region: slop's geometry (even sized) reaches ffmpeg";; *) fail "--region ffmpeg command" "$RG";; esac
decutil record stop >/dev/null
sleep 1.1
: > "$S/calls.log"; : > "$S/notify.log"
SLOP_FAIL=1 decutil record start --region >/dev/null 2>&1; eq "cancelled slop: exit 1" "$?" 1
eq "cancelled slop: nothing recorded" "$(count "$S/calls.log" '^ffmpeg ')" 0
grep -q "Recording cancelled" "$S/notify.log" && pass "cancelled slop says so" || fail "cancel notification"
eq "cancelled slop: status off" "$(decutil record status)" off
# a recorder that dies is not "recording": kill it behind decutil's back
decutil record start >/dev/null; kill -9 "$(cat "$S/ffmpeg.pid")"; wait_for 2 proc_gone "$(cat "$S/ffmpeg.pid")"
eq "a crashed recorder shows as off" "$(decutil record status)" off
decutil -q record start >/dev/null; : > "$S/notify.log"; decutil -q record stop >/dev/null
eq "--quiet suppresses the toast" "$(cat "$S/notify.log")" ""

echo "== decutil idle-inhibit"
: > "$S/calls.log"
xq() { xset q | sed -n 's/.*timeout: *\([0-9]*\) *cycle: *\([0-9]*\).*/\1 \2/p'; }
eq "before: 600 600" "$(xq)" "600 600"
decutil idle-inhibit on; eq "on exits 0" "$?" 0
eq "status on" "$(decutil idle-inhibit status)" on
eq "screensaver timeout is 0" "$(xq)" "0 600"
xset q | grep -q 'DPMS is Disabled' && pass "DPMS is disabled" || fail "DPMS disabled"
N0=$(count "$S/calls.log" '^xset ')
decutil idle-inhibit on >/dev/null; eq "on twice: no further xset calls" "$(count "$S/calls.log" '^xset ')" "$N0"
decutil idle-inhibit off
eq "off restores the timeout" "$(xq)" "600 600"
xset q | grep -q 'DPMS is Enabled' && pass "DPMS is back on" || fail "DPMS restored"
eq "off restores DPMS timeouts" "$(cat "$S/xs_off")" 900
eq "status off" "$(decutil idle-inhibit status)" off
N0=$(count "$S/calls.log" '^xset ')
decutil idle-inhibit off >/dev/null; eq "off twice: no further xset calls" "$(count "$S/calls.log" '^xset ')" "$N0"
decutil idle-inhibit toggle >/dev/null; eq "toggle on" "$(decutil idle-inhibit status)" on
decutil idle-inhibit toggle >/dev/null; eq "toggle off" "$(decutil idle-inhibit status)" off
# a custom timeout is restored exactly
echo 300 > "$S/xs_timeout"; echo 450 > "$S/xs_cycle"
decutil idle-inhibit on >/dev/null; decutil idle-inhibit off >/dev/null
eq "custom 300/450 restored" "$(xq)" "300 450"

echo "== decutil gamemode"
: > "$S/calls.log"
picom --config /tmp/x.conf -b &
PIDS+=($!)
wait_for 2 test -s "$S/picom.pid"
eq "fake picom is running" "$(pgrep -x picom >/dev/null && echo yes)" yes
decutil gamemode on; eq "on exits 0" "$?" 0
eq "status on" "$(decutil gamemode status)" on
pgrep -x picom >/dev/null && fail "picom stopped" || pass "picom is stopped"
eq "performance profile set" "$(powerprofilesctl get)" performance
N0=$(wc -l < "$S/calls.log")
decutil gamemode on >/dev/null; eq "on twice: nothing more runs" "$(wc -l < "$S/calls.log")" "$N0"
decutil gamemode off; eq "off exits 0" "$?" 0
wait_for 2 pgrep -x picom && pass "picom is back" || fail "picom restarted"
eq "picom restarted with its own arguments" "$(cat "$S/picom.args")" "--config /tmp/x.conf -b"
eq "profile restored to balanced" "$(powerprofilesctl get)" balanced
eq "status off" "$(decutil gamemode status)" off
N0=$(wc -l < "$S/calls.log")
decutil gamemode off >/dev/null; eq "off twice: nothing more runs" "$(wc -l < "$S/calls.log")" "$N0"
decutil gamemode toggle >/dev/null; eq "toggle on" "$(decutil gamemode status)" on
decutil gamemode toggle >/dev/null; eq "toggle off" "$(decutil gamemode status)" off
eq "picom started once by hand and once per restore" "$(count "$S/calls.log" '^picom ')" 3

echo "== decutil dnd (settings-file flag, no daemon)"
eq "dnd off at first" "$(decutil dnd status)" off
decutil dnd on; eq "dnd on exits 0" "$?" 0
eq "dnd status on" "$(decutil dnd status)" on
grep -q 'dnd = true' "$XDG_CONFIG_HOME/decebal/notifications.toml" && pass "flag is in notifications.toml" || fail "dnd flag file"
decutil dnd on >/dev/null; eq "dnd on twice is fine" "$(decutil dnd status)" on
decutil dnd toggle >/dev/null; eq "dnd toggle off" "$(decutil dnd status)" off
decutil dnd toggle >/dev/null; decutil dnd off >/dev/null; eq "dnd off" "$(decutil dnd status)" off
eq "status line" "$(decutil status)" "record=off idle-inhibit=off gamemode=off dnd=off"
decutil bogus >/dev/null 2>&1; eq "an unknown command exits 2" "$?" 2
decutil record maybe >/dev/null 2>&1; eq "a bad argument exits 2" "$?" 2

# ====================================================== with a real daemon ===================================================
if command -v dbus-daemon >/dev/null; then
    echo "== decutil with decnotify on a private bus"
    dbus-daemon --session --fork --print-address=3 --print-pid=4 3>"$TMP/addr" 4>"$TMP/dpid" || { fail "private dbus-daemon"; }
    if [ -s "$TMP/addr" ]; then
        export DBUS_SESSION_BUS_ADDRESS=$(cat "$TMP/addr"); PIDS+=("$(cat "$TMP/dpid")")
        export XDG_SESSION_TYPE=x11
        decnotify >"$TMP/decnotify.log" 2>&1 &
        PIDS+=($!)
        up() { decnotify dnd status; }
        wait_for 5 up || fail "decnotify did not start" "$(cat "$TMP/decnotify.log")"
        eq "decnotify is the daemon (dnd off)" "$(decutil dnd status)" off
        decutil dnd on; eq "decutil dnd on reaches the daemon" "$(decnotify dnd status)" on
        decutil dnd toggle >/dev/null; eq "decutil dnd toggle" "$(decnotify dnd status)" off
        decutil dnd on >/dev/null   # banners hidden, history still recorded
        sleep 1.1
        decutil record start >/dev/null; decutil record stop >/dev/null
        HIST=$XDG_STATE_HOME/decebal/notification-history.json
        wait_for 3 grep -q "Recording saved" "$HIST" && pass "the stop notification reached decnotify's history (via D-Bus)" || fail "history entry" "$(cat "$HIST" 2>/dev/null | head -c 300)"
        decutil dnd off >/dev/null
    fi
else
    echo "  skip daemon phase (no dbus-daemon)"
fi

echo; echo "passed $PASS, failed $FAIL (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
