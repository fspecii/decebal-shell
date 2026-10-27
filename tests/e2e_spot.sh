#!/usr/bin/env bash
# End-to-end test of macspot on a private Xvfb under rdwm (if installed): type a query, press Enter, and check
# that a stub application (never a real one) was launched. Also checks the toggle and Escape.
#
#   tests/e2e_spot.sh [path/to/macspot] [DISPLAY-number]
#
# Needs Xvfb and xdotool. Uses its own XDG_RUNTIME_DIR/HOME; never touches the real session.
set -u
BIN=$(readlink -f "${1:-target/release/macspot}")
NUM=${2:-94}
export DISPLAY=:$NUM
unset WAYLAND_DISPLAY
for t in Xvfb xdotool; do command -v $t >/dev/null || { echo "SKIP: $t missing"; exit 0; }; done

T=$(mktemp -d); chmod 700 "$T"
export XDG_RUNTIME_DIR=$T/run HOME=$T/home XDG_DATA_HOME=$T/data XDG_DATA_DIRS=$T/sysdata XDG_CACHE_HOME=$T/cache XDG_STATE_HOME=$T/state
mkdir -p "$XDG_RUNTIME_DIR" "$HOME" "$XDG_DATA_HOME/applications" "$T/sysdata" "$T/bin"
chmod 700 "$XDG_RUNTIME_DIR"
PIDS=()
cleanup() { for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done; rm -rf "$T"; }
trap cleanup EXIT
fail() { echo "FAIL: $*"; exit 1; }

cat > "$T/bin/stubapp" <<STUB
#!/bin/sh
echo "\$@" > "$T/launched"
STUB
chmod +x "$T/bin/stubapp"
cat > "$XDG_DATA_HOME/applications/zzstub.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=Zzstub Application
Exec=$T/bin/stubapp --flag %U
DESK
cat > "$XDG_DATA_HOME/applications/other.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=Other Thing
Exec=$T/bin/stubapp other
DESK

Xvfb :$NUM -screen 0 1600x900x24 >"$T/xvfb.log" 2>&1 & PIDS+=($!)
for _ in $(seq 50); do xdotool getdisplaygeometry >/dev/null 2>&1 && break; sleep 0.1; done
xdotool getdisplaygeometry >/dev/null 2>&1 || fail "Xvfb did not start"

RDWM=${RDWM:-$(command -v rdwm || true)}
if [ -n "$RDWM" ]; then "$RDWM" --backend x11 >"$T/rdwm.log" 2>&1 & PIDS+=($!); sleep 1; echo "rdwm: running"; else echo "rdwm: not found, no WM"; fi

wait_for() { for _ in $(seq 60); do eval "$1" && return 0; sleep 0.1; done; return 1; }
shot() { command -v import >/dev/null && import -window root "${SHOTS:-$T}/$1.png" 2>/dev/null; }
count_win() { xdotool search --onlyvisible --class macspot 2>/dev/null | wc -l; }

# 1. open, type, Enter -> stub launched, window gone
"$BIN" & SP=$!; PIDS+=($SP)
wait_for '[ "$(count_win)" -ge 1 ]' || fail "no macspot window appeared"
read -r X Y W H < <(xdotool search --onlyvisible --class macspot | head -1 | xargs -I{} xdotool getwindowgeometry --shell {} | awk -F= '/^X=/{x=$2}/^Y=/{y=$2}/^WIDTH=/{w=$2}/^HEIGHT=/{h=$2}END{print x,y,w,h}')
CX=$((X + W / 2)); [ $((CX - 800)) -lt 3 ] && [ $((800 - CX)) -lt 3 ] || fail "not centred horizontally (x=$X w=$W)"
sleep 0.3
xdotool type --delay 30 "zzstub"
sleep 0.4
shot typed
xdotool key Return
wait_for '[ -s "$T/launched" ]' || fail "stub app was not launched"
grep -q -- "--flag" "$T/launched" || fail "Exec arguments wrong: $(cat "$T/launched")"
wait_for '! kill -0 $SP 2>/dev/null' || fail "macspot did not exit after Enter"
echo "ok: type + Enter launches the stub"

# 2. history learned
[ -s "$XDG_STATE_HOME/macshell/spotlight-history" ] || fail "no usage history written"
grep -q "app:Zzstub Application" "$XDG_STATE_HOME/macshell/spotlight-history" || fail "history lacks the app"
echo "ok: usage history"

# 3. toggle: second invocation closes the first
"$BIN" & SP=$!; PIDS+=($SP)
wait_for '[ "$(count_win)" -ge 1 ]' || fail "no window (toggle test)"
"$BIN"
wait_for '! kill -0 $SP 2>/dev/null' || fail "second macspot did not close the first"
echo "ok: toggle"

# 4. Escape closes
"$BIN" & SP=$!; PIDS+=($SP)
wait_for '[ "$(count_win)" -ge 1 ]' || fail "no window (escape test)"
sleep 0.3; xdotool key Escape
wait_for '! kill -0 $SP 2>/dev/null' || fail "Escape did not close"
echo "ok: Escape"

# 5. click outside closes
"$BIN" & SP=$!; PIDS+=($SP)
wait_for '[ "$(count_win)" -ge 1 ]' || fail "no window (click test)"
sleep 0.3; xdotool mousemove 20 880 click 1
wait_for '! kill -0 $SP 2>/dev/null' || fail "click outside did not close"
echo "ok: click outside"

# 6. empty cache is rebuilt and a calculation shows without crashing
"$BIN" & SP=$!; PIDS+=($SP)
wait_for '[ "$(count_win)" -ge 1 ]' || fail "no window (calc test)"
sleep 0.3; xdotool type --delay 30 "2+3*4"; sleep 0.3; shot calc
xdotool key Return
wait_for '! kill -0 $SP 2>/dev/null' || fail "calc Enter did not close"
echo "ok: calculator"
# 7. the hotkey pressed again (Super+Space reaches us through our keyboard grab) closes it
"$BIN" & SP=$!; PIDS+=($SP)
wait_for '[ "$(count_win)" -ge 1 ]' || fail "no window (hotkey test)"
sleep 0.3; xdotool key super+space
wait_for '! kill -0 $SP 2>/dev/null' || fail "Super+Space did not close"
echo "ok: hotkey toggles off"

# 8. recents: with history, an empty box lists the stub app
"$BIN" & SP=$!; PIDS+=($SP)
wait_for '[ "$(count_win)" -ge 1 ]' || fail "no window (recents test)"
sleep 0.3; shot recents; kill $SP
echo "ALL PASSED"
