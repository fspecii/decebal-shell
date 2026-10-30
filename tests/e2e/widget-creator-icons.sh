#!/usr/bin/env bash
# Native navigation, no-code creation and host rendering on a private desktop.
set -eu
HERE=$(cd "$(dirname "$0")/../.." && pwd)
OLDHOME=$HOME; export OLDHOME
export MS_BIN_DIR=${MS_BIN_DIR:-$HERE/target/release}
source "$HERE/tests/e2e/env.sh"
N=${E2E_DISPLAY:-:295}
case "$N" in :0|:1) exit 2;; esac
[ ! -e "/tmp/.X11-unix/X${N#:}" ] && [ ! -e "/tmp/.X${N#:}-lock" ] || exit 77
cleanup() {
 set +e
 [ ! -f "${XDG_RUNTIME_DIR:-/nonexistent}/decwidgets.lock" ] || kill "$(cat "$XDG_RUNTIME_DIR/decwidgets.lock")" 2>/dev/null
 e2e_down
 return 0
}
trap cleanup EXIT
e2e_up "$N"
export XDG_CACHE_HOME="$E2E_T/cache" XDG_STATE_HOME="$E2E_T/state" XDG_DATA_HOME="$E2E_T/data"
export XDG_DATA_DIRS="$E2E_T/nodata" DECEBAL_SYSTEM_THEMES="$E2E_T/nosystem"
export DECEBAL_WIDGETS="$E2E_T/user-widgets" DECSETTINGS_DEBUG_LAYOUT="$E2E_T/layout.json"
export DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent
mkdir -p "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME" "$DECEBAL_WIDGETS"
printf '%s\n' '{"widgets":{"enabled":false,"allow_exec":false,"future":"keep","items":[{"use":"builtin:clock-card","x":21,"options":{"sentinel":"keep"}}]}}' > "$DECEBAL_CONFIG/decebal.json"
e2e_start_app icons
export SETTINGS_WID=$(rdwmctl state | jq -r '.state.windows[]|select(.class=="DecSettings")|.id' | head -1)
python3 - <<'PY'
import pathlib,os,json,subprocess,time
layout=pathlib.Path(os.environ['DECSETTINGS_DEBUG_LAYOUT']);wid=os.environ['SETTINGS_WID']
def state(): return json.loads(layout.read_text())
def wait(test,reason):
 for _ in range(100):
  try:
   if test():return
  except (OSError,ValueError,KeyError):pass
  time.sleep(.1)
 raise AssertionError(reason)
def mouse(x,y,button=1):subprocess.run(['xdotool','mousemove','--window',wid,str(int(x)),str(int(y)),'click',str(button)],check=True);time.sleep(.2)
def click(id):
 for _ in range(40):
  s=state();c=next(c for c in s['ctls'] if c['id']==id);x,y,w,h=c['rect']
  height=int(subprocess.check_output(['xdotool','getwindowgeometry','--shell',wid],text=True).split('HEIGHT=')[1].splitlines()[0])
  if y>=95 and y+h<height-10:mouse(x+w/2,y+h/2);return
  mouse(650,height/2,5 if y+h>=height-10 else 4)
 raise AssertionError('control could not be revealed: '+id)
wait(lambda:state()['pane']=='icons','icons CLI did not open dedicated pane')
assert any(c['id']=='ic:use' for c in state()['ctls'])
ids=[p['id'] for p in state()['panes']];assert ids.index('wallpaper')<ids.index('icons')<ids.index('widgets')
widget=next(p for p in state()['panes'] if p['id']=='widgets');x,y,w,h=widget['rect'];mouse(x+w/2,y+h/2)
wait(lambda:state()['pane']=='widgets','native sidebar did not open widgets')
click('wg:new:open');wait(lambda:any(c['id']=='wg:new:create' for c in state()['ctls']),'creator form missing')
assert any(c['id']=='wg:new:template' for c in state()['ctls'])
click('wg:new:create')
cfg=pathlib.Path(os.environ['DECEBAL_CONFIG'])/'decebal.json'
wait(lambda:len(json.loads(cfg.read_text())['widgets']['items'])==2,'new widget was not saved')
w=json.loads(cfg.read_text())['widgets'];assert w['future']=='keep' and not w['allow_exec'] and w['enabled']
assert w['items'][0]['x']==21 and w['items'][0]['options']['sentinel']=='keep'
assert w['items'][1]['use']=='my-clock'
file=pathlib.Path(os.environ['DECEBAL_WIDGETS'])/'my-clock.toml';assert file.is_file()
wait(lambda:json.loads(subprocess.check_output(['decwidgets','status','--json']))['running'],'desktop host did not complete rendering')
status=json.loads(subprocess.check_output(['decwidgets','status','--json']));assert all(i['error'] is None for i in status['items'])
print('PASS native Icons startup/sidebar order, widget form/Create, saved file, preserved layout and ready desktop host')
PY
