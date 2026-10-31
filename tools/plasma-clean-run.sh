#!/bin/sh
# Runs a heavy cargo build/test in slot 2 with a cleaned environment (no host display, bus, session or RDWM_* variables).
# usage: CARGO_TARGET_DIR=<private dir> tools/plasma-clean-run.sh cargo test -j2 --offline ...
# - `flock --close`: children of unit tests (xclip and friends) must not inherit the lock fd.
# - env -i: DISPLAY, WAYLAND_DISPLAY, XAUTHORITY, RDWM_*, DBUS_SESSION_BUS_ADDRESS and session variables are not passed on.
# - private HOME / XDG_* / bus addresses and a PATH whose first entry holds do-nothing fake commands.
# Native-test rule (root, 2026-10-31): none of these tools starts an X server. A future private Xvfb fixture must use an explicitly reserved
# display number >= 700 (never auto-probe from :0 with a bare -displayfd), verify the owned server and BOTH socket namespaces (the
# /tmp/.X11-unix/X<n> path and the abstract @/tmp/.X11-unix/X<n>) before any input, and never unlink a foreign X socket or lock.
set -eu
REAL_HOME=$HOME
P=${PLASMA_PRIVATE:-$HOME/.cache/decebal-plasma-ports/private}
mkdir -p "$P/home/.config" "$P/home/.cache" "$P/home/.local/share" "$P/run" "$P/fakebin"
chmod 700 "$P/run"
for c in xclip xsel wl-copy wl-paste notify-send pactl wpctl nmcli bluetoothctl brightnessctl playerctl; do
  [ -x "$P/fakebin/$c" ] || { printf '#!/bin/sh\nexit 0\n' > "$P/fakebin/$c"; chmod +x "$P/fakebin/$c"; }
done
mkdir -p "$HOME/.cache/decebal-build-slots"
exec flock --close "$HOME/.cache/decebal-build-slots/slot2.lock" env -i \
  HOME="$P/home" XDG_CONFIG_HOME="$P/home/.config" XDG_CACHE_HOME="$P/home/.cache" XDG_DATA_HOME="$P/home/.local/share" \
  XDG_RUNTIME_DIR="$P/run" XDG_DATA_DIRS="/usr/share" XDG_CONFIG_DIRS="/etc/xdg" \
  DBUS_SESSION_BUS_ADDRESS="unix:path=$P/run/no-session-bus" DBUS_SYSTEM_BUS_ADDRESS="unix:path=$P/run/no-system-bus" \
  PATH="$P/fakebin:/usr/local/bin:/usr/bin:/bin" LANG=C.UTF-8 TERM=dumb \
  PLASMA_REFS="${PLASMA_REFS:-}" \
  CARGO_HOME="${CARGO_HOME:-$REAL_HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$REAL_HOME/.rustup}" CARGO_TARGET_DIR="${CARGO_TARGET_DIR:?set a private CARGO_TARGET_DIR}" \
  nice -n10 "$@"
