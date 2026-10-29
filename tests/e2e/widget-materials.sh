#!/usr/bin/env bash
# Actual opacity/palette pixels on a private X11 desktop; never edits live appearance.
set -eu
export DECEBAL_SYSTEM_AUTOMATIC=0
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
if [ "${NO_BUILD:-0}" != 1 ]; then (cd "$HERE" && cargo build --bin decwidgets); fi
export MS_BIN_DIR="$HERE/target/debug"
OLDHOME=$HOME
source "$HERE/tests/e2e/env.sh"
N=${E2E_DISPLAY:-:248}
case "$N" in :0|:1) echo 'refusing live display'; exit 2;; esac
[ ! -e "/tmp/.X11-unix/X${N#:}" ] || { echo 'display occupied'; exit 2; }
trap 'e2e_down' EXIT
e2e_up "$N"
export DECEBAL_THEMES="$XDG_CONFIG_HOME/decebal/themes" DECEBAL_PALETTES="$XDG_CONFIG_HOME/decebal/palettes"
export DECEBAL_WIDGETS="$XDG_CONFIG_HOME/decebal/widgets" XDG_DATA_HOME="$E2E_T/data" XDG_STATE_HOME="$E2E_T/state"
mkdir -p "$DECEBAL_THEMES" "$DECEBAL_PALETTES" "$DECEBAL_WIDGETS"
cat > "$DECEBAL_THEMES/probe.toml" <<'TOML'
name="Probe"
[metrics]
popup_alpha=0.8
[dark]
background="#146432"
popup="#204060"
text="#ffffff"
accent="#ff2200"
[light]
background="#146432"
popup="#e0c0a0"
text="#111111"
accent="#0022ff"
TOML
cat > "$DECEBAL_PALETTES/probe-colors.toml" <<'TOML'
name="Probe colours"
[dark]
popup="#604020"
[light]
popup="#a0c0e0"
TOML
cat > "$DECEBAL_WIDGETS/material-probe.toml" <<'TOML'
format=1
id="material-probe"
size=[160,80]
[root]
type="text"
text="Material probe"
size=14
background="popup"
color="text"
TOML
magick -size 1600x1000 xc:'#146432' "$E2E_T/wall.png"
rdwmctl wallpaper "$E2E_T/wall.png" --mode fill >/dev/null
CFG="$DECEBAL_CONFIG/decebal.json"
write_config() {
  python3 - "$CFG" "$1" "$2" "$3" "$4" <<'PY'
import json,sys,os
path,mode,opacity,background,palette=sys.argv[1:]
theme=dict(name='probe',mode=mode,opacity=float(opacity),sync_rdwm=False)
if palette!='none':theme['palette']=palette
config=dict(theme=theme,widgets=dict(items=[dict(use='material-probe',anchor='top-left',x=80,y=140,options=dict(background_opacity=float(background)))]))
with open(path+'.new','w') as f:json.dump(config,f)
os.replace(path+'.new',path)
PY
}
check_pixel() {
  local label=$1 color=$2 alpha=$3
  for _ in $(seq 1 80); do
    local actual
    actual=$(import -window root -crop 1x1+200+200 +repage -depth 8 txt:- 2>/dev/null | tail -1)
    if python3 - "$actual" "$color" "$alpha" 2>/dev/null <<'PY'
import sys,re
m=re.search(r'#([0-9A-Fa-f]{6})',sys.argv[1]);assert m
actual=tuple(int(m[1][i:i+2],16) for i in (0,2,4))
fg=tuple(int(sys.argv[2][i:i+2],16) for i in (0,2,4))
a=float(sys.argv[3]);bg=(20,100,50)
assert all(abs(got-(f*a+b*(1-a)))<=3 for got,f,b in zip(actual,fg,bg)),(actual,fg,a)
PY
    then echo "  ok   $label"; return; fi
    sleep .1
  done
  echo "FAIL $label"; cat "$E2E_T/widgets.log"; exit 1
}
write_config dark 1 0.5 none
decwidgets daemon >"$E2E_T/widgets.log" 2>&1 & E2E_PIDS+=($!)
check_pixel 'theme .8 times instance .5 blends over real wallpaper' 204060 .4
write_config dark .5 .5 none
check_pixel 'theme opacity updates existing widget pixels' 204060 .2
write_config dark .5 .5 probe-colors
check_pixel 'independent colour scheme updates existing widget pixels' 604020 .2
write_config light .5 .5 probe-colors
check_pixel 'light variant uses independent scheme and retained opacity' a0c0e0 .2
write_config light .5 0 probe-colors
check_pixel 'zero background opacity exposes wallpaper without fill' a0c0e0 0
if [ -n "${WIDGET_EVIDENCE_DIR:-}" ]; then
  decwidgets render fetch-summary --live --png "$WIDGET_EVIDENCE_DIR/fetch-summary-fastfetch-cli-live.png"
fi
echo 'passed 5, failed 0'
