#!/usr/bin/env python3
"""Private real-control navigation proof; never connects to the user's display/bus.
DECFILES_BIN and DECFILES_EVIDENCE choose binary and retained evidence folder.
"""
from pathlib import Path
import hashlib,json,os,select,signal,subprocess,tempfile,time
binary=Path(os.environ.get('DECFILES_BIN',str(Path.home()/'.cache/decebal-files-target/debug/decfiles'))).resolve()
root=Path(tempfile.mkdtemp(prefix='decfiles-native-',dir='/var/tmp'));root.chmod(0o700)
out=Path(os.environ.get('DECFILES_EVIDENCE',str(root/'evidence')));out.mkdir(parents=True,exist_ok=True)
(out/'result.json').unlink(missing_ok=True)
env={k:v for k,v in os.environ.items() if k not in ('DISPLAY','WAYLAND_DISPLAY','XAUTHORITY','DBUS_SESSION_BUS_ADDRESS','DBUS_SYSTEM_BUS_ADDRESS','AT_SPI_BUS_ADDRESS','RDWM_SOCK','SESSION_MANAGER') and not k.startswith(('DECEBAL_','DECFILES_','GTK_','GDK_','GSK_'))}
for key,part in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_STATE_HOME','state'),('XDG_DATA_HOME','data'),('XDG_RUNTIME_DIR','run')]:
 env[key]=str(root/part);Path(env[key]).mkdir();Path(env[key]).chmod(0o700)
env.update(GDK_BACKEND='x11',GSK_RENDERER='cairo',GTK_A11Y='none',NO_AT_BRIDGE='1',GSETTINGS_BACKEND='memory',DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(root/'no-system'),DECFILES_EVIDENCE_STATE=str(root/'state.json'))
base=Path(env['HOME'])/'Documents';base.mkdir();(base/'Alpha space'/'Nested').mkdir(parents=True);(base/'Beta').mkdir();(base/'readme.txt').write_text('fixture bytes\n');(base/'Blocked').mkdir();(base/'Blocked').chmod(0)
raw=bytes(str(base),'utf8')+b'/raw-\xff';fd=os.open(raw,os.O_CREAT|os.O_WRONLY,0o600);os.write(fd,b'raw');os.close(fd)
physical=root/'physical';(physical/'nested').mkdir(parents=True);(physical/'marker').write_text('physical target');(base/'link').symlink_to(physical/'nested',target_is_directory=True)
owned=[];logs=[];cases=[]
def start(args,name,**kw):
 f=(out/(name+'.log')).open('w');logs.append(f);p=subprocess.Popen(args,env=env,stdout=f,stderr=subprocess.STDOUT,**kw);owned.append(p);return p
def run(args):return subprocess.run(args,env=env,check=True,capture_output=True,text=True,timeout=8)
def wait(fn,message,seconds=8):
 end=time.monotonic()+seconds
 while time.monotonic()<end:
  value=fn()
  if value:return value
  if app and app.poll() is not None:raise AssertionError('app exited: '+str(app.returncode))
  time.sleep(.02)
 raise AssertionError(message)
def state():
 try:return json.loads((root/'state.json').read_text())
 except (FileNotFoundError,json.JSONDecodeError):return {}
def at(path):
 s=state();m=s.get('model',{});c=m.get('columns',[])
 return s if not m.get('pending') and c and c[-1]['path']==str(path) else None
def key(*keys):run(['xdotool','key','--clearmodifiers',*keys])
def address(path):
 key('ctrl+l');run(['xdotool','type','--clearmodifiers','--delay','1',str(path)]);key('Return')
def snap(name):
 time.sleep(.2) # allow GTK's next frame to paint the committed model
 run(['import','-window',window,str(out/(name+'.png'))])
def click(x,y,count=1):
 time.sleep(.2) # committed model may precede GTK's frame/layout allocation
 run(['xdotool','mousemove','--window',window,str(x),str(y),'click','--repeat',str(count),'--delay','100','1'])
app=None
try:
 r,w=os.pipe();xlog=(out/'xvfb.log').open('w');logs.append(xlog)
 x=subprocess.Popen(['Xvfb','-displayfd',str(w),'-screen','0','1400x900x24','-noreset','-nolisten','tcp'],env=env,pass_fds=(w,),stdout=xlog,stderr=subprocess.STDOUT);owned.append(x);os.close(w)
 assert select.select([r],[],[],5)[0];env['DISPLAY']=':'+os.read(r,32).decode().strip();os.close(r)
 bus=subprocess.Popen(['dbus-daemon','--session','--nofork','--print-address=1'],env=env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True);owned.append(bus);env['DBUS_SESSION_BUS_ADDRESS']=bus.stdout.readline().strip();assert env['DBUS_SESSION_BUS_ADDRESS'].startswith('unix:')
 app=start([str(binary),str(base)],'app')
 wait(lambda:at(base),'initial folder did not load')
 window=wait(lambda:run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','Decebal Files']).stdout.strip().splitlines(),'no Files window')[0]
 run(['xdotool','windowfocus','--sync',window]);snap('windows-details')
 assert len(state()['model']['columns'][-1]['entries'])==6
 # Real pointer selection and keyboard activation in the details view.
 click(300,100);key('Home');key('Return')
 wait(lambda:at(base/'Alpha space'),'details row activation did not open first folder');cases.append('details keyboard folder activation')
 key('alt+Left');wait(lambda:at(base),'Back failed');key('alt+Right');wait(lambda:at(base/'Alpha space'),'Forward failed');key('alt+Up');wait(lambda:at(base),'Up failed');cases.append('back forward up')
 address(base/'missing');wait(lambda:state().get('model',{}).get('error'),'missing path error absent');assert at(base);cases.append('invalid path preserves previous folder')
 address(base/'Blocked');wait(lambda:state().get('model',{}).get('error'),'unreadable path error absent');assert at(base);cases.append('unreadable path preserves previous folder')
 # Test actual address semantics before GIO can lexically collapse link/.. .
 address(str(base/'link')+'/..');wait(lambda:at(physical),'symlink parent resolved to wrong folder');cases.append('symlink parent physical identity')
 address(base);wait(lambda:at(base),'return to fixture failed')
 # Select columns using the actual GTK dropdown, not a test-only model method.
 click(980,26);key('End');key('Return');wait(lambda:state().get('layout')=='columns','layout selector did not choose columns');snap('macos-columns-root')
 # First row, first column (single-click activation in column mode).
 click(285,100);wait(lambda:at(base/'Alpha space'),'column folder click did not navigate');assert len(state()['model']['columns'])==2
 click(530,100);wait(lambda:at(base/'Alpha space'/'Nested'),'second column click did not navigate');assert len(state()['model']['columns'])==3;cases.append('hierarchical columns append')
 # Click sibling Beta in the first column; ancestors must stay present.
 click(285,136);wait(lambda:at(base/'Beta'),'ancestor sibling click failed');assert len(state()['model']['columns'])==2;assert state()['model']['columns'][0]['path']==str(base);cases.append('ancestor column sibling preserves parent')
 snap('macos-columns-sibling')
 address(base);wait(lambda:at(base),'fixture address failed')
 raw_entry=next(e for e in state()['model']['columns'][-1]['entries'] if e['path_bytes']==list(raw));assert raw_entry['directory'] is False
 click(285,260);wait(lambda:state()['model']['columns'][-1]['selected_bytes']==list(raw),'non UTF8 file selection lost identity');assert at(base);cases.append('non UTF8 selection retained losslessly without execution')
 # Preserve a final source of truth and graceful close.
 (out/'state-final.json').write_text(json.dumps(state(),indent=2)+'\n')
 key('ctrl+w');app.wait(timeout=8);assert app.returncode==0
 source_root=Path(__file__).resolve().parents[1]
 sources={str(p.relative_to(source_root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source_root.rglob('*')) if p.is_file() and (p.suffix=='.rs' or p.name in ('Cargo.toml','Cargo.lock'))}
 result={'source_hashes':sources,'passed':True,'cases':cases,'binary':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'private_root':str(root),'scope':'X11 GTK actual controls, read-only navigation; no Orca/Wayland/operations acceptance'}
 (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
finally:
 (base/'Blocked').chmod(0o700)
 for p in reversed(owned):
  if p.poll() is None:p.terminate()
  try:p.wait(timeout=5)
  except subprocess.TimeoutExpired:p.kill();p.wait()
 for f in logs:f.close()
