#!/usr/bin/env bash
# Real Super+Space -> Screenshot -> X11 region selection, saved PNG, clipboard and editor.
set -eu
HERE=$(cd "$(dirname "$0")/../.." && pwd)
OLDHOME=$HOME; export OLDHOME
export MS_BIN_DIR=${MS_BIN_DIR:-$HERE/target/release}
source "$HERE/tests/e2e/env.sh"
N=${E2E_DISPLAY:-:287}
case "$N" in :0|:1) exit 2;; esac
E2E_CONFIG_SRC=$(mktemp); export E2E_CONFIG_SRC
cat > "$E2E_CONFIG_SRC" <<'TOML'
[general]
border_px = 2
[[bind]]
mods = ["super"]
key = "space"
action = "spawn"
args = ["decspot"]
TOML
cleanup() { set +e; e2e_down; rm -f "$E2E_CONFIG_SRC"; return 0; }
trap cleanup EXIT
e2e_up "$N"
mkdir -p "$E2E_T/shims"
export CAPTURE_EDITOR_LOG="$E2E_T/editor.args" OMARCHY_SCREENSHOT_DIR="$E2E_T/Pictures with spaces"
cat > "$E2E_T/shims/tensaku" <<'EDITOR'
#!/bin/sh
printf '%s\n' "$@" > "$CAPTURE_EDITOR_LOG"
EDITOR
chmod +x "$E2E_T/shims/tensaku"
export PATH="$E2E_T/shims:$PATH"
# Restart only the private WM with the final test environment.
kill "$E2E_RDWM"; wait "$E2E_RDWM" 2>/dev/null || true
rdwm --backend x11 --config "$E2E_CFG" >"$E2E_T/rdwm.log" 2>&1 & E2E_RDWM=$!; E2E_PIDS+=($E2E_RDWM)
sleep .5
xdotool key super+space
sleep .5
xdotool type --clearmodifiers --delay 25 screenshot
sleep .3
xdotool key Return
sleep 1
xdotool mousemove 100 200
sleep .1
xdotool mousedown 1
sleep .2
xdotool mousemove 420 440
sleep .2
xdotool mouseup 1
for _ in $(seq 1 80); do [ -e "$CAPTURE_EDITOR_LOG" ] && break; sleep .1; done
python3 - <<'PY'
import os,pathlib,subprocess
from PIL import Image
p=pathlib.Path(os.environ['OMARCHY_SCREENSHOT_DIR']);images=list(p.glob('*.png'));assert len(images)==1,images
image=Image.open(images[0]);assert 300<=image.width<=330 and 220<=image.height<=250,image.size
args=pathlib.Path(os.environ['CAPTURE_EDITOR_LOG']).read_text();assert str(images[0]) in args and 'xclip -selection clipboard -t image/png -i' in args,args
copied=subprocess.check_output(['xclip','-selection','clipboard','-t','image/png','-o']);assert copied.startswith(b'\x89PNG'),copied[:20]
assert not subprocess.run(['xdotool','search','--name','^Spotlight$'],capture_output=True).stdout,'launcher remained open'
print('PASS real shortcut, popup teardown, X11 selection, PNG size, spaced path, clipboard and editor arguments')
PY
# Cancellation must leave the prior image intact and start no editor.
rm "$CAPTURE_EDITOR_LOG"
decutil screenshot >"$E2E_T/cancel.log" 2>&1 & capture=$!; E2E_PIDS+=($capture)
sleep .5
xdotool key Escape
wait "$capture"
[ ! -e "$CAPTURE_EDITOR_LOG" ]
[ "$(find "$OMARCHY_SCREENSHOT_DIR" -name '*.png' | wc -l)" = 1 ]
echo 'PASS cancellation creates no image or editor'
