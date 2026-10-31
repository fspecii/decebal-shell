#!/usr/bin/env python3
"""Real Cut/Copy/Paste controls, conflict dialog and GNOME clipboard formats.
Private Xvfb/bus/HOME/XDG fixtures only; xclip talks to the private X server.
DECFILES_BIN and DECFILES_TRANSFERS_EVIDENCE choose binary and evidence folder."""
from pathlib import Path
import sys as _sys
_sys.path.insert(0,str(Path(__file__).resolve().parent))
import xserver
import hashlib,json,os,select,subprocess,tempfile,time
binary=Path(os.environ.get('DECFILES_BIN',str(Path.home()/'.cache/decebal-files-target/debug/decfiles'))).resolve()
root=Path(tempfile.mkdtemp(prefix='decfiles-transfers-native-',dir='/var/tmp'));root.chmod(0o700)
out=Path(os.environ.get('DECFILES_TRANSFERS_EVIDENCE',str(root/'evidence')));out.mkdir(parents=True,exist_ok=True);(out/'result.json').unlink(missing_ok=True)
env={k:v for k,v in os.environ.items() if k not in ('DISPLAY','WAYLAND_DISPLAY','XAUTHORITY','DBUS_SESSION_BUS_ADDRESS','DBUS_SYSTEM_BUS_ADDRESS','AT_SPI_BUS_ADDRESS','RDWM_SOCK','SESSION_MANAGER') and not k.startswith(('DECEBAL_','DECFILES_','GTK_','GDK_','GSK_'))}
for key,part in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_STATE_HOME','state'),('XDG_DATA_HOME','data'),('XDG_RUNTIME_DIR','run')]:
 env[key]=str(root/part);Path(env[key]).mkdir();Path(env[key]).chmod(0o700)
env.update(GDK_BACKEND='x11',GSK_RENDERER='cairo',GTK_A11Y='none',NO_AT_BRIDGE='1',GSETTINGS_BACKEND='memory',GIO_USE_VFS='local',DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(root/'no-system'),DECFILES_EVIDENCE_STATE=str(root/'state.json'))
base=Path(env['HOME'])/'Documents';base.mkdir()
MTIME=(1_600_000_000,123_456_789)
def fixture(path,data,mode=0o600):
 path.write_bytes(data);path.chmod(mode);os.utime(path,ns=(MTIME[0]*10**9+MTIME[1],)*2)
folder=base/'Folder A';folder.mkdir();fixture(folder/'nested.txt',b'nested bytes',0o640);(folder/'inner-link').symlink_to('nested.txt')
dest=base/'Dest';dest.mkdir();fixture(dest/'report.txt',b'existing destination bytes')
denied=base/'Denied';denied.mkdir();denied.chmod(0o500)
report=base/'report.txt';fixture(report,b'report bytes',0o640)
external=base/'external.txt';fixture(external,b'external clipboard bytes')
target=root/'external-target';target.write_bytes(b'never touch symlink target');link=base/'link';link.symlink_to(target)
raw=os.fsencode(base)+b'/raw-\xff';fd=os.open(raw,os.O_CREAT|os.O_WRONLY,0o600);os.write(fd,b'raw name bytes');os.close(fd)
owned=[];logs=[];app=None;cases=[]
def start(args,name,**kw):
 f=(out/(name+'.log')).open('w');logs.append(f);p=subprocess.Popen(args,env=env,stdout=f,stderr=subprocess.STDOUT,**kw);owned.append(p);return p
def run(args,check=True):return subprocess.run(args,env=env,check=check,capture_output=True,timeout=8)
def wait(fn,message,seconds=10):
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
def find_window(name):
 q=subprocess.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name',name],env=env,capture_output=True,text=True,timeout=3)
 return q.stdout.strip().splitlines() if q.returncode==0 else None
def key(*keys):run(['xdotool','key','--clearmodifiers',*keys])
def focus(win):run(['xdotool','windowfocus','--sync',win])
def click(win,x,y):
 time.sleep(.2);run(['xdotool','mousemove','--window',win,str(x),str(y),'click','1'])
def geometry(win):
 text=run(['xdotool','getwindowgeometry','--shell',win]).stdout.decode()
 return {k:int(v) for k,v in (line.split('=',1) for line in text.splitlines()) if k in ('X','Y','WIDTH','HEIGHT')}
def snap(win,name):
 time.sleep(.25);run(['import','-window',win,str(out/(name+'.png'))])
def button(label):
 focus(window);click(window,{'Cut':413,'Copy':484,'Paste':562}[label],geometry(window)['HEIGHT']-52)
def select_item(path):
 focus(window);wanted=list(os.fsencode(path));entries=state()['model']['columns'][-1]['entries'];index=next(i for i,e in enumerate(entries) if e['path_bytes']==wanted)
 click(window,310,103+50*index);wait(lambda:op().get('selected_bytes')==wanted,'selection did not match exact path')
def address(path):
 focus(window);key('ctrl+l');run(['xdotool','type','--clearmodifiers','--delay','1',str(path)]);key('Return');wait(lambda:at(path),'address did not load '+str(path))
def last_id():return (op().get('last_result') or {}).get('id',0)
def finished(previous,status):
 def ready():
  s=state();o=s.get('operation',{});result=o.get('last_result') or {}
  return result if not o.get('busy') and not o.get('dialog') and not s.get('model',{}).get('pending') and result.get('id',0)>previous and result.get('kind')=='Transfer' else None
 result=wait(ready,'transfer did not finish');assert result['status']==status,result;return result
def conflict_dialog():
 wait(lambda:op().get('dialog')=='Item already exists','conflict dialog state missing');win=wait(lambda:find_window('^Item already exists$'),'conflict window missing')[0];focus(win);return win
def clipboard(target):
 p=run(['xclip','-selection','clipboard','-o','-t',target],check=False);return p.stdout if p.returncode==0 else None
def offer(target,text):
 p=start(['xclip','-quiet','-selection','clipboard','-t',target,'-i'],'xclip-'+target.replace('/','-'),stdin=subprocess.PIPE);p.stdin.write(text.encode());p.stdin.close()
 wait(lambda:clipboard(target)==text.encode(),'external clipboard owner not ready');return p
def same_meta(a,b):
 sa,sb=os.lstat(a),os.lstat(b);return (sa.st_mode&0o7777,sa.st_mtime_ns)==(sb.st_mode&0o7777,sb.st_mtime_ns)
def start_x():
 # Reserved high display, both socket namespaces proven ours before any input.
 xserver.start(env,lambda name:(lambda f:(logs.append(f),f)[1])((out/(name+'.log')).open('w')),owned)
try:
 start_x()
 bus=subprocess.Popen(['dbus-daemon','--session','--nofork','--print-address=1'],env=env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True);owned.append(bus);env['DBUS_SESSION_BUS_ADDRESS']=bus.stdout.readline().strip();assert env['DBUS_SESSION_BUS_ADDRESS'].startswith('unix:')
 app=start([str(binary),str(base)],'app');wait(lambda:at(base),'initial folder missing');window=wait(lambda:find_window('^Decebal Files'),'main window missing')[0];focus(window)
 # Copy button, then keyboard paste into a folder with a conflict: default Enter is Skip.
 select_item(report);button('Copy');wait(lambda:(op().get('clipboard') or {}).get('mode')=='Copy','Copy button did not publish clipboard')
 assert clipboard('x-special/gnome-copied-files')==b'copy\nfile://'+bytes(str(report),'utf8');assert clipboard('text/uri-list')==b'file://'+bytes(str(report),'utf8')+b'\r\n'
 cases.append('Copy button publishes GNOME copied-files and uri-list formats')
 address(dest);select_item(dest/'report.txt');previous=last_id();key('ctrl+v');win=conflict_dialog();snap(win,'conflict-dialog');key('Return')
 result=finished(previous,'success');assert result['report']['skipped']==1 and not result['report']['changed'];assert (dest/'report.txt').read_bytes()==b'existing destination bytes';assert not (dest/'report (copy).txt').exists()
 cases.append('conflict default Enter skips and preserves destination bytes')
 previous=last_id();button('Paste');win=conflict_dialog();g=geometry(win);click(win,g['WIDTH']-60,g['HEIGHT']-24)
 result=finished(previous,'success');copy=dest/'report (copy).txt';assert copy.read_bytes()==b'report bytes' and same_meta(copy,report);assert (dest/'report.txt').read_bytes()==b'existing destination bytes'
 wait(lambda:op().get('selected')==str(copy),'Keep Both result not selected');cases.append('Paste button Keep Both creates verified copy and selects it')
 # Recursive folder copy through keyboard Copy/Paste keeps bytes, metadata and inner symlink.
 address(base);select_item(folder);key('ctrl+c');address(dest);select_item(dest/'report.txt');previous=last_id();key('ctrl+v');finished(previous,'success')
 assert (dest/'Folder A'/'nested.txt').read_bytes()==b'nested bytes' and same_meta(dest/'Folder A'/'nested.txt',folder/'nested.txt');assert os.readlink(dest/'Folder A'/'inner-link')=='nested.txt';assert (folder/'nested.txt').read_bytes()==b'nested bytes'
 wait(lambda:op().get('selected')==str(dest/'Folder A'),'copied folder not selected');cases.append('keyboard folder copy preserves tree bytes metadata and relative symlink')
 # Non-UTF8 names round-trip through the clipboard URI encoding.
 address(base);select_item(raw);button('Copy');assert clipboard('x-special/gnome-copied-files').endswith(b'/raw-%FF')
 address(dest);previous=last_id();button('Paste');finished(previous,'success');assert Path(os.fsdecode(os.fsencode(dest)+b'/raw-\xff')).read_bytes()==b'raw name bytes';assert os.path.exists(raw)
 cases.append('non UTF8 item copied losslessly through clipboard URI')
 # Cut a selected symlink: the link moves, its target is untouched, the cut is consumed.
 address(base);select_item(link);button('Cut');assert clipboard('x-special/gnome-copied-files')==b'cut\nfile://'+bytes(str(link),'utf8')
 address(dest);previous=last_id();button('Paste');finished(previous,'success')
 assert os.readlink(dest/'link')==str(target) and not os.path.lexists(link);assert target.read_bytes()==b'never touch symlink target';assert op().get('clipboard') is None;assert clipboard('x-special/gnome-copied-files') is None
 cases.append('Cut and Paste moves selected symlink leaf and consumes the cut')
 # Another clipboard owner using the GNOME Files format: cut means move.
 owner=offer('x-special/gnome-copied-files','cut\nfile://'+str(external));previous=last_id();button('Paste');finished(previous,'success')
 assert (dest/'external.txt').read_bytes()==b'external clipboard bytes' and not external.exists();owner.terminate();owner.wait()
 cases.append('external x-special/gnome-copied-files cut pastes as a move')
 # A plain uri-list owner means copy; Escape at the conflict means Stop.
 owner=offer('text/uri-list','file://'+str(report)+'\r\n');previous=last_id();button('Paste');conflict_dialog();key('Escape')
 result=finished(previous,'cancelled');assert result['message']=='Cancelled. Nothing was changed.';assert (dest/'report.txt').read_bytes()==b'existing destination bytes';assert report.exists()
 cases.append('external uri-list paste conflict Escape stops without change')
 address(denied);previous=last_id();button('Paste');result=finished(previous,'error');assert 'permission denied' in result['message'];assert not any(denied.iterdir())
 wait(lambda:'permission denied' in (state()['model'].get('error') or ''),'denied error not shown');snap(window,'denied-paste');owner.terminate();owner.wait()
 cases.append('denied destination reports error and creates nothing')
 # Paste into the source folder duplicates without asking.
 address(base);select_item(report);key('ctrl+c');select_item(report);previous=last_id();key('ctrl+v');finished(previous,'success')
 assert (base/'report (copy).txt').read_bytes()==b'report bytes' and same_meta(base/'report (copy).txt',report);snap(window,'duplicate-selected')
 cases.append('same folder paste duplicates with copy name')
 (out/'state-final.json').write_text(json.dumps(state(),indent=2)+'\n');focus(window);key('ctrl+w');app.wait(timeout=8);assert app.returncode==0
 source_root=Path(__file__).resolve().parents[1];sources={str(p.relative_to(source_root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source_root.rglob('*')) if p.is_file() and 'target' not in p.parts and (p.suffix=='.rs' or p.name in ('Cargo.toml','Cargo.lock'))}
 result={'features':[],'passed':True,'cases':cases,'binary':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'source_hashes':sources,'private_root':str(root),'scope':'Private X11 actual GTK controls and private X clipboard owners; no live data, Nautilus process, Orca or Wayland acceptance'}
 (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
finally:
 denied.chmod(0o700)
 for p in reversed(owned):
  if p.poll() is None:p.terminate()
  try:p.wait(timeout=5)
  except subprocess.TimeoutExpired:p.kill();p.wait()
 for f in logs:f.close()
