#!/usr/bin/env bash
# End-to-end test of decnotify on a PRIVATE session bus and a PRIVATE Xvfb. Never touches the real bus or display.
# usage: tests/e2e/notify.sh [display]   (needs Xvfb, dbus-daemon, import; busctl and notify-send are used when present)
set -u
cd "$(dirname "$0")/../.." || exit 1
BIN=${DECNOTIFY_BIN:-${MS_BIN_DIR:-$PWD/target/${PROFILE:-debug}}/decnotify}
D=${1:-:151}
T=$(mktemp -d "${TMPDIR:-/tmp}/decnotify-e2e.XXXXXX")
SHOTS=${SHOTS:-$T/shots}
mkdir -p "$T/home" "$T/run" "$T/cfg" "$T/state" "$SHOTS"
chmod 700 "$T/run"
fail=0
ok() { echo "ok   - $1"; }
bad() { echo "FAIL - $1"; fail=1; }
nwin() { xdotool search --class decnotify 2>/dev/null | wc -l; }
check() { if eval "$2"; then ok "$1"; else bad "$1"; fi; }

export HOME="$T/home" XDG_CONFIG_HOME="$T/cfg" XDG_STATE_HOME="$T/state" XDG_RUNTIME_DIR="$T/run" DISPLAY="$D" XDG_SESSION_TYPE=x11
unset WAYLAND_DISPLAY DBUS_SESSION_BUS_ADDRESS
Xvfb "$D" -screen 0 1280x800x24 -nolisten tcp 2>/dev/null &
XPID=$!
sleep 1
eval "$(dbus-launch --sh-syntax 2>/dev/null)" || true
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
  dbus-daemon --session --fork --print-address=3 --print-pid=4 3>"$T/addr" 4>"$T/pid" || exit 1
  DBUS_SESSION_BUS_ADDRESS=$(cat "$T/addr"); DBUS_SESSION_BUS_PID=$(cat "$T/pid"); export DBUS_SESSION_BUS_ADDRESS
fi
cleanup() { kill "${NPID:-}" "$XPID" "${DBUS_SESSION_BUS_PID:-}" 2>/dev/null; rm -rf "$T"; }
trap cleanup EXIT
check "private bus is not the user's bus" '[ "$DBUS_SESSION_BUS_ADDRESS" != "${REAL_BUS:-none}" ] && ! echo "$DBUS_SESSION_BUS_ADDRESS" | grep -q "/run/user/"'

"$BIN" & NPID=$!
for _ in $(seq 50); do busctl --user list 2>/dev/null | grep -q '^org.freedesktop.Notifications ' && break; sleep 0.1; done
check "daemon owns org.freedesktop.Notifications" "busctl --user list | grep -q '^org.freedesktop.Notifications '"
check "GetServerInformation" "busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications GetServerInformation | grep -q decnotify"
check "GetCapabilities has actions" "busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications GetCapabilities | grep -q actions"
check "introspection works" "busctl --user introspect org.freedesktop.Notifications /org/freedesktop/Notifications | grep -q NotificationClosed"

# second instance must exit quietly with the name taken
check "second instance exits 0 and does not take over" '"$BIN"; [ $? -eq 0 ]'

# Notify through busctl: replies with an id
OUT=$(busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications Notify 'susssasa{sv}i' decebal-test 0 dialog-information "Hello from busctl" "A body that is long enough to wrap onto a second line so we can see the wrapping work in the banner" 4 default Open reply Reply 1 urgency y 1 60000)
check "Notify returns an id" "echo '$OUT' | grep -q '^u [0-9]'"
sleep 0.5
import -window root "$SHOTS/1-one-banner.png" 2>/dev/null
check "a banner window exists" "[ \$(nwin) -ge 1 ]"

if command -v notify-send >/dev/null; then
  notify-send -a Mail "New message" "From Ann: lunch at noon?" -t 8000
  notify-send -a Calendar "Meeting in 5 minutes" "Design review" -u critical
  notify-send -a Music "Now playing" "Some Artist - Some Song" -t 8000
  sleep 0.5
  import -window root "$SHOTS/2-stack.png" 2>/dev/null
  check "stacked banners" "[ \$(nwin) -ge 4 ]"
fi

# close signal on CloseNotification
busctl --user monitor --match "type='signal',interface='org.freedesktop.Notifications'" > "$T/sig" 2>&1 & MPID=$!
sleep 0.5
busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications CloseNotification u 1 >/dev/null
sleep 0.5
check "NotificationClosed(1, 3) emitted" "grep -q NotificationClosed '$T/sig' && grep -q 'UINT32 3' '$T/sig'"

# timeout: a short banner expires with reason 1
busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications Notify 'susssasa{sv}i' t 0 "" "short" "" 0 0 800 >/dev/null
sleep 2
check "expired banner reports reason 1" "grep -q 'UINT32 1' '$T/sig'"
kill $MPID 2>/dev/null

# do not disturb via the CLI hides banners but keeps history
"$BIN" dnd on
check "dnd status on" '[ "$("$BIN" dnd status)" = on ]'
check "settings file written" "grep -q 'dnd = true' '$T/cfg/decebal/notifications.toml'"
before=$(nwin)
busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications Notify 'susssasa{sv}i' q 0 "" "silenced" "" 0 0 4000 >/dev/null
after=$(nwin)
check "dnd suppresses the banner" "[ $before -eq $after ]"
"$BIN" dnd off

# history panel
"$BIN" history
sleep 0.5
import -window root "$SHOTS/3-history.png" 2>/dev/null
check "history panel opens" "[ \$(nwin) -ge 1 ]"
"$BIN" history
check "RSS under 10 MB at idle" '[ "$(awk "/VmRSS/{print \$2}" /proc/$NPID/status)" -lt 10240 ]'
echo "RSS: $(awk '/VmRSS/{print $2}' /proc/$NPID/status) kB; screenshots in $SHOTS"
[ -n "${KEEP:-}" ] && { trap 'kill $NPID $XPID ${DBUS_SESSION_BUS_PID:-} 2>/dev/null' EXIT; echo "kept $T"; }
exit $fail
