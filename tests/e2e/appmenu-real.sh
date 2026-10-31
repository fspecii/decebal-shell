#!/usr/bin/env bash
# Real exporters for src/appmenu: a Qt 6 window on a private Xvfb and a libdbusmenu-glib exporter,
# both on a private dbus-daemon with no service directories. Never touches the host session:
# HOME/XDG are fresh, host DISPLAY/WAYLAND/XAUTHORITY/session variables are dropped, the display
# number is chosen by Xvfb (-displayfd) and the bus socket lives under the private root.
# Needs: dbus-daemon, Xvfb, xprop, python3-gobject + libdbusmenu-glib typelib, g++ + Qt6Widgets.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd)
REPO=$(cd -- "$HERE/../.." && pwd)
TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/decebal-appmenu-target}
ROOT=$(mktemp -d /var/tmp/decebal-appmenu-real.XXXXXX)
PIDS=()
cleanup() { set +e; for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done; wait 2>/dev/null; case $ROOT in /var/tmp/decebal-appmenu-real.??????) rm -rf -- "$ROOT" ;; esac; }
trap cleanup EXIT
mkdir -p "$ROOT/home" "$ROOT/config" "$ROOT/cache" "$ROOT/state" "$ROOT/data" "$ROOT/runtime" "$TARGET/appmenu-fixtures"
chmod 700 "$ROOT/runtime"

QT_BIN="$TARGET/appmenu-fixtures/qt-fixture"
if [[ ! -x $QT_BIN || $HERE/appmenu/qt-fixture.cpp -nt $QT_BIN ]]; then
  # shellcheck disable=SC2046
  nice -n10 g++ -std=c++17 -O1 -fPIC "$HERE/appmenu/qt-fixture.cpp" -o "$QT_BIN" $(pkg-config --cflags --libs Qt6Widgets)
fi

cat >"$ROOT/bus.conf" <<EOF
<busconfig><type>session</type><listen>unix:path=$ROOT/bus</listen><auth>EXTERNAL</auth>
<policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>
EOF
env -i PATH=/usr/bin:/bin HOME="$ROOT/home" dbus-daemon --config-file="$ROOT/bus.conf" --nofork --nopidfile >"$ROOT/bus.log" 2>&1 & PIDS+=($!)
for _ in {1..200}; do [[ -S $ROOT/bus ]] && break; sleep .02; done
[[ -S $ROOT/bus ]] || { echo "private bus did not start"; exit 1; }

# Never -displayfd (it probes upward from :0, and a host server without a lock file gets shadowed).
# Reserved high numbers only; a number is usable when no lock file, socket path or abstract socket
# exists for it. Nothing foreign is ever unlinked: a taken number is skipped.
DISP=
for n in $(shuf -i 700-899 -n 80); do
  [[ -e /tmp/.X$n-lock || -e /tmp/.X11-unix/X$n ]] && continue
  grep -qE " @?/tmp/\.X11-unix/X$n\$" /proc/net/unix && continue
  DISP=":$n"; break
done
[[ -n $DISP ]] || { echo "no free reserved display number (700-899)"; exit 1; }
N=${DISP#:}
(( N >= 700 )) || { echo "refusing display $DISP"; exit 1; }
XAUTH="$ROOT/xauthority"
touch "$XAUTH"; chmod 600 "$XAUTH"
xauth -q -f "$XAUTH" add "$DISP" . "$(mcookie)"
env -i PATH=/usr/bin:/bin HOME="$ROOT/home" Xvfb "$DISP" -auth "$XAUTH" -screen 0 800x600x24 -nolisten tcp >"$ROOT/xvfb.log" 2>&1 & XVFB_PID=$!; PIDS+=($XVFB_PID)
# Before any client connects: the lock file names our Xvfb, and both the socket path and the
# abstract socket for this number are listening sockets held by that same process.
owned() {
  [[ $(tr -dc 0-9 <"/tmp/.X$N-lock" 2>/dev/null) == "$XVFB_PID" ]] || return 1
  local listing; listing=$(ss -xlp 2>/dev/null) || return 1
  grep -E " /tmp/\.X11-unix/X$N " <<<"$listing" | grep -q "pid=$XVFB_PID," || return 1
  grep -E " @/tmp/\.X11-unix/X$N " <<<"$listing" | grep -q "pid=$XVFB_PID," || return 1
  [[ $(grep -cE " @?/tmp/\.X11-unix/X$N\$" /proc/net/unix) == 2 ]]
}
for _ in {1..250}; do
  owned && break
  kill -0 "$XVFB_PID" 2>/dev/null || { echo "Xvfb did not start"; cat "$ROOT/xvfb.log"; exit 1; }
  sleep .02
done
owned || { echo "display $DISP is not provably our Xvfb; stopping only our own process"; kill "$XVFB_PID"; exit 1; }
env -i PATH=/usr/bin:/bin DISPLAY="$DISP" XAUTHORITY="$XAUTH" xprop -root -len 0 _NET_SUPPORTED >/dev/null 2>&1 || { echo "private display not answering"; exit 1; }

BUS="unix:path=$ROOT/bus"
STATUS=0
DECEBAL_APPMENU_EXTRA_ENV="DISPLAY=$DISP XAUTHORITY=$XAUTH DECEBAL_APPMENU_DISPLAY=$DISP DECEBAL_APPMENU_BUS=$BUS DBUS_SESSION_BUS_ADDRESS=$BUS DECEBAL_APPMENU_REAL_ROOT=$ROOT DECEBAL_APPMENU_QT_FIXTURE=$QT_BIN DECEBAL_APPMENU_DBUSMENU_FIXTURE=$HERE/appmenu/dbusmenu-fixture.py DECEBAL_BACKEND=x11 QT_QPA_PLATFORMTHEME=" \
  "$HERE/appmenu-cargo.sh" test -j2 --offline --lib appmenu::real_tests -- --ignored --exact appmenu::real_tests::real_qt_and_libdbusmenu_exporters --test-threads=1 --nocapture || STATUS=$?
if [[ $STATUS != 0 ]]; then tail -n 40 "$ROOT"/*.log 2>/dev/null || true; fi
exit "$STATUS"
