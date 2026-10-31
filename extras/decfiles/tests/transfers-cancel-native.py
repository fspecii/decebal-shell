#!/usr/bin/env python3
"""Only the fixture-delay binary: a capped per-chunk delay makes copy progress, mid-copy
Cancel, Keep Open, cancel-and-close and an external source change observable through
actual GTK controls. Private Xvfb/bus/HOME/XDG fixtures only."""
from pathlib import Path
import sys as _sys
_sys.path.insert(0,str(Path(__file__).resolve().parent))
import xserver
import hashlib,json,os,select,subprocess,tempfile,time
binary=Path(os.environ.get('DECFILES_FIXTURE_BIN','/var/tmp/decfiles-transfer-artifacts/fixture-delay/decfiles')).resolve()
root=Path(tempfile.mkdtemp(prefix='decfiles-transfers-cancel-',dir='/var/tmp'));root.chmod(0o700)
out=Path(os.environ.get('DECFILES_TRANSFERS_CANCEL_EVIDENCE',str(root/'evidence')));out.mkdir(parents=True,exist_ok=True);(out/'result.json').unlink(missing_ok=True)
env={k:v for k,v in os.environ.items() if k not in ('DISPLAY','WAYLAND_DISPLAY','XAUTHORITY','DBUS_SESSION_BUS_ADDRESS','DBUS_SYSTEM_BUS_ADDRESS','AT_SPI_BUS_ADDRESS','RDWM_SOCK','SESSION_MANAGER') and not k.startswith(('DECEBAL_','DECFILES_','GTK_','GDK_','GSK_'))}
for key,part in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_STATE_HOME','state'),('XDG_DATA_HOME','data'),('XDG_RUNTIME_DIR','run')]:
 env[key]=str(root/part);Path(env[key]).mkdir();Path(env[key]).chmod(0o700)
DELAY=150
env.update(GDK_BACKEND='x11',GSK_RENDERER='cairo',GTK_A11Y='none',NO_AT_BRIDGE='1',GSETTINGS_BACKEND='memory',GIO_USE_VFS='local',DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(root/'no-system'),DECFILES_EVIDENCE_STATE=str(root/'state.json'),DECFILES_FIXTURE_TRANSFER_DELAY_MS=str(DELAY))
base=Path(env['HOME'])/'Documents';base.mkdir()
payload=bytes((i*31)%251 for i in range(16*256*1024))
big=base/'big.bin';big.write_bytes(payload)
targets={name:base/name for name in ('Cancelled','Kept open','Changed','Closed')}
for path in targets.values():path.mkdir()
owned=[];logs=[];app=None;cases=[]
def run(args):return subprocess.run(args,env=env,check=True,capture_output=True,text=True,timeout=8)
def wait(fn,message,seconds=12):
 end=time.monotonic()+seconds
 while time.monotonic()<end:
  value=fn()
  if value:return value
  if app and app.poll() is not None:raise AssertionError('app exited '+str(app.returncode))
  time.sleep(.02)
 raise AssertionError(message)
def state():
 try:return json.loads((root/'state.json').read_text())
 except (FileNotFoundError,json.JSONDecodeError):return {}
def op():return state().get('operation',{})
def at(path):
 s=state();m=s.get('model',{});columns=m.get('columns',[])
 return s if not m.get('pending') and columns and columns[-1]['path']==str(path) else None
def find(name):
 p=subprocess.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name',name],env=env,capture_output=True,text=True,timeout=3)
 return p.stdout.strip().splitlines() if p.returncode==0 else None
def key(*keys):run(['xdotool','key','--clearmodifiers',*keys])
def focus(win):run(['xdotool','windowfocus','--sync',win])
def geom(win):return {k:int(v) for k,v in (line.split('=',1) for line in run(['xdotool','getwindowgeometry','--shell',win]).stdout.splitlines()) if k in ('WIDTH','HEIGHT')}
def click(win,x,y):
 time.sleep(.15);run(['xdotool','mousemove','--window',win,str(x),str(y),'click','1'])
def address(path):
 focus(window);key('ctrl+l');run(['xdotool','type','--clearmodifiers','--delay','1',str(path)]);key('Return');wait(lambda:at(path),'address did not load')
def last_id():return (op().get('last_result') or {}).get('id',0)
def paste_running():
 # Paste button sits after Cut/Copy; the progress row appears above the bar, not inside it.
 previous=last_id();focus(window);click(window,562,geom(window)['HEIGHT']-52)
 wait(lambda:((op().get('transfer') or {}).get('progress') or {}).get('bytes_done',0)>0 and op().get('busy'),'transfer did not report byte progress');return previous
def finished(previous,status):
 def ready():
  s=state();o=s.get('operation',{});r=o.get('last_result') or {}
  return r if not o.get('busy') and not s.get('model',{}).get('pending') and r.get('id',0)>previous else None
 result=wait(ready,'transfer did not finish',20);assert result['status']==status,result;return result
def close_prompt():
 focus(window);key('ctrl+w');dialog=wait(lambda:find('^Close Files during an operation[?]$'),'close confirmation absent')[0];focus(dialog);return dialog
def start_x():
 # Reserved high display, both socket namespaces proven ours before any input.
 xserver.start(env,lambda name:(lambda f:(logs.append(f),f)[1])((out/(name+'.log')).open('w')),owned)
try:
 start_x()
 bus=subprocess.Popen(['dbus-daemon','--session','--nofork','--print-address=1'],env=env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True);owned.append(bus);env['DBUS_SESSION_BUS_ADDRESS']=bus.stdout.readline().strip();assert env['DBUS_SESSION_BUS_ADDRESS'].startswith('unix:')
 log=(out/'app.log').open('w');logs.append(log);app=subprocess.Popen([str(binary),str(base)],env=env,stdout=log,stderr=subprocess.STDOUT);owned.append(app)
 wait(lambda:at(base),'initial folder missing');window=wait(lambda:find('^Decebal Files'),'window missing')[0];focus(window)
 entries=state()['model']['columns'][-1]['entries'];index=next(i for i,e in enumerate(entries) if e['path']==str(big))
 click(window,310,103+50*index);wait(lambda:op().get('selected')==str(big),'big.bin not selected');key('ctrl+c')
 # Progress, then actual Cancel operation mid-file: no partial or hidden file is published.
 address(targets['Cancelled']);previous=paste_running();progress=op()['transfer']['progress']
 assert progress['bytes_total']==len(payload) and 0<progress['bytes_done']<len(payload),progress;run(['import','-window',window,str(out/'transfer-progress.png')])
 click(window,450,geom(window)['HEIGHT']-52);result=finished(previous,'cancelled')
 assert result['message']=='Cancelled. Nothing was changed.' and not result['report']['changed'];assert not any(targets['Cancelled'].iterdir());assert big.read_bytes()==payload
 cases.append('progress reports bytes and Cancel operation mid-file publishes nothing')
 # Close request during a transfer: default Enter keeps the window and the copy completes.
 address(targets['Kept open']);previous=paste_running();close_prompt();key('Return')
 wait(lambda:op().get('dialog') is None,'Keep Open did not dismiss');assert app.poll() is None
 finished(previous,'success');copy=targets['Kept open']/'big.bin';assert copy.read_bytes()==payload and copy.stat().st_mtime_ns==big.stat().st_mtime_ns
 cases.append('close prompt Keep Open lets the transfer finish with verified bytes')
 # A source modified mid-copy is reported and never published as complete.
 address(targets['Changed']);previous=paste_running()
 with big.open('ab') as handle:handle.write(b'external append')
 result=finished(previous,'error');assert 'changed' in result['message'] or 'grew' in result['message'],result;assert not any(targets['Changed'].iterdir())
 big.write_bytes(payload);cases.append('external source change mid-copy reports failure without a destination file')
 # Cancel and close: exits promptly without joining the worker; no partial file appears.
 address(targets['Closed']);paste_running();dialog=close_prompt();run(['import','-window',dialog,str(out/'close-during-transfer.png')]);g=geom(dialog)
 started=time.monotonic();click(dialog,g['WIDTH']-135,g['HEIGHT']-24);app.wait(timeout=3);elapsed=time.monotonic()-started;assert app.returncode==0
 time.sleep(.5);assert not any(targets['Closed'].iterdir());assert big.read_bytes()==payload
 cases.append('cancel and close exits without waiting and leaves no partial file')
 source_root=Path(__file__).resolve().parents[1];sources={str(p.relative_to(source_root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source_root.rglob('*')) if p.is_file() and 'target' not in p.parts and (p.suffix=='.rs' or p.name in ('Cargo.toml','Cargo.lock'))}
 output={'passed':True,'features':['fixture-delay'],'fixture_transfer_delay_ms_per_256KiB_chunk':DELAY,'cases':cases,'close_elapsed_seconds':elapsed,'binary':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'source_hashes':sources,'private_root':str(root),'scope':'Controlled per-chunk fixture delay only; not a slow or blocked physical filesystem'}
 (out/'result.json').write_text(json.dumps(output,indent=2)+'\n');print(json.dumps(output))
finally:
 for p in reversed(owned):
  if p.poll() is None:p.terminate()
  try:p.wait(timeout=5)
  except subprocess.TimeoutExpired:p.kill();p.wait()
 for f in logs:f.close()
