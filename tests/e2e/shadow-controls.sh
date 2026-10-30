#!/usr/bin/env bash
# Writable controls depend on the actual live parser, including after upgrading the CLI.
set -eu
HERE=$(cd "$(dirname "$0")/../.." && pwd)
OLDHOME=$HOME; export OLDHOME
export MS_BIN_DIR=${MS_BIN_DIR:-$HERE/target/release}
source "$HERE/tests/e2e/env.sh"
N=${E2E_DISPLAY:-:321}
[[ "$N" =~ ^:([0-9]+)$ ]] && [ "$((10#${BASH_REMATCH[1]}))" -ge 2 ] || exit 2
[ ! -e "/tmp/.X11-unix/X${N#:}" ] && [ ! -e "/tmp/.X${N#:}-lock" ] || exit 77
ROOT_XDG=$(mktemp -d /tmp/decebal-shadow-controls.XXXXXX)
export XDG_CACHE_HOME="$ROOT_XDG/cache" XDG_STATE_HOME="$ROOT_XDG/state" XDG_DATA_HOME="$ROOT_XDG/data" XDG_DATA_DIRS="$ROOT_XDG/nodata"
export E2E_WALLS=/nonexistent DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent
unset WAYLAND_DISPLAY DECEBAL_FONT_SCALE DECEBAL_HIGH_CONTRAST DECEBAL_REDUCE_MOTION
cleanup() { set +e; e2e_down; rm -rf "$ROOT_XDG"; return 0; }
trap cleanup EXIT
e2e_up "$N"
export DECSETTINGS_DEBUG_LAYOUT="$E2E_T/layout.json" SHADOW_CTL_CFG="$E2E_CFG"
e2e_start_app windowstyle
export SETTINGS_WID=$(xdotool search --onlyvisible --class DecSettings | head -1)
python3 - <<'PY'
import json,os,pathlib,subprocess,time,tomllib
layout=pathlib.Path(os.environ['DECSETTINGS_DEBUG_LAYOUT']);cfg=pathlib.Path(os.environ['SHADOW_CTL_CFG']);wid=os.environ['SETTINGS_WID']
def state():return json.loads(layout.read_text())
def wait(fn,reason):
 for _ in range(150):
  try:
   if fn():return
  except (OSError,ValueError,KeyError,StopIteration):pass
  time.sleep(.1)
 raise AssertionError(reason)
def ctl(ident):return next(c for c in state()['ctls'] if c['id']==ident)
unsupported=os.environ.get('EXPECT_OLD_SHADOW_WM')=='1'
if unsupported:
 # Informational notes are deliberately absent from dump.warnings; verify the
 # correct native view and old live schema, then retain pixels for visual QA.
 wait(lambda:state()['pane']=='appearance' and state()['section']=='window' and any(c['id']=='r:decoration:enabled' for c in state()['ctls']),'Window style view missing')
 time.sleep(1.2)
 schema=json.loads(subprocess.check_output(['rdwmctl','schema'],text=True))
 assert not any(k['section']=='decoration' and k['key']=='shadow' for k in schema['keys'])
 assert not any(c['id'].startswith('shadow:') for c in state()['ctls'])
 assert 'shadow' not in tomllib.loads(cfg.read_text())['decoration']
 if os.environ.get('SHADOW_CONTROLS_EVIDENCE_DIR'):
  evidence=pathlib.Path(os.environ['SHADOW_CONTROLS_EVIDENCE_DIR']);evidence.mkdir(parents=True,exist_ok=True)
  for _ in range(25):
   geometry=dict(line.split('=',1) for line in subprocess.check_output(['xdotool','getwindowgeometry','--shell',wid],text=True).splitlines())
   height=int(geometry['HEIGHT']);width=int(geometry['WIDTH'])
   border=ctl('r:general:border_px')['rect']
   if border[1]+border[3]<height-20:break
   subprocess.run(['xdotool','mousemove','--window',wid,str(width-35),str(height//2),'click','5'],check=True);time.sleep(.15)
  subprocess.run(['import','-window',wid,str(evidence/'old-wm-controls.png')],check=True,timeout=5)
 print('PASS native upgraded Settings/CLI with old live WM exposes no unsupported shadow writes')
else:
 wait(lambda:any(c['id']=='shadow:shadow' for c in state()['ctls']),'supporting session controls missing')
 keys=['shadow_radius','shadow_opacity','shadow_inactive_opacity']
 assert all(not ctl('shadow:'+k)['enabled'] for k in keys)
 def click(ident):
  for _ in range(50):
   x,y,w,h=ctl(ident)['rect']
   geometry=dict(line.split('=',1) for line in subprocess.check_output(['xdotool','getwindowgeometry','--shell',wid],text=True).splitlines())
   height=int(geometry['HEIGHT']);width=int(geometry['WIDTH'])
   if y>=95 and y+h<height-12:
    subprocess.run(['xdotool','mousemove','--window',wid,str(int(x+w/2)),str(int(y+h/2)),'click','1'],check=True);return
   subprocess.run(['xdotool','mousemove','--window',wid,str(width-35),str(height//2),'click','5' if y+h>=height-12 else '4'],check=True);time.sleep(.15)
  raise AssertionError('shadow control unreachable')
 click('shadow:shadow')
 wait(lambda:tomllib.loads(cfg.read_text())['decoration'].get('shadow') is True,'toggle not saved')
 wait(lambda:all(ctl('shadow:'+k)['enabled'] for k in keys),'sliders not enabled')
 click('shadow:shadow')
 wait(lambda:tomllib.loads(cfg.read_text())['decoration'].get('shadow') is False,'disable not saved')
 wait(lambda:all(not ctl('shadow:'+k)['enabled'] for k in keys),'disabled sliders remain active')
 assert subprocess.run(['rdwmctl','ping'],capture_output=True).returncode==0
 print('PASS native new WM shadow toggle/save/reload and dependent slider state')
PY
