#!/usr/bin/env python3
"""Real create/rename/trash controls, isolated Xvfb/bus/HOME/XDG fixtures only."""
from pathlib import Path
import hashlib,json,os,select,subprocess,tempfile,time,urllib.parse
binary=Path(os.environ.get('DECFILES_BIN',str(Path.home()/'.cache/decebal-files-target/debug/decfiles'))).resolve()
root=Path(tempfile.mkdtemp(prefix='decfiles-ops-native-',dir='/var/tmp'));root.chmod(0o700)
out=Path(os.environ.get('DECFILES_OPERATIONS_EVIDENCE',str(root/'evidence')));out.mkdir(parents=True,exist_ok=True);(out/'result.json').unlink(missing_ok=True)
env={k:v for k,v in os.environ.items() if k not in ('DISPLAY','WAYLAND_DISPLAY','XAUTHORITY','DBUS_SESSION_BUS_ADDRESS','DBUS_SYSTEM_BUS_ADDRESS','AT_SPI_BUS_ADDRESS','RDWM_SOCK','SESSION_MANAGER') and not k.startswith(('DECEBAL_','DECFILES_','GTK_','GDK_','GSK_'))}
for key,part in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_STATE_HOME','state'),('XDG_DATA_HOME','data'),('XDG_RUNTIME_DIR','run')]:
 env[key]=str(root/part);Path(env[key]).mkdir();Path(env[key]).chmod(0o700)
env.update(GDK_BACKEND='x11',GSK_RENDERER='cairo',GTK_A11Y='none',NO_AT_BRIDGE='1',GSETTINGS_BACKEND='memory',GIO_USE_VFS='local',DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(root/'no-system'),DECFILES_EVIDENCE_STATE=str(root/'state.json'))
base=Path(env['HOME'])/'Documents';base.mkdir();existing=base/'existing.txt';existing.write_bytes(b'never overwrite these bytes')
target=root/'external-target';target.write_bytes(b'never touch symlink target');link=base/'symlink';link.symlink_to(target)
raw=os.fsencode(base)+b'/raw-\xff';fd=os.open(raw,os.O_CREAT|os.O_WRONLY,0o600);os.write(fd,b'non UTF8 bytes');os.close(fd)
denied=Path(env['HOME'])/'Denied';denied.mkdir();(denied/'protected').write_bytes(b'protected');denied.chmod(0o500)
owned=[];logs=[];app=None;cases=[]
def start(args,name,**kw):
 f=(out/(name+'.log')).open('w');logs.append(f);p=subprocess.Popen(args,env=env,stdout=f,stderr=subprocess.STDOUT,**kw);owned.append(p);return p
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
def at(path):
 s=state();m=s.get('model',{});columns=m.get('columns',[])
 return s if not m.get('pending') and columns and columns[-1]['path']==str(path) else None
def find_window(name):
 q=subprocess.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name',name],env=env,capture_output=True,text=True,timeout=3)
 return q.stdout.strip().splitlines() if q.returncode==0 else None
def key(*keys):run(['xdotool','key','--clearmodifiers',*keys])
def focus(win):run(['xdotool','windowfocus','--sync',win])
def type_text(text):key('ctrl+a');run(['xdotool','type','--clearmodifiers','--delay','1',text])
def click(win,x,y):
 time.sleep(.2);run(['xdotool','mousemove','--window',win,str(x),str(y),'click','1'])
def geometry(win):
 text=run(['xdotool','getwindowgeometry','--shell',win]).stdout
 return {k:int(v) for k,v in (line.split('=',1) for line in text.splitlines()) if k in ('X','Y','WIDTH','HEIGHT')}
def snap(win,name):
 time.sleep(.2);run(['import','-window',win,str(out/(name+'.png'))])
def dialog(name):
 win=wait(lambda:find_window('^'+name+'$'),'dialog did not appear: '+name)[0];focus(win);return win
def finish_dialog(name,text):
 win=dialog(name);type_text(text);key('Return');wait(lambda:state()['operation']['dialog'] is None,'name dialog did not submit');focus(window)
def new_folder(name):
 focus(window);key('ctrl+shift+n');finish_dialog('Create folder',name)
def rename(name):
 focus(window);key('F2');finish_dialog('Rename item',name)
def select_item(path):
 wanted=os.fsencode(path);entries=state()['model']['columns'][-1]['entries'];index=next(i for i,e in enumerate(entries) if e['path_bytes']==list(wanted))
 click(window,310,103+50*index)
 wait(lambda:state().get('operation',{}).get('selected_bytes')==list(wanted),'selection did not match exact path')
def wait_result(kind='success',error_kind=None):
 def ready():
  s=state();op=s.get('operation',{});result=op.get('last_result') or {}
  return s if not op.get('busy') and not op.get('dialog') and not s.get('model',{}).get('pending') and result.get('status')==kind and (not error_kind or result.get('kind')==error_kind) else None
 return wait(ready,'operation did not finish as '+kind+' '+str(error_kind))
def open_trash():
 focus(window);click(window,270,geometry(window)['HEIGHT']-48);return dialog('Move item to Trash\\?')
def trash_confirm():
 win=open_trash();g=geometry(win);click(win,g['WIDTH']-72,g['HEIGHT']-24);focus(window)
def address(path):
 focus(window);key('ctrl+l');run(['xdotool','type','--clearmodifiers','--delay','1',str(path)]);key('Return');wait(lambda:at(path),'address did not load')
try:
 r,w=os.pipe();f=(out/'xvfb.log').open('w');logs.append(f)
 x=subprocess.Popen(['Xvfb','-displayfd',str(w),'-screen','0','1400x900x24','-noreset','-nolisten','tcp'],env=env,pass_fds=(w,),stdout=f,stderr=subprocess.STDOUT);owned.append(x);os.close(w);assert select.select([r],[],[],5)[0];env['DISPLAY']=':'+os.read(r,32).decode().strip();os.close(r)
 bus=subprocess.Popen(['dbus-daemon','--session','--nofork','--print-address=1'],env=env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True);owned.append(bus);env['DBUS_SESSION_BUS_ADDRESS']=bus.stdout.readline().strip();assert env['DBUS_SESSION_BUS_ADDRESS'].startswith('unix:')
 app=start([str(binary),str(base)],'app');wait(lambda:at(base),'initial folder missing');window=wait(lambda:find_window('^Decebal Files'),'main window missing')[0];focus(window)
 # Invalid input never dispatches an operation; Cancel/Escape does not mutate.
 key('ctrl+shift+n');win=dialog('Create folder');type_text('bad/name');key('Return');time.sleep(.15);assert state()['operation']['dialog']=='Create folder';assert not state()['operation']['busy'];snap(win,'invalid-name');key('Escape');focus(window);assert not (base/'bad').exists();cases.append('invalid leaf rejected before dispatch')
 new_folder('created folder');wait(lambda:(base/'created folder').is_dir(),'create did not change filesystem');wait_result();assert state()['operation']['selected']==str(base/'created folder');cases.append('create folder and select created item')
 (base/'created folder'/'inside').write_bytes(b'preserve directory contents')
 new_folder('existing.txt');wait_result('error','Exists');assert state()['operation']['last_result']['message']=='An item with that name already exists. Choose another name.';assert existing.read_bytes()==b'never overwrite these bytes';assert state()['operation']['selected']==str(base/'created folder');cases.append('create conflict preserves existing bytes and selection')
 rename('existing.txt');wait_result('error','Exists');assert state()['operation']['last_result']['message']=='An item with that name already exists. Choose another name.';assert existing.read_bytes()==b'never overwrite these bytes';assert (base/'created folder'/'inside').read_bytes()==b'preserve directory contents';assert state()['operation']['selected']==str(base/'created folder');snap(window,'rename-conflict');cases.append('rename conflict preserves source destination and selection')
 rename('renamed folder');wait(lambda:(base/'renamed folder').is_dir() and not (base/'created folder').exists(),'rename did not change folder');wait_result();assert (base/'renamed folder'/'inside').read_bytes()==b'preserve directory contents';assert state()['operation']['selected']==str(base/'renamed folder');cases.append('rename folder and retain contents')
 win=open_trash();snap(win,'trash-confirmation');key('Return');focus(window);wait(lambda:state()['operation']['dialog'] is None,'default cancel did not dismiss confirmation');assert (base/'renamed folder').exists();assert not (Path(env['XDG_DATA_HOME'])/'Trash/files').exists();cases.append('Trash default Enter cancels without mutation')
 win=open_trash();key('Escape');focus(window);wait(lambda:state()['operation']['dialog'] is None,'Escape did not cancel');assert (base/'renamed folder').exists();cases.append('Trash Escape cancels without mutation')
 trash_confirm();wait(lambda:not (base/'renamed folder').exists(),'confirmed Trash did not move folder');wait_result();cases.append('confirmed Trash moves directory')
 select_item(link);rename('renamed-symlink');wait(lambda:(base/'renamed-symlink').is_symlink() and not link.is_symlink(),'symlink rename failed');wait_result();assert os.readlink(base/'renamed-symlink')==str(target);assert target.read_bytes()==b'never touch symlink target';cases.append('selected symlink renamed without following target')
 trash_confirm();wait(lambda:not (base/'renamed-symlink').is_symlink(),'symlink Trash failed');wait_result();assert target.read_bytes()==b'never touch symlink target';cases.append('selected symlink trashed without following target')
 select_item(raw);rename('raw-renamed');wait(lambda:not os.path.lexists(raw) and (base/'raw-renamed').exists(),'non UTF8 rename failed');wait_result();assert (base/'raw-renamed').read_bytes()==b'non UTF8 bytes';cases.append('non UTF8 selected path renamed without lossy reconstruction')
 address(denied);select_item(denied/'protected');new_folder('not-permitted');wait_result('error','PermissionDenied');assert not (denied/'not-permitted').exists();assert state()['operation']['selected']==str(denied/'protected');assert at(denied);cases.append('denied creation preserves current folder and selection')
 trash=Path(env['XDG_DATA_HOME'])/'Trash';infos=[]
 for info in (trash/'info').glob('*.trashinfo'):
  text=info.read_text();assert '[Trash Info]' in text and 'DeletionDate=' in text
  encoded=next(line[5:] for line in text.splitlines() if line.startswith('Path='));original=Path(os.fsdecode(urllib.parse.unquote_to_bytes(encoded)));payload=trash/'files'/info.name[:-10]
  assert os.path.lexists(payload);infos.append({'source':str(original),'payload':str(payload),'symlink':payload.is_symlink()})
  if original==base/'renamed folder':assert (payload/'inside').read_bytes()==b'preserve directory contents'
  elif original==base/'renamed-symlink':assert payload.is_symlink() and os.readlink(payload)==str(target)
  else:raise AssertionError('unexpected Trash entry '+str(original))
 assert len(infos)==2;cases.append('Trash payloads and trashinfo independently verified')
 (out/'state-final.json').write_text(json.dumps(state(),indent=2)+'\n');focus(window);key('ctrl+w');app.wait(timeout=8);assert app.returncode==0
 source_root=Path(__file__).resolve().parents[1];sources={str(p.relative_to(source_root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source_root.rglob('*')) if p.is_file() and (p.suffix=='.rs' or p.name in ('Cargo.toml','Cargo.lock'))}
 result={'features':[],'passed':True,'cases':cases,'trash_entries':infos,'binary':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'source_hashes':sources,'private_root':str(root),'scope':'Private X11 actual GTK controls; no live data/default-handler access; no undo/Orca/Wayland acceptance'}
 (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
finally:
 denied.chmod(0o700)
 for p in reversed(owned):
  if p.poll() is None:p.terminate()
  try:p.wait(timeout=5)
  except subprocess.TimeoutExpired:p.kill();p.wait()
 for f in logs:f.close()
