#!/usr/bin/env python3
"""Only fixture-delay binary: deterministic pre-dispatch cancellation/close UI."""
from pathlib import Path
import sys as _sys
_sys.path.insert(0,str(Path(__file__).resolve().parent))
import xserver
import hashlib,json,os,select,subprocess,tempfile,time
binary=Path(os.environ.get('DECFILES_FIXTURE_BIN','/var/tmp/decfiles-operations-artifacts/fixture-delay/decfiles')).resolve()
root=Path(tempfile.mkdtemp(prefix='decfiles-close-native-',dir='/var/tmp'));root.chmod(0o700)
out=Path(os.environ.get('DECFILES_CLOSE_EVIDENCE',str(root/'evidence')));out.mkdir(parents=True,exist_ok=True);(out/'result.json').unlink(missing_ok=True)
env={k:v for k,v in os.environ.items() if k not in ('DISPLAY','WAYLAND_DISPLAY','XAUTHORITY','DBUS_SESSION_BUS_ADDRESS','DBUS_SYSTEM_BUS_ADDRESS','AT_SPI_BUS_ADDRESS','RDWM_SOCK','SESSION_MANAGER') and not k.startswith(('DECEBAL_','DECFILES_','GTK_','GDK_','GSK_'))}
for key,part in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_STATE_HOME','state'),('XDG_DATA_HOME','data'),('XDG_RUNTIME_DIR','run')]:
 env[key]=str(root/part);Path(env[key]).mkdir();Path(env[key]).chmod(0o700)
env.update(GDK_BACKEND='x11',GSK_RENDERER='cairo',GTK_A11Y='none',NO_AT_BRIDGE='1',GSETTINGS_BACKEND='memory',GIO_USE_VFS='local',DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(root/'no-system'),DECFILES_EVIDENCE_STATE=str(root/'state.json'),DECFILES_FIXTURE_DELAY_MS='5000')
base=Path(env['HOME'])/'Documents';base.mkdir();(base/'sentinel').write_bytes(b'unchanged')
owned=[];logs=[];app=None
def run(args):return subprocess.run(args,env=env,check=True,capture_output=True,text=True,timeout=8)
def wait(fn,message,seconds=8):
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
def find(name):
 p=subprocess.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name',name],env=env,capture_output=True,text=True,timeout=3)
 return p.stdout.strip().splitlines() if p.returncode==0 else None
def key(*keys):run(['xdotool','key','--clearmodifiers',*keys])
def focus(win):run(['xdotool','windowfocus','--sync',win])
def geom(win):return {k:int(v) for k,v in (line.split('=',1) for line in run(['xdotool','getwindowgeometry','--shell',win]).stdout.splitlines()) if k in ('WIDTH','HEIGHT')}
def click(win,x,y):
 time.sleep(.15);run(['xdotool','mousemove','--window',win,str(x),str(y),'click','1'])
def create(name):
 focus(window);key('ctrl+shift+n');dialog=wait(lambda:find('^Create folder$'),'create dialog missing')[0];focus(dialog);run(['xdotool','type','--clearmodifiers','--delay','1',name]);key('Return');wait(lambda:state().get('operation',{}).get('busy'),'queued operation missing');focus(window)
def close_prompt():
 focus(window);key('ctrl+w');dialog=wait(lambda:find('^Close Files during an operation[?]$'),'close confirmation absent')[0];focus(dialog);return dialog
try:
 xserver.start(env,lambda name:(lambda f:(logs.append(f),f)[1])((out/(name+'.log')).open('w')),owned)  # reserved high display, owned in both socket namespaces
 bus=subprocess.Popen(['dbus-daemon','--session','--nofork','--print-address=1'],env=env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True);owned.append(bus);env['DBUS_SESSION_BUS_ADDRESS']=bus.stdout.readline().strip();assert env['DBUS_SESSION_BUS_ADDRESS'].startswith('unix:')
 log=(out/'app.log').open('w');logs.append(log);app=subprocess.Popen([str(binary),str(base)],env=env,stdout=log,stderr=subprocess.STDOUT);owned.append(app)
 wait(lambda:state().get('model',{}).get('columns') and state()['model'].get('pending') is None,'initial folder missing');window=wait(lambda:find('^Decebal Files'),'window missing')[0]
 create('cancel-before-start');g=geom(window);click(window,450,g['HEIGHT']-48)
 wait(lambda:state().get('operation',{}).get('cancel_requested'),'cancel button did not request cancellation')
 wait(lambda:not state().get('operation',{}).get('busy'),'cancelled queued operation did not finish')
 result=state()['operation']['last_result'];assert result['kind']=='Cancelled' and result['may_have_changed'] is False;assert not (base/'cancel-before-start').exists()
 create('close-before-start');dialog=close_prompt();key('Return');wait(lambda:state()['operation']['dialog'] is None,'Keep Open did not dismiss');assert app.poll() is None;assert state()['operation']['busy'];assert not state()['operation']['cancel_requested']
 dialog=close_prompt();key('Escape');wait(lambda:state()['operation']['dialog'] is None,'Escape did not keep open');assert app.poll() is None and state()['operation']['busy']
 dialog=close_prompt();time.sleep(.15);run(['import','-window',dialog,str(out/'close-confirmation.png')]);g=geom(dialog);started=time.monotonic();click(dialog,g['WIDTH']-135,g['HEIGHT']-24);app.wait(timeout=2);elapsed=time.monotonic()-started;assert app.returncode==0
 assert state()['operation']['cancel_requested'];assert not (base/'close-before-start').exists();assert not (base/'cancel-before-start').exists();assert (base/'sentinel').read_bytes()==b'unchanged';assert not (Path(env['XDG_DATA_HOME'])/'Trash').exists()
 source_root=Path(__file__).resolve().parents[1];sources={str(p.relative_to(source_root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source_root.rglob('*')) if p.is_file() and (p.suffix=='.rs' or p.name in ('Cargo.toml','Cargo.lock'))}
 output={'passed':True,'features':['fixture-delay'],'fixture_delay_ms':5000,'cases':['actual Cancel operation before dispatch leaves filesystem unchanged','close default Enter keeps operation and window','close Escape keeps operation and window','explicit cancel-and-close exits without joining and leaves queued fixture unchanged'],'close_elapsed_seconds':elapsed,'binary':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'source_hashes':sources,'private_root':str(root),'scope':'Controlled pre-dispatch fixture delay only, not a blocked physical filesystem/commit-race proof'}
 (out/'result.json').write_text(json.dumps(output,indent=2)+'\n');print(json.dumps(output))
finally:
 for p in reversed(owned):
  if p.poll() is None:p.terminate()
  try:p.wait(timeout=5)
  except subprocess.TimeoutExpired:p.kill();p.wait()
 for f in logs:f.close()
