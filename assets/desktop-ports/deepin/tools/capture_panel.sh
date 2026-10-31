#!/bin/bash
# One private, bounded native capture of the Deepin dock on a private Xvfb: no window manager, no session or system bus,
# private HOME/XDG, work files under /var/tmp (not the /tmp tmpfs).
#
#   DECEBAL_BIN=<dir with decthemectl and decdock> ICON_ROOT=<staged icon dir> \
#     capture_panel.sh MODE EDGE SCALE WIDTH HEIGHT OUT.png [all-applets]
#
# MODE dark|light, EDGE top|bottom|left|right, SCALE the DECEBAL_SCALE factor (1, 1.25, 2). `all-applets` switches every
# applet of docs/universal-panels.md on. Installs the theme and applies it into the private config only (`--no-rdwm`).
# Prints `ok OUT` on success (the work directory is removed again unless KEEP=1). Exit status 0 only for a validated capture:
# right size, more than 12 colours, decdock alive, no panic in its log.
# Every external command runs under `timeout`; the only processes ever signalled are the two this script started.
# Optional: BACKDROP=#rrggbb paints the root window a flat colour (so translucency has something to show through), and
# COMPOSITOR=1 starts a private picom (xrender, no blur) on the display, as a child that is stopped with the rest.
# Display numbers: reserved 700..999, checked in the lock, filesystem and abstract namespaces and proved ours before any client connects.
# STEPS="hover:start;click:clock;shot:popup.png;key:Escape;key:Tab;shot:key.png": input on the proven private display only, after the dock
# painted. hover:/click: take a zone key from the dock's layout dump (start, search, task NAME, tray NAME, clock, show_desktop ...),
# click:ZONE:NAME:BUTTON; key: an xdotool key name (decdock --focus is started first when a step is `focus`); shot: an extra screenshot in
# the directory of OUT. Every step runs under `timeout`.
# Environment: CAPTURE_WAIT (seconds the dock gets to paint, default 3), SETUP_TIMEOUT (default 60: `theme apply` took over 20 s on a host at load 12), CAPTURE_TIMEOUT (default 40).
set -u
MODE=${1:?mode}; EDGE=${2:?edge}; SCALE=${3:?scale}; W=${4:?width}; H=${5:?height}; OUT=${6:?out.png}; ALL=${7:-}
: "${DECEBAL_BIN:?}" "${ICON_ROOT:?}"
case $MODE in dark|light) ;; *) echo "bad mode $MODE" >&2; exit 64;; esac
case $EDGE in top|bottom|left|right) ;; *) echo "bad edge $EDGE" >&2; exit 64;; esac
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PROGRAM=decdock
REPO=$(cd "$HERE/../../../.." && pwd)
THEME=$REPO/themes/deepin
SETUP_TIMEOUT=${SETUP_TIMEOUT:-60}; CAPTURE_TIMEOUT=${CAPTURE_TIMEOUT:-40}
for t in Xvfb import identify magick xprop timeout; do command -v "$t" >/dev/null || { echo "$t needed" >&2; exit 77; }; done
[ -x "$DECEBAL_BIN/decthemectl" ] && [ -x "$DECEBAL_BIN/$PROGRAM" ] || { echo "decthemectl/$PROGRAM not found in $DECEBAL_BIN" >&2; exit 78; }
[ -d "$ICON_ROOT/bloom" ] || { echo "$ICON_ROOT has no bloom icon theme (install it first: decthemectl assets install deepin-bloom --root DIR, see assets/desktop-ports/deepin/README.md)" >&2; exit 78; }

WORK_ROOT=${CAPTURE_TMP:-/var/tmp/decebal-deepin}
mkdir -p "$WORK_ROOT" || exit 70
S=$(mktemp -d "$WORK_ROOT/cap.XXXXXX") || exit 70
XP= DP= CP= DPSTART=
# the start time (clock ticks since boot) of a pid, so a recycled pid is never signalled
starttime() { awk '{ sub(/^.*\) /, ""); print $20 }' "/proc/$1/stat" 2>/dev/null; }
cleanup() {
  # an adopted dock (spawned by `theme apply`, not our child) is only signalled while it is still the same process
  if [ -n "$DP" ] && [ -n "$DPSTART" ] && [ "$(starttime "$DP")" != "$DPSTART" ]; then DP=; fi
  # TERM, a bounded wait, then KILL: exact pids this script started, nothing else
  for p in $DP $CP $XP; do
    [ -n "$p" ] && kill -0 "$p" 2>/dev/null && kill -TERM "$p" 2>/dev/null
  done
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do
    alive=0
    for p in $DP $CP $XP; do [ -n "$p" ] && kill -0 "$p" 2>/dev/null && alive=1; done
    [ $alive = 0 ] && break
    sleep 0.25
  done
  for p in $DP $CP $XP; do
    [ -n "$p" ] && kill -0 "$p" 2>/dev/null && kill -KILL "$p" 2>/dev/null
  done
  for p in $DP $CP $XP; do [ -n "$p" ] && wait "$p" 2>/dev/null; done
  # remove only the work directory this script created: a cap.* directory directly inside the owned work root
  if [ "${KEEP:-0}" != 1 ] && [ -d "$S" ] && [ "$(dirname "$(realpath "$S")")" = "$(realpath "$WORK_ROOT")" ] && [[ "$(basename "$S")" == cap.* ]]; then
    rm -r -- "$S"
  fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP
fail() { echo "capture failed: $*" >&2; [ -s "$S/setup.log" ] && tail -5 "$S/setup.log" >&2; exit 1; }

mkdir -p "$S/home/.config/decebal" "$S/home/.local/share" "$S/home/.cache" "$S/cfg" "$S/run" "$S/tmp" || exit 70
chmod 700 "$S/run"
ln -s "$ICON_ROOT" "$S/home/.local/share/icons"

# Pick the display number ourselves: reserved HIGH numbers only (700..999), never bare -displayfd (it can resolve to :0, the live session,
# whose filesystem socket may be absent while its abstract socket still exists). A number is usable only if NONE of its names exists in
# either namespace: lock file, filesystem socket, abstract socket (/proc/net/unix). Foreign sockets and locks are never touched.
abstract_busy() { grep -q "@/tmp/.X11-unix/X$1\$" /proc/net/unix; }
N=
for _ in $(seq 60); do
  c=$((700 + RANDOM % 300))
  [ -e "/tmp/.X$c-lock" ] || [ -e "/tmp/.X11-unix/X$c" ] || abstract_busy "$c" || { N=$c; break; }
done
[ -n "$N" ] || fail "no free display number in 700..999"
D=:$N
Xvfb "$D" -screen 0 "${W}x${H}x24" -noreset -nolisten tcp >"$S/xvfb.log" 2>&1 &
XP=$!
for _ in $(seq 60); do
  kill -0 "$XP" 2>/dev/null || fail "Xvfb exited: $(tail -2 "$S/xvfb.log" | tr '\n' ' ')"
  [ -S "/tmp/.X11-unix/X$N" ] && abstract_busy "$N" && [ "$(cat "/tmp/.X$N-lock" 2>/dev/null | tr -d ' ')" = "$XP" ] && break
  sleep 0.1
done
# Proof the display is ours before any client connects, in every namespace: the lock file holds our pid, the pid is the Xvfb we started
# for this size, the filesystem socket exists, and the abstract socket's inode is open in OUR process (not merely present).
[ "$(cat "/tmp/.X$N-lock" 2>/dev/null | tr -d ' ')" = "$XP" ] || fail "display $D is not owned by our Xvfb (pid $XP)"
tr '\0' ' ' <"/proc/$XP/cmdline" | grep -q -- "^Xvfb $D -screen 0 ${W}x${H}x24 " || fail "pid $XP is not our Xvfb"
[ -S "/tmp/.X11-unix/X$N" ] || fail "filesystem socket of $D is missing"
INODE=$(awk -v p="@/tmp/.X11-unix/X$N" '$NF==p {print $(NF-1)}' /proc/net/unix | head -1)
[ -n "$INODE" ] || fail "abstract socket of $D is missing"
ls -l "/proc/$XP/fd" 2>/dev/null | grep -q "socket:\[$INODE\]" || fail "abstract socket of $D is not held by our Xvfb"
[ "$N" -ge 700 ] || fail "refusing a display below :700"
# the server answers with the geometry asked for
E=(env -i PATH=/usr/bin:/bin HOME="$S/home" DECEBAL_CONFIG="$S/cfg" DECEBAL_THEMES="$S/home/.config/decebal/themes" XDG_CONFIG_HOME="$S/home/.config"
   XDG_DATA_HOME="$S/home/.local/share" XDG_CACHE_HOME="$S/home/.cache" XDG_RUNTIME_DIR="$S/run" TMPDIR="$S/tmp" DISPLAY="$D" DECEBAL_BACKEND=x11
   DECEBAL_SYSTEM_AUTOMATIC=0 DBUS_SESSION_BUS_ADDRESS="unix:path=$S/run/no-session-bus" DBUS_SYSTEM_BUS_ADDRESS="unix:path=$S/run/no-system-bus"
   DECEBAL_SCALE="$SCALE" LANG=C.UTF-8 DECDOCK_DEBUG_LAYOUT="$S/layout.txt")
timeout 10 import -display "$D" -window root "$S/blank.png" 2>"$S/import.log" || fail "cannot talk to $D: $(head -2 "$S/import.log")"
[ "$(identify -format '%wx%h' "$S/blank.png" 2>/dev/null)" = "${W}x${H}" ] || fail "display $D is not ${W}x${H}"
timeout 5 xprop -display "$D" -root >"$S/root.props" 2>&1 || fail "xprop cannot read the root window"

# `theme apply` and `panel dock` reconcile the dock themselves (they spawn a detached decdock). Setup therefore runs against a display
# that does not exist: such a dock dies at once, and the one dock of this capture is started below, after the edge is written.
NODISPLAY=
for c in $(seq 1000 1100); do [ -e "/tmp/.X$c-lock" ] || [ -e "/tmp/.X11-unix/X$c" ] || abstract_busy "$c" || { NODISPLAY=:$c; break; }; done
[ -n "$NODISPLAY" ] || fail "no unused display number for setup"
ES=("${E[@]/DISPLAY=$D/DISPLAY=$NODISPLAY}")
[ "${#ES[@]}" = "${#E[@]}" ] && printf '%s\n' "${ES[@]}" | grep -qx "DISPLAY=$NODISPLAY" || fail "setup environment is not display-less"
run() { timeout "$SETUP_TIMEOUT" "${ES[@]}" "$@" >>"$S/setup.log" 2>&1 || fail "setup step failed: ${*##*/}"; }
run "$DECEBAL_BIN/decthemectl" theme install "$THEME" --force
run "$DECEBAL_BIN/decthemectl" theme apply deepin --no-rdwm
run "$DECEBAL_BIN/decthemectl" set mode "$MODE"
# the user's own choice of an icon pack staged in ICON_ROOT (the theme never selects an icon theme); Deepin uses `bloom` in both modes,
# ICON_THEME=nirvana picks the converted default theme (tools/build_nirvana.py)
[ -d "$ICON_ROOT/${ICON_THEME:-bloom}" ] || fail "no ${ICON_THEME:-bloom} icon theme in ICON_ROOT"
run "$DECEBAL_BIN/decthemectl" icons set "${ICON_THEME:-bloom}"
run "$DECEBAL_BIN/decthemectl" panel dock "$EDGE"
if [ "$ALL" = all-applets ]; then
  for a in launcher app_menu workspaces window_title tasks layout media clock weather tray clipboard cpu memory notifications stay_awake agents \
           lock_keys volume network bluetooth battery brightness search dashboard control_center power show_desktop; do
    run "$DECEBAL_BIN/decthemectl" panel applet "$a" on
  done
fi
grep -q "\"edge\": \"$EDGE\"" "$S/cfg/dock-settings.json" || fail "dock edge was not written"

if [ -n "${BACKDROP:-}" ]; then
  case $BACKDROP in '#'[0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F]) ;; *) fail "BACKDROP must be #rrggbb";; esac
  # a root pixmap (what a compositor reads), not just a root colour
  command -v feh >/dev/null || fail "feh needed for BACKDROP"
  magick -size 64x64 "xc:$BACKDROP" "$S/backdrop.png" || fail "cannot make the backdrop"
  timeout 10 "${E[@]}" feh --no-fehbg --bg-scale "$S/backdrop.png" || fail "feh failed"
fi
if [ "${COMPOSITOR:-0}" = 1 ]; then
  command -v picom >/dev/null || fail "picom needed for COMPOSITOR=1"
  timeout --foreground "$CAPTURE_TIMEOUT" "${E[@]}" picom --config /dev/null --backend xrender --no-vsync </dev/null >"$S/picom.log" 2>&1 &
  CP=$!
  sleep 1
  kill -0 "$CP" 2>/dev/null || fail "picom exited: $(tail -2 "$S/picom.log" | tr '\n' ' ')"
fi
# Setup's reconcile may still have spawned a dock (it waits for its display). It holds our private lock, so it is identifiable
# exactly: stop that one process (TERM, bounded wait, KILL) before starting the dock this capture is about.
stray=
for _ in $(seq 15); do
  P=$(cat "$S/run/$PROGRAM.lock" 2>/dev/null | tr -dc '0-9')
  if [ -n "$P" ] && kill -0 "$P" 2>/dev/null && [ "$(basename "$(readlink "/proc/$P/exe" 2>/dev/null)")" = "$PROGRAM" ]; then stray=$P; break; fi
  sleep 0.1
done
if [ -n "$stray" ]; then
  st=$(starttime "$stray")
  kill -TERM "$stray" 2>/dev/null
  for _ in $(seq 20); do kill -0 "$stray" 2>/dev/null && [ "$(starttime "$stray")" = "$st" ] || break; sleep 0.15; done
  if kill -0 "$stray" 2>/dev/null && [ "$(starttime "$stray")" = "$st" ]; then kill -KILL "$stray" 2>/dev/null; sleep 0.3; fi
  kill -0 "$stray" 2>/dev/null && [ "$(starttime "$stray")" = "$st" ] && fail "could not stop the dock setup spawned (pid $stray)"
fi
timeout --foreground "$CAPTURE_TIMEOUT" "${E[@]}" "$DECEBAL_BIN/$PROGRAM" </dev/null >"$S/dock.log" 2>&1 &
DP=$!
sleep "${CAPTURE_WAIT:-3}"
if ! kill -0 "$DP" 2>/dev/null; then wait "$DP"; rc=$?; fail "$PROGRAM exited early (status $rc): $(tail -3 "$S/dock.log" | tr '\n' ' ')"; fi
kill -0 "$XP" 2>/dev/null || fail "Xvfb died"
# ---- optional input steps (the display was proven ours above; nothing else is ever addressed)
xd() { timeout 10 "${E[@]}" xdotool "$@"; }
dockwin() { xd search --name '^decebal-dock$' 2>/dev/null | head -1; }
zone() { # zone KEY [NAME] -> "x y" in root coordinates (dump positions are relative to the dock window)
  local w line gx gy
  w=$(dockwin); [ -n "$w" ] || fail "no dock window to address"
  gx=$(xd getwindowgeometry --shell "$w" | sed -n 's/^X=//p'); gy=$(xd getwindowgeometry --shell "$w" | sed -n 's/^Y=//p')
  if [ -n "${2:-}" ]; then line=$(awk -v k="$1" -v n="$2" '$1==k && $2==n {print $3, $4}' "$S/layout.txt"); else line=$(awk -v k="$1" '$1==k {print $2, $3}' "$S/layout.txt"); fi
  set -- $line
  [ -n "${1:-}" ] || fail "zone not in the layout dump"
  echo "$(( gx + ${1%.*} )) $(( gy + ${2%.*} ))"
}
if [ -n "${STEPS:-}" ]; then
  [ -s "$S/layout.txt" ] || fail "the dock wrote no layout dump (DECDOCK_DEBUG_LAYOUT)"
  OUTDIR=$(dirname "$OUT"); IFS=';' read -ra ST <<<"$STEPS"
  for st in "${ST[@]}"; do
    IFS=: read -r op a1 a2 a3 <<<"$st"
    case $op in
      hover) read -r X Y <<<"$(zone "$a1" "$a2")"; xd mousemove "$X" "$Y"; sleep 0.8;;
      click) read -r X Y <<<"$(zone "$a1" "$a2")"; xd mousemove "$X" "$Y"; sleep 0.2; xd click "${a3:-1}"; sleep 1.0;;
      key) xd key "$a1"; sleep 0.5;;
      focus) timeout 10 "${E[@]}" "$DECEBAL_BIN/decdock" --focus >>"$S/dock.log" 2>&1; sleep 0.6;;
      wait) sleep "$a1";;
      shot) timeout 10 import -display "$D" -window root "$OUTDIR/$a1" 2>>"$S/import.log" || fail "step screenshot $a1 failed";;
      *) fail "unknown step $op";;
    esac
    kill -0 "$XP" 2>/dev/null && kill -0 "$DP" 2>/dev/null || fail "server or dock died during step $st"
  done
fi
timeout 10 import -display "$D" -window root "$OUT.part.png" 2>"$S/import.log" || fail "import failed: $(head -2 "$S/import.log")"
[ "$(identify -format '%wx%h' "$OUT.part.png")" = "${W}x${H}" ] || fail "capture has the wrong size"
# a real dock paints many colours (icons, text, border); an empty screen has at most 2
[ "$(magick "$OUT.part.png" -format %k info:)" -ge 12 ] || fail "capture is blank: dock did not draw"
grep -qi "panic" "$S/dock.log" 2>/dev/null && fail "panic in $PROGRAM log"
mv -f "$OUT.part.png" "$OUT"
[ "${KEEP:-0}" = 1 ] && echo "work directory kept: $S"
echo "ok $OUT"
