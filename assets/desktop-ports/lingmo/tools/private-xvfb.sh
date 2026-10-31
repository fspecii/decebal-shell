#!/bin/sh
# Sourced helper: start ONE private Xvfb on an explicitly reserved high display and prove ownership. Never run directly.
#   . private-xvfb.sh; start_private_xvfb DISPLAY_NUM WxH LOG_FILE      -> sets XPID, exports DISPLAY=:N
#   stop_private_xvfb                                                   -> stops only XPID and waits for it
# Rules (after the rollout X11 transport incident): N >= 700 (never :0, never auto-probed with -displayfd); refuses a number whose filesystem
# socket, lock file or abstract socket already exists; after the start the process must be the server for exactly :N and hold the inode of BOTH
# socket namespaces (/tmp/.X11-unix/X<N> and @/tmp/.X11-unix/X<N>). Nothing is ever unlinked. Fails with exit 2 (refused) or 3 (not proven).
start_private_xvfb() {
  _N=$1; _GEO=$2; _LOG=$3
  case "$_N" in ''|*[!0-9]*) echo "DISPLAY_NUM must be a number, got: $_N" >&2; return 2 ;; esac
  [ "$_N" -ge 700 ] || { echo "DISPLAY_NUM must be a reserved high number (>= 700), got $_N; never use :0 or a low number" >&2; return 2; }
  _SOCK="/tmp/.X11-unix/X$_N"; _LOCK="/tmp/.X$_N-lock"
  if [ -e "$_SOCK" ] || [ -L "$_SOCK" ] || [ -e "$_LOCK" ] || grep -q "@/tmp/.X11-unix/X$_N\$" /proc/net/unix; then
    echo "display :$_N is already in use (filesystem socket, lock or abstract socket exists); pick another reserved number, nothing was touched" >&2; return 2
  fi
  Xvfb ":$_N" -screen 0 "${_GEO}x24" -nolisten tcp >"$_LOG" 2>&1 &
  XPID=$!
  sleep 2
  kill -0 "$XPID" 2>/dev/null || { echo "Xvfb :$_N did not start; see $_LOG" >&2; return 3; }
  tr '\0' ' ' < "/proc/$XPID/cmdline" | grep -q "^Xvfb :$_N " || { echo "process $XPID is not the Xvfb for :$_N" >&2; return 3; }
  [ -S "$_SOCK" ] || { echo "filesystem socket $_SOCK is missing after start" >&2; return 3; }
  _FS=$(awk -v p="$_SOCK" '$NF==p {print $7}' /proc/net/unix | head -n 1)
  _AB=$(awk -v p="@/tmp/.X11-unix/X$_N" '$NF==p {print $7}' /proc/net/unix | head -n 1)
  [ -n "$_FS" ] && [ -n "$_AB" ] || { echo "both socket namespaces are required for :$_N (filesystem=$_FS abstract=$_AB)" >&2; return 3; }
  for _ino in "$_FS" "$_AB"; do
    ls -l "/proc/$XPID/fd" 2>/dev/null | grep -q "socket:\\[$_ino\\]" || { echo "socket inode $_ino of :$_N is not held by our Xvfb $XPID" >&2; return 3; }
  done
  DISPLAY=":$_N"; export DISPLAY
}
stop_private_xvfb() {
  [ -z "${XPID:-}" ] || { kill "$XPID" 2>/dev/null || true; wait "$XPID" 2>/dev/null || true; }
}
