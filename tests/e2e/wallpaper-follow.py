#!/usr/bin/env python3
"""Real CLI/worker regression using only private paths, loopback HTTP and fake rdwm IPC."""
import contextlib,http.server,json,os,pathlib,signal,socket,struct,subprocess,tempfile,threading,time,zlib
ROOT=pathlib.Path(__file__).resolve().parents[2]
BIN=pathlib.Path(os.environ.get('MS_BIN_DIR',str(ROOT/'target'/os.environ.get('PROFILE','release'))))/'decthemectl'
assert BIN.is_file(), 'Build decthemectl before this test'
CAT=json.loads((ROOT/'data/wallhaven-curated.json').read_text())
def png(rgb=(10,120,200)):
 def chunk(kind,data):return struct.pack('!I',len(data))+kind+data+struct.pack('!I',zlib.crc32(kind+data)&0xffffffff)
 return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('!IIBBBBB',8,6,8,6,0,0,0))+chunk(b'IDAT',zlib.compress((b'\0'+bytes((*rgb,255))*8)*6))+chunk(b'IEND',b'')
def wait(fn,why):
 end=time.monotonic()+12
 while time.monotonic()<end:
  try:
   value=fn()
   if value:return value
  except (FileNotFoundError,json.JSONDecodeError,KeyError,ProcessLookupError):pass
  time.sleep(.025)
 raise AssertionError(why)
class Case:
 def __init__(self,code=200,ai=False,delay=False,block_refresh_write=False,photo_rgb=(10,120,200),pause_colour_input=False,delay_all=False):
  self.temp=tempfile.TemporaryDirectory(prefix='decebal-follow-cli-');self.root=pathlib.Path(self.temp.name);self.events=[];self.applied=[];self.pause_colour_input=pause_colour_input;self.colour_probe=threading.Event();self.release_colours=threading.Event();self.gate_timed_out=False;self.block_refresh_write=block_refresh_write;self.photo_rgb=photo_rgb;self.delay_all=delay_all;self.code=code;self.ai=ai;self.delay=delay;self.started=threading.Event();self.stop=threading.Event();self.wall={'image':'/old.jpg','mode':'fit','color':'#142527','set':True};self.lock=threading.Lock()
  self.env=os.environ.copy()
  for key in ['DISPLAY','WAYLAND_DISPLAY','RDWM_SOCK','DBUS_STARTER_ADDRESS','DBUS_STARTER_BUS_TYPE','AT_SPI_BUS_ADDRESS','DECBAR_DEBUG_TRAY','DECBAR_DEBUG_INDICATORS','GTK_THEME']:self.env.pop(key,None)
  for key,folder in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_STATE_HOME','state'),('XDG_CACHE_HOME','cache'),('XDG_DATA_HOME','data'),('XDG_RUNTIME_DIR','run')]:
   p=self.root/folder;p.mkdir(mode=0o700);self.env[key]=str(p)
  self.shell=self.root/'shell';self.shell.mkdir();self.config=self.root/'rdwm.toml';self.sock=self.root/'wm.sock'
  self.env.update(DECEBAL_CONFIG=str(self.shell),DECSETTINGS_CONFIG=str(self.config),RDWM_CONFIG=str(self.config),RDWM_SOCK=str(self.sock),RDWMCTL='/bin/true',DECEBAL_THEMES=str(self.root/'themes'),DECEBAL_SYSTEM_THEMES=str(self.root/'no-system-themes'),DECEBAL_SYSTEM_AUTOMATIC='0',DBUS_SESSION_BUS_ADDRESS='unix:path='+str(self.root/'missing-session'),DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(self.root/'missing-system'),XDG_DATA_DIRS=str(self.root/'no-data'),XDG_CONFIG_DIRS=str(self.root/'no-config'))
  (self.shell/'decebal.json').write_text(json.dumps({'theme':{'name':'tidal-loom','mode':'dark'},'wallpaper':{'followTheme':False},'system':{'enabled':False},'dock':{'enabled':False},'widgets':{'enabled':False,'items':[]},'unrelated':'keep'}))
  self.config.write_text('# independent fixture\n[general]\nlayouts=["tile"]\n[wallpaper]\nimage="/old.jpg"\nmode="fit"\ncolor="#142527"\n')
  self.listener=socket.socket(socket.AF_UNIX);self.listener.bind(str(self.sock));self.listener.listen();self.listener.settimeout(.1)
  self.ipc=threading.Thread(target=self.serve_ipc,daemon=True);self.ipc.start()
  case=self
  class HTTP(http.server.BaseHTTPRequestHandler):
   def log_message(self,*args):pass
   def do_GET(self):
    case.events.append(self.path)
    if '/w/' in self.path:
     id=self.path.rsplit('/',1)[-1];case.started.set()
     if case.delay and (id=='m9wr19' or case.delay_all):time.sleep(.6)
     data=json.dumps({'data':{'id':id,'purity':'sfw','category':'general','dimension_x':8,'dimension_y':6,'file_type':'image/png','path':case.base+'/full/'+id+'.png','thumbs':{'small':case.base+'/thumb.png','large':case.base+'/thumb.png'},'ai_art':case.ai,'tags':[{'id':1,'name':'photography'}]}}).encode();code=case.code;kind='application/json'
    else:data=png(case.photo_rgb);code=200;kind='image/png'
    try:self.send_response(code);self.send_header('Content-Type',kind);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
    except (BrokenPipeError,ConnectionResetError):pass
  self.http=http.server.ThreadingHTTPServer(('127.0.0.1',0),HTTP);self.http.daemon_threads=True;self.base='http://127.0.0.1:'+str(self.http.server_port);self.http_thread=threading.Thread(target=self.http.serve_forever,daemon=True);self.http_thread.start();self.env['WALLHAVEN_API_BASE']=self.base
 def serve_ipc(self):
  while not self.stop.is_set():
   try:stream,_=self.listener.accept()
   except socket.timeout:continue
   except OSError:break
   with stream:
    stream.settimeout(2)
    try:
     data=b''
     while not data.endswith(b'\n'):
      block=stream.recv(65536)
      if not block:break
      data+=block
     if not data:continue
     request=json.loads(data);pause=False
     with self.lock:
      if request['cmd']=='get_state':
       reply={'ok':True,'state':{'wallpaper':self.wall.copy(),'monitors':[],'windows':[]}}
       if self.pause_colour_input and self.applied and not self.colour_probe.is_set():pause=True;self.colour_probe.set()
      elif request['cmd']=='wallpaper':
       self.wall.update(image=request['image'],mode=request.get('mode','fill'));self.applied.append(request['image']);reply={'ok':True}
       if self.block_refresh_write:self.shell.chmod(0o500)
      elif request['cmd']=='get_config':reply={'ok':True,'config':{}}
      else:reply={'ok':True}
     if pause and not self.release_colours.wait(5):self.gate_timed_out=True
     stream.sendall(json.dumps(reply).encode()+b'\n')
    except (OSError,ValueError):pass
 def run(self,*args):
  p=subprocess.run([str(BIN),*args],env=self.env,text=True,capture_output=True,timeout=15)
  assert p.returncode==0,(args,p.stdout,p.stderr)
  return p.stdout
 def status(self):return json.loads((self.shell/'wallpaper-follow/status.json').read_text())
 def done(self):return wait(lambda:(s if not s['pending'] else None) if (s:=self.status()) else None,'wallpaper worker did not finish')
 def close(self):
  self.release_colours.set()
  self.shell.chmod(0o700)
  try:
   pid=self.status().get('pid')
   if pid:
    command=pathlib.Path('/proc/'+str(pid)+'/cmdline').read_bytes()
    if b'--wallpaper-follow-worker\0' in command and str(self.shell).encode() in command:os.killpg(pid,signal.SIGTERM)
  except (FileNotFoundError,ProcessLookupError,PermissionError):pass
  self.stop.set();self.listener.close();self.ipc.join(3);self.http.shutdown();self.http.server_close();self.http_thread.join(3);self.temp.cleanup()
@contextlib.contextmanager
def case(**kwargs):
 c=Case(**kwargs)
 try:yield c
 finally:c.close()
with case() as c:
 assert json.loads(c.run('wallpaper','follow','status','--json'))['enabled'] is False
 c.run('wallpaper','follow','on');s=c.done();assert not s['error'],s;assert c.wall['image'].endswith('wallhaven-m9wr19.png');assert len(c.events)==2
 c.run('apply','windows11','--keep-widgets');s=c.done();assert not s['error'],s
 nord=CAT['themes']['windows11']['dark']['image_id'];assert c.wall['image'].endswith('wallhaven-'+nord+'.png')
 before=len(c.events);c.run('apply','tidal-loom','--keep-widgets');s=c.done();assert not s['error'] and 'cached' in s['message'];assert len(c.events)==before
 c.wall['image']='/manual-runtime.jpg';c.run('apply','tidal-loom','--keep-widgets');s=c.done();assert not s['error'],s;assert c.wall['image'].endswith('wallhaven-m9wr19.png')
 c.run('set','mode','light');s=c.done();assert not s['error'],s;assert c.wall['image'].endswith('wallhaven-3qqd26.png')
 before=len(c.events);wall=c.wall.copy();c.run('theme','apply','nord','--preview','1');assert c.wall==wall and len(c.events)==before;assert json.loads((c.shell/'decebal.json').read_text())['theme']['name']=='tidal-loom'
 c.run('wallpaper','follow','off');before=len(c.events);c.run('apply','windows11','--keep-widgets');assert c.wall==wall and len(c.events)==before
 assert json.loads((c.shell/'decebal.json').read_text())['unrelated']=='keep'
 print('PASS real CLI/worker on, full theme/mode follow, validated cache reuse, preview no requests, off independence')
for kwargs in [{'code':403},{'ai':True}]:
 with case(**kwargs) as c:
  before=c.config.read_bytes();c.run('wallpaper','follow','on');s=c.done();assert s['error'] and 'wallpaper unchanged' in s['message'];assert c.config.read_bytes()==before and c.wall['image']=='/old.jpg';assert len(c.events)==1
print('PASS403 and excluded-AI metadata preserve selected wallpaper with visible failure state')
with case(delay=True) as c:
 c.run('wallpaper','follow','on');assert c.started.wait(3);c.run('apply','windows11','--keep-widgets');s=c.done();assert not s['error'],s;time.sleep(.7)
 expected=CAT['themes']['windows11']['dark']['image_id'];assert c.wall['image'].endswith('wallhaven-'+expected+'.png');assert len(c.applied)==1
 assert not list((c.shell/'wallpaper-follow').glob('download-*'))
 print('PASS superseded slow worker cannot apply old photo; staging cleaned')

with case(delay=True) as c:
 c.run('wallpaper','follow','on');assert c.started.wait(3);c.run('palette','nord');s=c.done();assert not s['error'],s
 assert c.wall['image'].endswith('wallhaven-m9wr19.png')
 assert json.loads((c.shell/'decebal.json').read_text())['theme']['palette']=='nord'
 print('PASS slow photo completion preserves a newer independent Nord palette choice')


# Post-photo colour matching is independent of selecting a desktop design. A
# previous live-wallpaper poster must not remain the active colour-cache image.
DYNAMIC_CACHE_KEYS={'seed','tone','accents','grey','image'}
def selection(c):return json.loads((c.shell/'decebal.json').read_text())['theme']
def preferences(theme):
 return {**{k:v for k,v in theme.items() if k!='dynamic'},
         'dynamic':{k:v for k,v in theme.get('dynamic',{}).items() if k not in DYNAMIC_CACHE_KEYS}}
def colour_cache(c):
 path=pathlib.Path(c.env['XDG_CACHE_HOME'])/'decebal/wallpaper-colors.json'
 return path.read_bytes() if path.exists() else None
def setup_dynamic(c,source='wallpaper',enabled=True):
 old=c.root/'old-live-poster.png';old.write_bytes(png((150,25,35)))
 c.wall['image']=str(old)
 c.config.write_text(c.config.read_text().replace('/old.jpg',str(old)))
 if source=='fixed-image':
  fixed=c.root/'fixed-colour-source.png';fixed.write_bytes(png((20,85,35)));chosen=str(fixed)
 elif source=='fixed-colour':chosen='#235d39'
 else:chosen='wallpaper'
 c.run('dynamic','on','--source',chosen,'--scheme','expressive','--surfaces','full',
       '--strength','73','--contrast','15','--mode','wallpaper','--no-rdwm')
 c.run('set','radius','24');c.run('set','opacity','0.84')
 if not enabled:c.run('dynamic','off','--no-rdwm')
 # Preserve even a legacy accepted configuration containing a palette together
 # with enabled matching. The helper must not silently normalize user choices.
 doc=json.loads((c.shell/'decebal.json').read_text());doc['theme']['palette']='nord'
 (c.shell/'decebal.json').write_text(json.dumps(doc))
 return selection(c),old

with case(photo_rgb=(180,220,245)) as c:
 before,old=setup_dynamic(c)
 assert before['dynamic']['enabled'] and before['dynamic']['image']==str(old)
 c.run('wallpaper','follow','on');s=c.done();assert not s['error'],s
 after=selection(c);photo=c.wall['image']
 assert photo!=str(old) and after['dynamic']['image']==photo,(before,after,s)
 assert after['dynamic']['seed']!=before['dynamic']['seed'],'new photo kept old poster colours'
 assert before['dynamic']['tone']<55<=after['dynamic']['tone'], 'fixture did not exercise wallpaper-driven dark/light change'
 assert preferences(after)==preferences(before),'colour refresh changed selected design or user matching preferences'
 assert after['name']=='tidal-loom' and after['palette']=='nord' and after['radius']==24
 cache=json.loads(colour_cache(c));assert any(e['path']==photo for e in cache['entries'])
 assert len(c.applied)==1 and len(c.events)==2,'colour refresh recursively followed another photo'
 print('PASS post-photo Dynamic wallpaper cache refresh preserves theme/palette/radius and matching preferences')

for source,enabled in [('fixed-image',True),('fixed-colour',True),('wallpaper',False)]:
 with case() as c:
  before,old=setup_dynamic(c,source,enabled);cache=colour_cache(c)
  c.run('wallpaper','follow','on');s=c.done();assert not s['error'],s
  assert c.wall['image']!=str(old),'matching theme photo was not applied'
  assert selection(c)==before,(source,'photo follow changed a fixed or disabled colour source')
  assert colour_cache(c)==cache,(source,'photo follow rewrote a fixed/disabled colour cache')
  assert len(c.applied)==1 and len(c.events)==2
print('PASS fixed-image/fixed-colour sources and disabled Dynamic matching remain unchanged')

with case(delay=True) as c:
 setup_dynamic(c)
 c.run('wallpaper','follow','on');assert c.started.wait(3)
 c.run('palette','nord');newer=selection(c)
 assert newer['palette']=='nord' and not newer['dynamic']['enabled']
 s=c.done();assert not s['error'],s
 assert selection(c)==newer,'inflight photo overwrote a newer palette/disabled matching choice'
 assert len(c.applied)==1
 print('PASS inflight photo completion honours a newer palette choice which disables Dynamic matching')

# Gate the first colour-source lookup after the fake WM accepts the photo.
# The worker already captured its initial Dynamic input, but has not completed
# prewarming. This verifies latest-selection rereads at transaction start, not
# universal serialization of concurrent edits inside the subsequent transaction.
for edit in ['palette','source']:
 with case(pause_colour_input=True) as c:
  setup_dynamic(c)
  c.run('wallpaper','follow','on');assert c.colour_probe.wait(3),'post-photo colour lookup was not reached'
  if edit=='palette':c.run('palette','nord')
  else:
   fixed=c.root/'new-fixed-source.png';fixed.write_bytes(png((15,80,35)))
   c.run('dynamic','source',str(fixed))
  newer=selection(c)
  assert not c.gate_timed_out,'fixture could not edit preferences while the initial colour lookup was gated'
  c.release_colours.set();s=c.done();assert not s['error'],s
  assert selection(c)==newer,(edit,'post-prewarm refresh overwrote newer source/palette preferences')
  assert len(c.applied)==1 and len(c.events)==2
print('PASS latest source/palette edits during gated colour prewarm are respected at refresh transaction start')

if os.geteuid()!=0:
 with case(block_refresh_write=True) as c:
  before,old=setup_dynamic(c)
  c.run('wallpaper','follow','on');s=c.done()
  assert len(c.applied)==1 and c.wall['image']!=str(old),'failure fixture never applied the photo'
  assert c.applied[0] in c.config.read_text(),'colour error rolled back the already applied wallpaper config'
  assert s['error'] and 'wallpaper applied' in s['message'].lower() and 'colour refresh failed' in s['message'].lower(),s
  assert 'wallpaper unchanged' not in s['message'].lower() and 'previous wallpaper kept' not in s['message'].lower(),s
  assert selection(c)==before,'failed colour refresh changed cached colours or user preferences'
  c.shell.chmod(0o700);c.block_refresh_write=False;requests=len(c.events)
  c.run('wallpaper','follow','retry');recovered=c.done();assert not recovered['error'],recovered
  after=selection(c);assert after['dynamic']['image']==c.wall['image'] and after['dynamic']['seed']!=before['dynamic']['seed']
  assert preferences(after)==preferences(before) and len(c.events)==requests,'colour recovery lost preferences or fetched the same photo again'
  print('PASS post-photo colour persistence failure reports partial success and Retry recovers from validated cache')
else:print('SKIP colour-write EACCES stage: requires an unprivileged test uid')


# Exercise an actual earlier installed producer, but only against this fixture.
# Capture its job while its own private worker waits on loopback metadata, stop
# only that verified fixture worker, then feed the unchanged JSON to the new one.
old_producer=pathlib.Path(os.environ.get('WALLPAPER_OLD_PRODUCER',str(pathlib.Path.home()/'.local/share/rdwm-decebal/bin/decthemectl')))
if old_producer.is_file():
 with case(delay=True,delay_all=True) as c:
  doc=json.loads((c.shell/'decebal.json').read_text());doc['theme']={'name':'macos','mode':'dark'}
  (c.shell/'decebal.json').write_text(json.dumps(doc));before,old=setup_dynamic(c)
  previous=subprocess.run([str(old_producer),'wallpaper','follow','on'],env=c.env,text=True,capture_output=True,timeout=15)
  assert previous.returncode==0,('old producer failed in isolated fixture',previous.stdout,previous.stderr)
  assert c.started.wait(3),'old producer did not start its private worker'
  state=c.status();job=c.shell/'wallpaper-follow'/(state['generation']+'.json');legacy_bytes=job.read_bytes()
  pid=state['pid'];args=pathlib.Path('/proc/'+str(pid)+'/cmdline').read_bytes().split(b'\0')
  assert b'--wallpaper-follow-worker' in args and os.fsencode(job) in args,'refusing to stop an unrelated process'
  if os.getpgid(pid)==pid:os.killpg(pid,signal.SIGTERM)
  else:os.kill(pid,signal.SIGTERM)
  def stopped():
   path=pathlib.Path('/proc/'+str(pid)+'/stat')
   return not path.exists() or path.read_text().split()[2]=='Z'
  wait(stopped,'verified old fixture worker did not stop')
  assert not c.applied and job.read_bytes()==legacy_bytes,'old worker applied or modified the captured job'
  c.run('--wallpaper-follow-worker',str(job));s=c.done();assert not s['error'],s
  after=selection(c)
  assert len(c.applied)==1 and after['dynamic']['image']==c.wall['image'] and after['dynamic']['seed']!=before['dynamic']['seed']
  assert preferences(after)==preferences(before),'new worker changed old producer preferences'
  print('PASS prior installed producer job JSON is accepted unchanged by the new worker and refreshes colours')
else:print('SKIP old-producer compatibility: set WALLPAPER_OLD_PRODUCER to the prior deployed helper')
