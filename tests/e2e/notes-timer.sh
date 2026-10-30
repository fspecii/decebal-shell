#!/usr/bin/env bash
# Real native typing, linked data and timer completion after an atomic binary upgrade.
set -eu
HERE=$(cd "$(dirname "$0")/../.." && pwd)
OLDHOME=$HOME; export OLDHOME
export MS_BIN_DIR=${MS_BIN_DIR:-$HERE/target/release}
source "$HERE/tests/e2e/env.sh"
N=${E2E_DISPLAY:-:296}
[[ "$N" =~ ^:([0-9]{1,4})$ ]] || exit 2
N=":$((10#${BASH_REMATCH[1]}))"
[ "${N#:}" -ge 2 ] || exit 2
[ ! -e "/tmp/.X11-unix/X${N#:}" ] && [ ! -e "/tmp/.X${N#:}-lock" ] || exit 77
E2E_BOOT=$(mktemp -d /tmp/decebal-notes-bootstrap.XXXXXX)
trap 'e2e_down; rm -rf "$E2E_BOOT"' EXIT
export XDG_CACHE_HOME="$E2E_BOOT/cache" XDG_STATE_HOME="$E2E_BOOT/state" XDG_DATA_HOME="$E2E_BOOT/data"
export XDG_DATA_DIRS="$E2E_BOOT/nodata" DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent
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
decnotes --new >"$E2E_T/notes.log" 2>&1 & E2E_NOTES_PID=$!; E2E_PIDS+=($E2E_NOTES_PID); export E2E_NOTES_PID
python3 - <<'PY'
import os,json,pathlib,subprocess,time,shutil
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
# Notes' visible Focus timer action launches the sibling app with only its title.
g=dict(line.split('=',1) for line in run('xdotool','getwindowgeometry','--shell',w).splitlines())
run('xdotool','mousemove','--window',w,str(int(g['WIDTH'])-165),'32','click','1')
linked=window('Dectimer');run('xdotool','windowfocus',linked)
assert not (state/'decebal/timer/timer.json').exists(), 'preparing a task must not start a countdown'
parent=os.environ['E2E_NOTES_PID']
linked_pid=wait(lambda:next((pid for pid in pathlib.Path('/proc/'+parent+'/task/'+parent+'/children').read_text().split()
 if b'--task\0' in pathlib.Path('/proc/'+pid+'/cmdline').read_bytes()),None),'Notes did not launch a task Timer')
argv=pathlib.Path('/proc/'+linked_pid+'/cmdline').read_bytes().split(b'\0')
assert b'--task' in argv and b'A useful thought' in argv
assert not any(b'tomorrow' in arg for arg in argv), 'note body leaked into process arguments'
if os.environ.get('ECOSYSTEM_EVIDENCE_DIR'):
 evidence=pathlib.Path(os.environ['ECOSYSTEM_EVIDENCE_DIR']);evidence.mkdir(parents=True,exist_ok=True)
 subprocess.run(['import','-window',w,str(evidence/'notes-native.png')],check=True)
 subprocess.run(['import','-window',linked,str(evidence/'timer-native.png')],check=True)
run('xdotool','key','Escape')
wait(lambda:not pathlib.Path('/proc/'+linked_pid).exists(),'prepared Timer did not close')
# Native timer controls: replace duration through the visible duration field.
timer_binary=pathlib.Path(shutil.which('dectimer'))
upgrade_dir=state/'upgrade bin with spaces';upgrade_dir.mkdir()
current=upgrade_dir/'dectimer';replacement=upgrade_dir/'dectimer.new'
shutil.copy2(timer_binary,current)
p=subprocess.Popen([str(current),'--task','A useful thought']);w=window('Dectimer');run('xdotool','windowfocus',w)
# A normal release update atomically replaces the on-disk executable while its
# existing UI keeps running from the old inode. Linux reports that inode as deleted.
shutil.copy2(timer_binary,replacement);os.replace(replacement,current)
assert os.readlink('/proc/'+str(p.pid)+'/exe').endswith(' (deleted)'), 'upgrade fixture did not replace the live executable'
def geom():return dict(line.split('=',1) for line in run('xdotool','getwindowgeometry','--shell',w).splitlines())
g=geom();cx=int(g['WIDTH'])/2;top=max(12,(int(g['HEIGHT'])-420)/2)
run('xdotool','mousemove','--window',w,str(int(cx)),str(int(top+245)),'click','1');run('xdotool','type','--clearmodifiers','2');run('xdotool','key','Return')
f=state/'decebal/timer/timer.json';wait(lambda:f.exists() and json.loads(f.read_text())['status']=='Running','timer failed to start')
assert json.loads(f.read_text())['title']=='A useful thought', 'linked task title was not shared'
def workers():
 matches=[]
 for entry in pathlib.Path('/proc').glob('[0-9]*'):
  try: args=(entry/'cmdline').read_bytes().split(b'\0')
  except (FileNotFoundError,ProcessLookupError,PermissionError): continue
  if len(args)>2 and args[0]==os.fsencode(current) and args[1]==b'--timer-worker' and args[2]==os.fsencode(f.parent): matches.append(entry)
 return matches
wait(workers,'replacement executable did not spawn a real timer worker')
# Closing the app must leave countdown delivery alive.
run('xdotool','key','Escape');p.wait(timeout=5)
wait(lambda:(state/'completions').exists(),'completion after close not delivered')
wait(lambda:not workers(),'completed worker did not exit')
assert len((state/'completions').read_text().splitlines())==1
assert json.loads(f.read_text())['status']=='Complete'
# Both built-ins render real shared data, including literal note braces.
for id in ['quick-note','timer-card']:
 subprocess.run(['decwidgets','render','builtin:'+id,'--live','--png',str(state/(id+'.png'))],check=True)
 assert (state/(id+'.png')).stat().st_size>100
print('PASS native notes/shared widgets and atomic-upgrade timer mouse start/completion after close')
PY
