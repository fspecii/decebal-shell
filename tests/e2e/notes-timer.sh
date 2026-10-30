#!/usr/bin/env bash
# Real native typing, persistence, linked widgets and completion after window close.
set -eu
HERE=$(cd "$(dirname "$0")/../.." && pwd)
OLDHOME=$HOME; export OLDHOME
export MS_BIN_DIR=${MS_BIN_DIR:-$HERE/target/release}
source "$HERE/tests/e2e/env.sh"
N=${E2E_DISPLAY:-:296}
case "$N" in :0|:1) exit 2;; esac
[ ! -e "/tmp/.X11-unix/X${N#:}" ] && [ ! -e "/tmp/.X${N#:}-lock" ] || exit 77
trap e2e_down EXIT
export E2E_WALLS=/nonexistent
e2e_up "$N"
export XDG_CACHE_HOME="$E2E_T/cache" XDG_STATE_HOME="$E2E_T/state" XDG_DATA_HOME="$E2E_T/data"
export XDG_DATA_DIRS="$E2E_T/nodata" DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent
mkdir -p "$E2E_T/tools" "$XDG_DATA_HOME" "$XDG_STATE_HOME"
cat > "$E2E_T/tools/notify-send" <<'NOTIFY'
#!/bin/sh
printf '%s\n' "$*" >> "$XDG_STATE_HOME/completions"
NOTIFY
chmod +x "$E2E_T/tools/notify-send"
export PATH="$E2E_T/tools:$PATH"
decnotes --new >"$E2E_T/notes.log" 2>&1 & E2E_PIDS+=($!)
python3 - <<'PY'
import os,json,pathlib,subprocess,time
state=pathlib.Path(os.environ['XDG_STATE_HOME']);data=pathlib.Path(os.environ['XDG_DATA_HOME'])
def run(*args):return subprocess.check_output(args,text=True).strip()
def wait(fn,reason):
 for _ in range(120):
  try:
   value=fn()
   if value:return value
  except (OSError,ValueError,KeyError,subprocess.CalledProcessError):pass
  time.sleep(.1)
 raise AssertionError(reason)
def window(cls):return wait(lambda:run('xdotool','search','--onlyvisible','--class',cls).splitlines()[0],cls+' window missing')
w=window('Decnotes');run('xdotool','windowfocus',w);run('xdotool','type','--clearmodifiers','A useful thought');run('xdotool','key','Tab');run('xdotool','type','--clearmodifiers','Literal {cpu.usage} and a reminder for tomorrow.')
notes=data/'decebal/notes'
note=wait(lambda:next(iter(notes.glob('*.json')),None),'note not autosaved')
wait(lambda:json.loads(note.read_text())['body'].endswith('tomorrow.'),'body not persisted')
assert json.loads(note.read_text())['title']=='A useful thought'
# Native timer controls: replace duration through the visible duration field.
p=subprocess.Popen(['dectimer']);w=window('Dectimer');run('xdotool','windowfocus',w)
def geom():return dict(line.split('=',1) for line in run('xdotool','getwindowgeometry','--shell',w).splitlines())
g=geom();cx=int(g['WIDTH'])/2;top=max(12,(int(g['HEIGHT'])-420)/2)
run('xdotool','mousemove','--window',w,str(int(cx)),str(int(top+245)),'click','1');run('xdotool','type','--clearmodifiers','2');run('xdotool','key','Return')
f=state/'decebal/timer/timer.json';wait(lambda:f.exists() and json.loads(f.read_text())['status']=='Running','timer failed to start')
# Closing the app must leave countdown delivery alive.
run('xdotool','key','Escape');p.wait(timeout=5)
wait(lambda:(state/'completions').exists(),'completion after close not delivered')
assert len((state/'completions').read_text().splitlines())==1
assert json.loads(f.read_text())['status']=='Complete'
# Both built-ins render real shared data, including literal note braces.
for id in ['quick-note','timer-card']:
 subprocess.run(['decwidgets','render','builtin:'+id,'--live','--png',str(state/(id+'.png'))],check=True)
 assert (state/(id+'.png')).stat().st_size>100
print('PASS native note typing/autosave, literal content, timer mouse start, completion after close, linked widget renders')
PY
