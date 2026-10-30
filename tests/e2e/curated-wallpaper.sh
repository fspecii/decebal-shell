#!/usr/bin/env bash
# Native curated-photo selection against a loopback API; no public Wallhaven calls.
set -eu
HERE=$(cd "$(dirname "$0")/../.." && pwd)
OLDHOME=$HOME; export OLDHOME
export MS_BIN_DIR=${MS_BIN_DIR:-$HERE/target/debug}
export E2E_SCREEN=1600x1000x24 E2E_WALLS=/nonexistent
# Ignore session accessibility overrides for deterministic private test geometry.
unset DECEBAL_FONT_SCALE DECEBAL_HIGH_CONTRAST DECEBAL_REDUCE_MOTION
source "$HERE/tests/e2e/env.sh"
N=${E2E_DISPLAY:-:299}
[[ "$N" =~ ^:([0-9]+)$ ]] || exit 2
[ "$((10#${BASH_REMATCH[1]}))" -ge 2 ] || exit 2
[ ! -e "/tmp/.X11-unix/X${N#:}" ] && [ ! -e "/tmp/.X${N#:}-lock" ] || exit 77
SRV_PID=''
ISOLATION_ROOT=$(mktemp -d /tmp/decebal-curated-xdg.XXXXXX)
# Isolate paths before rdwm starts, too; env.sh overrides HOME/config/runtime.
export XDG_CACHE_HOME="$ISOLATION_ROOT/cache" XDG_STATE_HOME="$ISOLATION_ROOT/state" XDG_DATA_HOME="$ISOLATION_ROOT/data"
export XDG_DATA_DIRS="$ISOLATION_ROOT/nodata" DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent
cleanup() { [ -z "$SRV_PID" ] || kill "$SRV_PID" 2>/dev/null || true; e2e_down; rm -rf "$ISOLATION_ROOT"; }
trap cleanup EXIT
e2e_up "$N"
export E2E_CFG
export XDG_CACHE_HOME="$E2E_T/cache" XDG_STATE_HOME="$E2E_T/state" XDG_DATA_HOME="$E2E_T/data"
export XDG_DATA_DIRS="$E2E_T/nodata" DECEBAL_SYSTEM_THEMES="$E2E_T/nosystem"
export DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent DECSETTINGS_DEBUG_LAYOUT="$E2E_T/layout.json"
export CURATED_ROOT="$HERE"
python3 - <<'PY'
import json,os,pathlib
from PIL import Image
root=pathlib.Path(os.environ['CURATED_ROOT']);tmp=pathlib.Path(os.environ['E2E_CFG']).parent.parent.parent
cache=pathlib.Path(os.environ['XDG_CACHE_HOME'])/'decebal/wallhaven/thumbs';cache.mkdir(parents=True)
catalog=json.loads((root/'data/wallhaven-curated.json').read_text())
for ident in catalog['images']:
 for suffix in ['.s','.l']:
  Image.new('RGB',(300,180),(36,110,136)).save(cache/(ident+suffix),'PNG')
config=pathlib.Path(os.environ['E2E_CFG']);initial=tmp/'initial.png'
Image.new('RGB',(64,64),(100,30,30)).save(initial)
config.write_text(config.read_text().replace('mode = "fill"','image = '+json.dumps(str(initial))+'\nmode = "fit"'))
(pathlib.Path(os.environ['DECEBAL_CONFIG'])/'decebal.json').write_text(json.dumps({'theme':{'name':'macos','mode':'dark'}}))
PY
mkdir -p "$E2E_T/server"
python3 -I "$HERE/tests/e2e/fakes/curated_wallhaven_server.py" "$E2E_T/server" >"$E2E_T/server.log" 2>&1 & SRV_PID=$!
for _ in $(seq 1 50); do [ ! -s "$E2E_T/server/port" ] || break; sleep .1; done
export WALLHAVEN_API_BASE="http://127.0.0.1:$(cat "$E2E_T/server/port")/api/v1"
e2e_start_app wallpaper
export SETTINGS_WID=$(xdotool search --onlyvisible --class DecSettings | head -1)
python3 - <<'PY'
import json,os,pathlib,subprocess,time,tomllib
root=pathlib.Path(os.environ['CURATED_ROOT']);layout=pathlib.Path(os.environ['DECSETTINGS_DEBUG_LAYOUT']);wid=os.environ['SETTINGS_WID']
tmp=pathlib.Path(os.environ['E2E_CFG']).parent.parent.parent
cfg=pathlib.Path(os.environ['E2E_CFG']);before=cfg.read_bytes();shell=pathlib.Path(os.environ['DECEBAL_CONFIG'])/'decebal.json';shell_before=shell.read_bytes()
request=tmp/'server/requests.jsonl';mode=tmp/'server/mode'
profile=json.loads((root/'data/wallhaven-curated.json').read_text())['themes']['macos']
dark=profile['dark']['image_id'];light=profile['light']['image_id']
def state():return json.loads(layout.read_text())
def wait(fn,why):
 for _ in range(120):
  try:
   value=fn()
   if value:return value
  except (OSError,ValueError,KeyError,StopIteration):pass
  time.sleep(.1)
 raise AssertionError(why+'; state='+str(state()))
def requests():return [json.loads(s) for s in request.read_text().splitlines()] if request.exists() else []
def mouse(rect):
 x,y,w,h=rect;subprocess.run(['xdotool','mousemove','--window',wid,str(int(x+w/2)),str(int(y+h/2)),'click','1'],check=True);time.sleep(.2)
def reveal(get):
 for _ in range(50):
  rect=get();y=rect[1];h=rect[3]
  if y>=95 and y+h<975:return rect
  subprocess.run(['xdotool','mousemove','--window',wid,'900','600','click','5' if y+h>=975 else '4'],check=True);time.sleep(.1)
 raise AssertionError('failed to reveal '+str(get()))
def control(ident):return next(c for c in state()['ctls'] if c['id']==ident)
def click(ident):mouse(reveal(lambda:control(ident)['rect']))
def thumb(ident):mouse(reveal(lambda:next(c for c in state()['thumbs'] if c['id']=='wh:'+ident)['rect']))
def enabled():return control('wh:set')['enabled']
wait(lambda:any(c['id']=='wh:curated' for c in state()['ctls']),'Wallpaper entry missing')
assert not requests(),'opening Backgrounds unexpectedly requested network'
click('wh:curated')
wait(lambda:len([c for c in state()['thumbs'] if c['id'].startswith('wh:')])==2,'two curated choices missing')
wait(lambda:state()['wh_thumbs']>=2,'cached thumbnails did not decode')
assert not requests(),'curated listing unexpectedly requested API search'
assert cfg.read_bytes()==before and shell.read_bytes()==shell_before,'browsing changed preferences'
thumb(dark);wait(lambda:enabled(),'SFW photo never became available')
assert all(r['path']=='/api/v1/w/'+dark for r in requests()),requests()
assert cfg.read_bytes()==before,'preview changed wallpaper'
click('wh:set')
background=pathlib.Path(os.environ['DECEBAL_CONFIG'])/'backgrounds'/('wallhaven-'+dark+'.png')
wait(lambda:background.exists() and tomllib.loads(cfg.read_text())['wallpaper']['image']==str(background),'Set failed')
current=tomllib.loads(cfg.read_text());assert current['wallpaper']['mode']=='fit';assert current['general']['smart_gaps']
assert background.read_bytes().startswith(b'\x89PNG\r\n\x1a\n')
assert len([r for r in requests() if r['path'].startswith('/full/')])==1
assert len([r for r in requests() if r['path']=='/api/v1/w/'+dark])>=2,'download skipped live recheck'
after=cfg.read_bytes()
click('wh:close');mode.write_text('ai');thumb(light)
wait(lambda:any(c['id']=='wh:verify' for c in state()['ctls']),'AI photo did not finish its check')
assert not enabled(),'AI flagged recommendation can be applied'
assert any('photography checks' in s for s in state()['warnings']),state()['warnings']
mode.write_text('nsfw');count=len(requests());click('wh:verify')
wait(lambda:len(requests())>count and any(c['id']=='wh:verify' for c in state()['ctls']),'NSFW recheck missing')
assert not enabled(),'NSFW recommendation can be applied'
assert cfg.read_bytes()==after,'rejected metadata changed wallpaper'
assert len([r for r in requests() if r['path'].startswith('/full/')])==1,'rejected photo downloaded'
assert not list(background.parent.glob('*.part'))
assert all(not r['path'].startswith('/api/v1/search') for r in requests()),requests()
print('PASS two cached curated photos, no browse mutation/search, native preview/Set, download recheck, preserved fit mode, AI and NSFW rejection')
PY
