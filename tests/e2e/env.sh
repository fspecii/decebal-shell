#!/usr/bin/env bash
# Private test desktop for macsettings: Xvfb + rdwm (x11 backend) with its own HOME, config and runtime dir.
# Never touches the real session: DISPLAY is a private Xvfb, WAYLAND_DISPLAY is unset, every path is a temp copy.
# usage: source env.sh; e2e_up [display]; ...; e2e_down
RDWM_BIN_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
MS_BIN_DIR=${MS_BIN_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/target/release}
E2E_PIDS=()

e2e_up() {
  E2E_DISPLAY=${1:-:95}
  E2E_T=$(mktemp -d /tmp/macsettings-e2e.XXXXXX)
  mkdir -p "$E2E_T/run" "$E2E_T/home" "$E2E_T/cfg/rdwm" "$E2E_T/cfg/omarchy/backgrounds"
  chmod 700 "$E2E_T/run"
  export HOME="$E2E_T/home" XDG_CONFIG_HOME="$E2E_T/cfg" XDG_RUNTIME_DIR="$E2E_T/run" MACSHELL_CONFIG="$E2E_T/cfg/omarchy"
  export DISPLAY="$E2E_DISPLAY" MACSHELL_BACKEND=x11 RDWMCTL="$RDWM_BIN_DIR/rdwmctl" MACSETTINGS_CONFIG="$E2E_T/cfg/rdwm/config.toml"
  unset WAYLAND_DISPLAY RDWM_SOCK
  export PATH="$RDWM_BIN_DIR:$MS_BIN_DIR:$PATH"
  E2E_CFG="$E2E_T/cfg/rdwm/config.toml"
  if [ -n "${E2E_CONFIG_SRC:-}" ]; then cp "$E2E_CONFIG_SRC" "$E2E_CFG"; else
    cat > "$E2E_CFG" <<'TOML'
# test config
[general]
border_px = 2
gap_inner = 6   # breathing room
smart_gaps = true
layouts = ["tile", "floating", "monocle", "grid", "bsp"]

[decoration]
enabled = true

[wallpaper]
mode = "fill"

[[bind]]
mods = ["super"]
key = "Return"
action = "spawn"
args = ["true"]

[[rule]]
class = "mpv"
floating = true
TOML
  fi
  for f in $(ls "${E2E_WALLS:-$OLDHOME/.config/omarchy/backgrounds}"/*/* 2>/dev/null | head -8); do cp "$f" "$E2E_T/cfg/omarchy/backgrounds/" 2>/dev/null; done
  first=$(ls "$E2E_T"/cfg/omarchy/backgrounds/* 2>/dev/null | head -1)
  [ -z "${E2E_CONFIG_SRC:-}" ] && [ -n "$first" ] && sed -i "s|^mode = \"fill\"|image = \"$first\"\nmode = \"fill\"|" "$E2E_CFG"
  Xvfb "$E2E_DISPLAY" -screen 0 "${E2E_SCREEN:-1600x1000x24}" -nolisten tcp >"$E2E_T/xvfb.log" 2>&1 &
  E2E_PIDS+=($!)
  sleep 1
  rdwm --backend x11 --config "$E2E_CFG" >"$E2E_T/rdwm.log" 2>&1 &
  E2E_RDWM=$!
  E2E_PIDS+=($E2E_RDWM)
  sleep 1
}

e2e_start_app() {
  macsettings --config "$E2E_CFG" "$@" >"$E2E_T/macsettings.log" 2>&1 &
  E2E_APP=$!
  E2E_PIDS+=($E2E_APP)
  sleep ${E2E_APP_WAIT:-3}
}

e2e_shot() { import -window root "$1"; }

e2e_down() {
  for p in "${E2E_PIDS[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null; done
  wait 2>/dev/null
  [ -n "${E2E_T:-}" ] && rm -rf "$E2E_T"
}
