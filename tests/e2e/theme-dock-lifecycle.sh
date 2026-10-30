#!/usr/bin/env bash
# Theme transitions must own the actual dock process, including stale supervisor suppression.
set -eu
export DECEBAL_SYSTEM_AUTOMATIC=0
HERE=$(cd "$(dirname "$0")/../.." && pwd)
OLDHOME=$HOME; export OLDHOME
export MS_BIN_DIR=${MS_BIN_DIR:-$HERE/target/release}
source "$HERE/tests/e2e/env.sh"
N=${E2E_DISPLAY:-:284}
case "$N" in :0|:1) exit 2;; esac
if [ -e "/tmp/.X11-unix/X${N#:}" ] || [ -e "/tmp/.X${N#:}-lock" ]; then
    echo "Display $N is already in use; choose another E2E_DISPLAY" >&2
    exit 77
fi
cleanup() { if [ -e "$XDG_RUNTIME_DIR/decdock.lock" ]; then kill "$(cat "$XDG_RUNTIME_DIR/decdock.lock")" 2>/dev/null || true; fi; e2e_down; }
trap cleanup EXIT
e2e_up "$N"
mkdir -p "$XDG_RUNTIME_DIR/rdwm-decebal"
: > "$XDG_RUNTIME_DIR/rdwm-decebal/decdock.off"
count() {
 python3 - <<'PY'
import os,pathlib
n=0
for p in pathlib.Path('/proc').iterdir():
 try:
  if (p/'comm').read_text().strip()=='decdock' and ('DISPLAY='+os.environ['DISPLAY']).encode() in (p/'environ').read_bytes().split(b'\0'): n+=1
 except (OSError,ValueError): pass
print(n)
PY
}
PASS=0
check_count() { for i in $(seq 1 60); do if [ "$(count)" = "$1" ]; then echo "ok $2"; PASS=$((PASS+1)); return; fi; sleep .1; done; echo "FAIL $2 ($(count) docks)"; exit 1; }
decthemectl theme apply macos
check_count 1 'macOS starts one dock despite stale off marker'
[ ! -e "$XDG_RUNTIME_DIR/rdwm-decebal/decdock.off" ]
decw=$MS_BIN_DIR/decdock
"$decw" & duplicate=$!; wait "$duplicate"
check_count 1 'duplicate launch preserves the original dock'
decthemectl theme install "$HERE/themes/orbit-glass" --force
decthemectl theme apply orbit-glass
check_count 0 'Orbit Glass stops dock process'
[ -e "$XDG_RUNTIME_DIR/rdwm-decebal/decdock.off" ]
"$decw"
check_count 0 'disabled dock exits without retaining a hidden process'
decthemectl theme apply macos
check_count 1 'switching back restarts the macOS dock'
decthemectl theme apply caelestia
check_count 0 'auto dock is absent for vertical Caelestia rail'
decthemectl theme apply windows11
check_count 1 'Windows taskbar starts one dock'
decthemectl theme install "$HERE/themes/tidal-loom" --force
decthemectl theme apply tidal-loom
check_count 0 'Tidal Loom keeps only its left rail, with no dock process'
jq -e '.bar.enabled==true and .bar.position=="left" and .dock.enabled==false' "$DECEBAL_CONFIG/decebal.json" >/dev/null

decthemectl theme apply macos
check_count 1 'Tidal Loom to macOS restores a single dock'
echo "passed $PASS, failed 0"
