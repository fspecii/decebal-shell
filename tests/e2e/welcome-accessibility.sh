#!/usr/bin/env bash
# Native onboarding, same-process deep links and 200% high-contrast text.
set -eu
HERE=$(cd "$(dirname "$0")/../.." && pwd)
OLDHOME=$HOME; export OLDHOME
export MS_BIN_DIR=${MS_BIN_DIR:-$HERE/target/release}
source "$HERE/tests/e2e/env.sh"
N=${E2E_DISPLAY:-:313}
case "$N" in :0|:1) exit 2;; esac
[ ! -e "/tmp/.X11-unix/X${N#:}" ] && [ ! -e "/tmp/.X${N#:}-lock" ] || exit 77
cleanup() {
 local result=$?
 if [ "$result" != 0 ] && [ -n "${READINESS_EVIDENCE_DIR:-}" ]; then
  mkdir -p "$READINESS_EVIDENCE_DIR"
  for name in rdwm decsettings xvfb; do [ ! -f "$E2E_T/$name.log" ] || cp "$E2E_T/$name.log" "$READINESS_EVIDENCE_DIR/$name.log"; done
 fi
 e2e_down
 return "$result"
}
trap cleanup EXIT
export E2E_WALLS=/nonexistent E2E_SCREEN=2200x1400x24
e2e_up "$N"
export XDG_CACHE_HOME="$E2E_T/cache" XDG_STATE_HOME="$E2E_T/state" XDG_DATA_HOME="$E2E_T/data"
export XDG_DATA_DIRS="$E2E_T/nodata" DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent
export DECSETTINGS_DEBUG_LAYOUT="$E2E_T/layout.json" DECEBAL_FONT_SCALE=2 DECEBAL_HIGH_CONTRAST=1 DECEBAL_REDUCE_MOTION=1
mkdir -p "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME"
printf '%s\n' '{"theme":{"name":"nord","mode":"dark"}}' > "$DECEBAL_CONFIG/decebal.json"
e2e_start_app welcome
export SETTINGS_PID=$E2E_APP
python3 - <<'PY'
import pathlib,os,json,subprocess,time
layout=pathlib.Path(os.environ['DECSETTINGS_DEBUG_LAYOUT']);pid=int(os.environ['SETTINGS_PID'])
def run(*args):return subprocess.check_output(args,text=True).strip()
def state():return json.loads(run('rdwmctl','state'))['state']
def wait(fn,reason):
 for _ in range(100):
  try:
   value=fn()
   if value:return value
  except (OSError,ValueError,KeyError,subprocess.CalledProcessError):pass
  time.sleep(.1)
 raise AssertionError(reason)
def page():return json.loads(layout.read_text())
w=wait(lambda:next((w['id'] for w in state()['windows'] if w['class']=='DecSettings'),None),'Settings not mapped')
wait(lambda:page()['pane']=='welcome','welcome did not open')
assert any(c['id']=='w:go:widgets' for c in page()['ctls'])
# A second launch navigates the same model/window, even from another workspace.
run('rdwmctl','action','view','2')
subprocess.run(['decsettings','icons'],check=True)
wait(lambda:page()['pane']=='icons','targeted icons request missing')
wait(lambda:state()['focused']==w,'targeted navigation did not reveal/focus existing window')
assert any(item['id']==w for item in state()['windows']) and pathlib.Path('/proc',str(pid)).exists()
subprocess.run(['decsettings','getting-started'],check=True)
wait(lambda:page()['pane']=='welcome','guide alias did not navigate')
# Explicit mismatched configs fail without changing/killing the running editor.
other=pathlib.Path(os.environ['XDG_CONFIG_HOME'])/'other.toml';other.write_text('[general]\n')
r=subprocess.run(['decsettings','--config',str(other),'general'],capture_output=True,timeout=8)
assert r.returncode!=0 and page()['pane']=='welcome' and pathlib.Path('/proc',str(pid)).exists()
run('rdwmctl','action','togglefullscreen');time.sleep(.3)
# Live native pixels are retained only when requested; no user data is shown.
if evidence:=os.environ.get('READINESS_EVIDENCE_DIR'):
 d=pathlib.Path(evidence);d.mkdir(parents=True,exist_ok=True)
 subprocess.run(['timeout','5','import','-window',str(w),str(d/'welcome-accessibility-200.png')],check=True)
 for pane in ['accessibility','widgets']:
  subprocess.run(['decsettings',pane],check=True);wait(lambda:page()['pane']==pane,'page switch failed');time.sleep(.2)
  subprocess.run(['timeout','5','import','-window',str(w),str(d/(pane+'-200.png'))],check=True)
print('PASS native Welcome/aliases, same PID/window, cross-workspace focus, config mismatch refusal, 200% high contrast')
PY
