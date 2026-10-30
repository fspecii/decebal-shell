#!/usr/bin/env python3
"""Private native window.* group replacement regression; no media worker is involved.
WIDGETS_BIN=/absolute/decwidgets EVIDENCE=/tmp/fresh-dir [EXPECT_LEAK=1] python3 ...
"""
import hashlib,json,os,subprocess,tempfile,time
from pathlib import Path
binary=Path(os.environ['WIDGETS_BIN']).resolve();out=Path(os.environ['EVIDENCE']);out.mkdir(parents=True,exist_ok=True)
processes=[];samples=[]
def wait(fn):
 end=time.monotonic()+8
 while time.monotonic()<end:
  if fn():return
  time.sleep(.05)
 raise AssertionError('private host did not reach expected state')
with tempfile.TemporaryDirectory(prefix='window-tracker-life-') as temp:
 root=Path(temp)
 env={k:v for k,v in os.environ.items() if not k.startswith(('XDG_','DBUS_','RDWM','DECEBAL_','WAYLAND','DECSETTINGS_'))}
 for key in ('XAUTHORITY','SESSION_MANAGER','AT_SPI_BUS_ADDRESS','HYPRLAND_INSTANCE_SIGNATURE'):env.pop(key,None)
 for key,name in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_DATA_HOME','data'),('XDG_STATE_HOME','state'),('XDG_RUNTIME_DIR','run'),('XDG_CONFIG_DIRS','nocfg'),('XDG_DATA_DIRS','nodata'),('DECEBAL_CONFIG','shell'),('DECEBAL_WIDGETS','widgets'),('DECEBAL_THEMES','themes')]:
  (root/name).mkdir(mode=0o700);env[key]=str(root/name)
 n=next(n for n in range(610,710) if not Path(f'/tmp/.X11-unix/X{n}').exists() and not Path(f'/tmp/.X{n}-lock').exists())
 env.update(DISPLAY=f':{n}',DECEBAL_BACKEND='x11',XDG_SESSION_TYPE='x11',DECEBAL_SYSTEM_AUTOMATIC='0',DBUS_SESSION_BUS_ADDRESS=f'unix:path={root}/no-session',DBUS_SYSTEM_BUS_ADDRESS=f'unix:path={root}/no-system',RDWM_SOCK=f'{root}/no-wm')
 (root/'widgets/window-life.toml').write_text('format = 1\nid = "window-life"\nname = "Window lifetime"\nsize = [220, 80]\nrefresh = 1000\n[root]\ntype = "text"\ntext = "{window.title|No active window}"\nsize = 16\ncolor = "text"\n')
 def cmd(*args):return subprocess.run(args,env=env,text=True,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL).stdout.strip()
 salt=0
 def config(enabled):
  global salt
  salt+=1;p=root/'shell/decebal.json';tmp=p.with_suffix('.new');tmp.write_text(json.dumps({'theme':{'name':'macos','mode':'dark'},'widgets':{'enabled':enabled,'items':[{'use':'window-life','place':'desktop','anchor':'top-left','x':20,'y':20}]}}));os.replace(tmp,p);t=time.time()+salt;os.utime(p,(t,t))
 def windows():return cmd('xdotool','search','--name','^decwidgets: ').split()
 def measure(host,label):
  assert host.poll() is None
  samples.append({'label':label,'threads':len(list(Path(f'/proc/{host.pid}/task').iterdir())),'fds':len(list(Path(f'/proc/{host.pid}/fd').iterdir()))})
 try:
  log=(out/'host.log').open('w');xlog=(out/'xvfb.log').open('w');x=subprocess.Popen(['Xvfb',env['DISPLAY'],'-screen','0','640x480x24','-nolisten','tcp'],env=env,stdout=xlog,stderr=xlog);processes.append(x)
  def ready():
   assert x.poll() is None
   p=Path(f'/tmp/.X{n}-lock');return p.exists() and int(p.read_text().strip())==x.pid
  wait(ready);config(False)
  host=subprocess.Popen([str(binary),'daemon'],env=env,stdout=log,stderr=log);processes.append(host);time.sleep(.6);measure(host,'disabled-before')
  for i in range(4):
   config(True);wait(lambda:len(windows())==1);time.sleep(.15);measure(host,f'enabled-{i}')
   config(False);wait(lambda:len(windows())==0);time.sleep(.4);measure(host,f'disabled-{i}')
  leak=samples[-1]['threads']>samples[0]['threads'] or samples[-1]['fds']>samples[0]['fds']
  result={'binary':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'samples':samples,'leak':leak,'expected_leak':os.environ.get('EXPECT_LEAK')=='1'}
  (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
  assert leak==result['expected_leak'],'collector lifetime regression'
 finally:
  for p in reversed(processes):
   if p.poll() is None:
    p.terminate()
    try:p.wait(timeout=3)
    except subprocess.TimeoutExpired:p.kill();p.wait()
