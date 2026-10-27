#!/bin/bash
# End-to-end: macshell's launcher starts the real Chrome on a private Xvfb + rdwm although the (copied) user
# flags file says --ozone-platform=wayland. Uses a throw-away HOME/profile; never touches ~/.config/google-chrome.
# Usage: tests/e2e_launch.sh [display-number]   Env: RDWM, RDWMCTL, SHOTS=dir, CHROME (default google-chrome-stable)
set -u
D=${1:-131}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
CHROME=${CHROME:-google-chrome-stable}
RDWM=${RDWM:-rdwm}
command -v "$CHROME" >/dev/null || { echo "skip: $CHROME not installed"; exit 0; }
T=$(mktemp -d); mkdir -p $T/run $T/home/.config $T/data/applications $T/cfg $T/prof; chmod 700 $T/run
# the Omarchy default that breaks Chrome on X11
printf -- '--ozone-platform=wayland\n--ozone-platform-hint=wayland\n--no-default-browser-check\n' > $T/home/.config/chrome-flags.conf
mkdesktop() { cat > $T/data/applications/testchrome.desktop <<EOT
[Desktop Entry]
Type=Application
Name=Test Chrome
Exec=$CHROME --user-data-dir=$T/prof --no-first-run --disable-gpu --no-sandbox about:blank %U
Icon=google-chrome
StartupWMClass=google-chrome
EOT
}
mkdesktop
unset WAYLAND_DISPLAY RDWM_SOCK
export HOME=$T/home XDG_CONFIG_HOME=$T/home/.config XDG_RUNTIME_DIR=$T/run XDG_DATA_HOME=$T/data XDG_DATA_DIRS=$T/data XDG_STATE_HOME=$T/state
export MACSHELL_CONFIG=$T/cfg MACSHELL_NOTIFY=true DISPLAY=:$D
PIDS=()
cleanup() { for p in "${PIDS[@]}"; do kill $p 2>/dev/null; done; rm -rf $T; }
trap cleanup EXIT
Xvfb :$D -screen 0 1280x800x24 >/dev/null 2>&1 & PIDS+=($!)
sleep 1
$RDWM --backend x11 >$T/rdwm.log 2>&1 & PIDS+=($!)
sleep 1.5
fail=0
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fail=1; fi; }
LA=$ROOT/target/debug/examples/launchapp
[ -x $LA ] || (cd $ROOT && cargo build --offline --example launchapp -q) || exit 1

# control: with the built-in rules off Chrome follows the flags file and cannot start on X11
echo '{"launch":{"builtin":false}}' > $T/cfg/macshell.json
$LA testchrome 8 > $T/ctl.out 2>&1
check "control: without the override Chrome does not open a window" '[ -z "$(xdotool search --onlyvisible --class google-chrome 2>/dev/null)" ]'
check "control: the early exit is logged with the Wayland error" 'grep -q "EXITED EARLY" $T/state/macshell/launch.log && grep -qi wayland $T/state/macshell/launch.log'
rm -rf $T/prof/*; rm -f $T/state/macshell/launch.log

# real run with the default rules
rm -f $T/cfg/macshell.json
$LA testchrome 1 > $T/run.out 2>&1
wins=""
for i in $(seq 40); do wins=$(xdotool search --onlyvisible --class google-chrome 2>/dev/null); [ -n "$wins" ] && break; sleep 0.5; done
check "Chrome window appears" '[ -n "$wins" ]'
sleep 2
[ -n "${SHOTS:-}" ] && import -window root $SHOTS/chrome-launch.png 2>/dev/null
CPID=$(sed -n 's/.*started Test Chrome (pid \([0-9]*\)).*/\1/p' $T/state/macshell/launch.log | head -1)
PIDS+=($CPID)
check "launch log has the override" 'grep -q -- "--ozone-platform=x11" $T/state/macshell/launch.log'
check "no Wayland connection attempted" '! grep -qi wayland $T/state/macshell/launch.log && ! ls -l /proc/$CPID/fd 2>/dev/null | grep -qi wayland'
check "launcher survived as detached session" '[ "$(ps -o sid= -p $CPID | tr -d " ")" = "$CPID" ]'
# kill the Chrome tree we started (children share the session id)
for p in $(ps -eo pid=,sid= | awk -v s=$CPID '$2==s {print $1}'); do kill $p 2>/dev/null; done
exit $fail
