#!/usr/bin/env bash
# Real Settings restart: visible native previews and saved references must not be regenerated.
set -eu
HERE=$(cd "$(dirname "$0")/../.." && pwd)
OLDHOME=$HOME; export OLDHOME
export MS_BIN_DIR=${MS_BIN_DIR:-$HERE/target/release}
source "$HERE/tests/e2e/env.sh"
N=${E2E_DISPLAY:-:291}
case "$N" in :0|:1) exit 2;; esac
if [ -e "/tmp/.X11-unix/X${N#:}" ] || [ -e "/tmp/.X${N#:}-lock" ]; then
    echo "Display $N is already in use; choose another E2E_DISPLAY" >&2
    exit 77
fi
cleanup() { set +e; e2e_down; return 0; }
trap cleanup EXIT
e2e_up "$N"
export XDG_CACHE_HOME="$E2E_T/cache" XDG_STATE_HOME="$E2E_T/state" XDG_DATA_HOME="$E2E_T/data"
export XDG_DATA_DIRS="$E2E_T/nodata" DECEBAL_SYSTEM_THEMES="$E2E_T/nosystem"
export DECSETTINGS_DEBUG_LAYOUT="$E2E_T/layout.json"
mkdir -p "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME"
decthemectl theme install "$HERE/themes/tidal-loom" --force >"$E2E_T/install.log"
printf '%s\n' '{"theme":{"name":"tidal-loom","mode":"dark"}}' > "$DECEBAL_CONFIG/decebal.json"
e2e_start_app theme
python3 - <<'PY'
import os,pathlib,time,json
cache=pathlib.Path(os.environ['XDG_CACHE_HOME'])/'decebal/theme-previews'
previous=None;stable=0
for _ in range(200):
 images=sorted((p.name,p.stat().st_mtime_ns) for p in cache.glob('*.png'))
 busy=any(cache.glob('.preview-*'))
 if images and images==previous and not busy:stable+=1
 else:stable=0
 if stable>=10:break
 previous=images;time.sleep(.2)
else:raise AssertionError('initial visible previews never settled')
assert list((cache/'saved').glob('*.json')),'no persistent preview references'
print('Cold gallery stored',len(images),'native previews')
PY
kill "$E2E_APP"; wait "$E2E_APP" 2>/dev/null || true
python3 - <<'PY'
import os,pathlib,json
cache=pathlib.Path(os.environ['XDG_CACHE_HOME'])/'decebal/theme-previews'
files=list(cache.glob('*.png'))+list((cache/'saved').glob('*.json'))
snapshot={str(p.relative_to(cache)):[p.stat().st_mtime_ns,p.stat().st_size,p.stat().st_ino] for p in files}
pathlib.Path(os.environ['DECSETTINGS_DEBUG_LAYOUT']+'.snapshot').write_text(json.dumps(snapshot))
PY
e2e_start_app theme
sleep 2
python3 - <<'PY'
import os,pathlib,json
cache=pathlib.Path(os.environ['XDG_CACHE_HOME'])/'decebal/theme-previews'
before=json.loads(pathlib.Path(os.environ['DECSETTINGS_DEBUG_LAYOUT']+'.snapshot').read_text())
files=list(cache.glob('*.png'))+list((cache/'saved').glob('*.json'))
after={str(p.relative_to(cache)):[p.stat().st_mtime_ns,p.stat().st_size,p.stat().st_ino] for p in files}
assert after==before,'restart generated or rewrote already stored visible previews'
assert not any(cache.glob('.preview-*')),'a native rendering job started after reopening'
print('PASS fresh Settings process reused stored gallery PNGs and references without rewriting them')
PY
