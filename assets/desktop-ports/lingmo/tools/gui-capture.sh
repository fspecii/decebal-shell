#!/bin/sh
# Real-GUI capture of the Lingmo package: private Xvfb (no window manager), private HOME/XDG/config/D-Bus, real decbar + decdock.
#
# usage: [EXTRA="declaunch ARGS"] [ICON_PACK=DIR] [COMPOSITOR=1] [EDGE=..] [DOCK=..] [WINDOWS=1] \
#        gui-capture.sh BIN_DIR WORK_DIR DISPLAY_NUM MODE SCALE WxH BACKDROP OUT.png
#   BIN_DIR      holds decthemectl, decbar, decdock of one pinned build
#   WORK_DIR     must not exist; it is created private (nothing is ever deleted)
#   DISPLAY_NUM  an explicitly reserved HIGH display number, 700 or more. The script refuses anything lower (never :0), refuses a number whose
#                filesystem socket, lock file or abstract socket already exists, and after starting Xvfb proves that its own process owns both
#                socket namespaces before any client runs. It never unlinks a socket or lock and never probes with -displayfd.
#   SCALE        sets DECEBAL_SCALE and Xft.dpi (96*SCALE)
#   BACKDROP     a local image or color:#rrggbb, painted beneath the bars (a private cache original, never shipped)
#   COMPOSITOR=1 runs a private picom (XRender backend, Gaussian blur of the repository's scoped _DECEBAL_BLUR rule, no GLX: GLX paints some
#                clients black on Xvfb) with its own config; without it nothing composites and bar_alpha < 1 is drawn over black
#   EDGE / DOCK  set the user's bar and dock edges through decthemectl panel, as Settings does (private config only)
#   ICON_PACK    a directory holding an installed Lingmo icon theme, selected like a user would, in the private config
#   EXTRA        one more engine program to start (for example declaunch); WINDOWS=1 writes the visible window list to WORK_DIR/windows.txt
# Nothing live is touched: no config, input device, install or restart outside WORK_DIR and the private X display.
set -eu
BIN=$1; W=$2; N=$3; MODE=$4; SCALE=$5; GEO=$6; BG=$7; OUT=$8
case "$N" in ''|*[!0-9]*) echo "DISPLAY_NUM must be a number, got: $N" >&2; exit 2 ;; esac
[ "$N" -ge 700 ] || { echo "DISPLAY_NUM must be a reserved high number (>= 700), got $N; never use :0 or a low number" >&2; exit 2; }
X_SOCK="/tmp/.X11-unix/X$N"; X_LOCK="/tmp/.X$N-lock"
if [ -e "$X_SOCK" ] || [ -L "$X_SOCK" ] || [ -e "$X_LOCK" ] || grep -q "@/tmp/.X11-unix/X$N\$" /proc/net/unix; then
  echo "display :$N is already in use (filesystem socket, lock or abstract socket exists); pick another reserved number, nothing was touched" >&2; exit 2
fi
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd)
[ ! -e "$W" ] || { echo "refusing to reuse existing work directory: $W" >&2; exit 2; }   # fresh directories only, nothing is deleted
mkdir -p "$W/home" "$W/cfg" "$W/tmp"
DPI=$(awk -v s="$SCALE" 'BEGIN{printf "%d", 96*s}')
E="HOME=$W/home XDG_CONFIG_HOME=$W/home/.config XDG_DATA_HOME=$W/home/.local/share XDG_CACHE_HOME=$W/home/.cache DECEBAL_CONFIG=$W/cfg DECEBAL_THEMES=$ROOT/themes TMPDIR=$W/tmp DISPLAY=:$N DECEBAL_SCALE=$SCALE"
env -i PATH="$PATH" $E "$BIN/decthemectl" theme apply lingmo --no-rdwm >/dev/null
# the mode is part of the private config, set by the engine's own switch
env -i PATH="$PATH" $E "$BIN/decthemectl" set mode "$MODE" >/dev/null
if [ -n "${ICON_PACK:-}" ]; then   # a directory holding an installed Lingmo icon theme; selected like a user would, in the private config
  mkdir -p "$W/home/.local/share/icons"; ln -s "$ICON_PACK/Lingmo" "$W/home/.local/share/icons/Lingmo"
  env -i PATH="$PATH" $E "$BIN/decthemectl" icons set Lingmo >/dev/null
fi
[ -z "${EDGE:-}" ] || env -i PATH="$PATH" $E "$BIN/decthemectl" panel edge "$EDGE" >/dev/null
[ -z "${DOCK:-}" ] || env -i PATH="$PATH" $E "$BIN/decthemectl" panel dock "$DOCK" >/dev/null
Xvfb ":$N" -screen 0 "${GEO}x24" -nolisten tcp >"$W/xvfb.log" 2>&1 &
XPID=$!
trap 'kill $XPID 2>/dev/null || true; wait $XPID 2>/dev/null || true' EXIT
sleep 2
# prove ownership before any client or input: our process is alive and is the server for exactly this display, and it holds the inode of both
# the filesystem socket and the abstract socket of :N
kill -0 "$XPID" 2>/dev/null || { echo "Xvfb :$N did not start; see $W/xvfb.log" >&2; exit 3; }
tr '\0' ' ' < "/proc/$XPID/cmdline" | grep -q "^Xvfb :$N " || { echo "process $XPID is not the Xvfb for :$N" >&2; exit 3; }
[ -S "$X_SOCK" ] || { echo "filesystem socket $X_SOCK is missing after start" >&2; exit 3; }
FS_INODE=$(awk -v p="$X_SOCK" '$NF==p {print $7}' /proc/net/unix | head -n 1)
AB_INODE=$(awk -v p="@/tmp/.X11-unix/X$N" '$NF==p {print $7}' /proc/net/unix | head -n 1)
[ -n "$FS_INODE" ] && [ -n "$AB_INODE" ] || { echo "both socket namespaces are required for :$N (filesystem=$FS_INODE abstract=$AB_INODE)" >&2; exit 3; }
for ino in "$FS_INODE" "$AB_INODE"; do
  ls -l "/proc/$XPID/fd" 2>/dev/null | grep -q "socket:\[$ino\]" || { echo "socket inode $ino of :$N is not held by our Xvfb $XPID" >&2; exit 3; }
done
export DISPLAY=":$N"
echo "Xft.dpi: $DPI" | xrdb -merge
W_H=${GEO%x*}x$(echo "$GEO" | cut -d x -f2)
case "$BG" in
  color:*) magick -size "$W_H" "xc:${BG#color:}" "$W/bg.png" ;;   # a flat test backdrop, e.g. color:#1b3a5c (not a wallpaper)
  *) magick "$BG" -resize "${W_H}^" -gravity center -extent "$W_H" "$W/bg.png" ;;
esac
display -window root "$W/bg.png" || true   # paints the root pixels directly; feh leaves this Xvfb black
if [ -n "${COMPOSITOR:-}" ]; then
  cat > "$W/picom.conf" <<'CONF'
backend = "xrender";
use-damage = false;
vsync = false;
shadow = false;
fading = false;
blur: { method = "gaussian"; size = 12; deviation = 5.0; };
rules = ( { blur-background = false; }, { match = "_DECEBAL_BLUR@ = 1"; blur-background = true; } );
CONF
  env -i PATH="$PATH" $E picom --config "$W/picom.conf" >"$W/picom.log" 2>&1 &
  PPID_=$!
  trap 'kill $PPID_ 2>/dev/null || true; kill $XPID 2>/dev/null || true; wait $XPID 2>/dev/null || true' EXIT
  sleep 1
  # a compositor repaints the root, so the backdrop must be a real window beneath the bars
  env -i PATH="$PATH" $E feh -x -Z -g "$((${GEO%x*}-2))x$((${GEO#*x}-2))+1+1" --no-menus "$W/bg.png" >"$W/feh.log" 2>&1 &
  FPID=$!
  trap 'kill $FPID $PPID_ 2>/dev/null || true; kill $XPID 2>/dev/null || true; wait $XPID 2>/dev/null || true' EXIT
  sleep 2
fi
SETTLE=${SETTLE:-10}; [ -z "${COMPOSITOR:-}" ] || SETTLE=${SETTLE_COMPOSITED:-20}
env -i PATH="$PATH" $E SETTLE=$SETTLE dbus-run-session -- sh -c "$BIN/decbar >$W/decbar.log 2>&1 & $BIN/decdock >$W/decdock.log 2>&1 & sleep 3; [ -z \"${EXTRA:-}\" ] || $BIN/${EXTRA:-true} >$W/extra.log 2>&1 & sleep $SETTLE; xdotool search --name \"^feh\" windowlower %@ 2>/dev/null; sleep 2; import -window root $OUT; [ -z \"${WINDOWS:-}\" ] || for w in \$(xdotool search --onlyvisible --name \"\"); do xdotool getwindowname \$w; xdotool getwindowgeometry \$w; done > $W/windows.txt 2>&1; kill %1 %2 %3 2>/dev/null; true"
