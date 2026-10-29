#!/usr/bin/env bash
# A normal Settings client must repaint after WM geometry changes without pointer/key input.
# Uses only a private Xvfb, HOME, rdwm and optional compositor (WITH_COMPOSITOR=1).
# This checks native geometry presentation; no application bug fix is implied.
set -eu
HERE=$(cd "$(dirname "$0")/../.." && pwd)
for tool in Xvfb xdotool import python3; do command -v "$tool" >/dev/null || { echo "SKIP: $tool missing"; exit 0; }; done
python3 -c 'from PIL import Image' 2>/dev/null || { echo 'SKIP: Pillow missing'; exit 0; }
export OLDHOME=${HOME}
source "$HERE/tests/e2e/env.sh"
SHOTS=${SHOTS:-/tmp/decebal-settings-repaint-shots}
mkdir -p "$SHOTS"
trap e2e_down EXIT
e2e_up "${DISPLAY_NUMBER:-:237}"
if [ "${WITH_COMPOSITOR:-0}" = 1 ] && command -v picom >/dev/null; then
  printf 'backend="glx";\nvsync=false;\nshadow=false;\n' > "$E2E_T/picom.conf"
  cat "$HERE/contrib/picom/decebal-glass.conf" >> "$E2E_T/picom.conf"
  picom --config "$E2E_T/picom.conf" > "$E2E_T/picom.log" 2>&1 & E2E_PIDS+=("$!")
  sleep 0.6
fi
export DECSETTINGS_DEBUG_LAYOUT="$E2E_T/layout.json"
"${SETTINGS_BIN:-$MS_BIN_DIR/decsettings}" --config "$E2E_CFG" "${SETTINGS_PANE:-theme}" > "$E2E_T/decsettings.log" 2>&1 & E2E_PIDS+=("$!")
sleep 2
W=$(xdotool search --class DecSettings | head -1)
assert_painted() {
  local name=$1
  import -window "$W" "$SHOTS/$name.png"
  local geo width height x y
  geo=$(xdotool getwindowgeometry --shell "$W")
  width=$(awk -F= '$1=="WIDTH" {print $2}' <<< "$geo")
  height=$(awk -F= '$1=="HEIGHT" {print $2}' <<< "$geo")
  x=$(awk -F= '$1=="X" {print $2}' <<< "$geo")
  y=$(awk -F= '$1=="Y" {print $2}' <<< "$geo")
  import -window root -crop "${width}x${height}+${x}+${y}" +repage "$SHOTS/$name-composited.png"
  python3 - "$SHOTS/$name.png" "$SHOTS/$name-composited.png" <<'PY'
from PIL import Image, ImageStat
import sys
for path in sys.argv[1:]:
    im=Image.open(path).convert('RGB')
    assert max(ImageStat.Stat(im).stddev)>5, f'blank Settings surface: {path}'
    assert len(im.getcolors(im.width*im.height) or [])>100, f'missing UI detail: {path}'
PY
}
assert_painted initial
xdotool windowactivate --sync "$W"
rdwmctl action togglefloating >/dev/null
sleep 0.4
for geometry in '940 680 120 130' '760 520 300 180' '760 520 390 240' '480 360 200 200' '940 680 120 130'; do
  read -r width height x y <<< "$geometry"
  xdotool windowsize "$W" "$width" "$height" windowmove "$W" "$x" "$y"
  sleep 0.6
  assert_painted "geometry-$width-$height-$x-$y"
done
# Repeat identical geometry requests: they may clear backing pixels without changing size.
for n in 1 2 3; do
  xdotool windowsize "$W" 940 680 windowmove "$W" 120 130
  sleep 0.4
  assert_painted "repeat-$n"
done
echo "PASS: native Settings repaints after float, resize, move and repeated geometry without synthetic input ($SHOTS)"
