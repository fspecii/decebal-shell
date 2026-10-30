#!/usr/bin/env python3
"""Real CLI/worker regression using only private paths, loopback HTTP and fake rdwm IPC."""
import contextlib,http.server,json,os,pathlib,signal,socket,struct,subprocess,tempfile,threading,time,zlib
ROOT=pathlib.Path(__file__).resolve().parents[2]
BIN=pathlib.Path(os.environ.get('MS_BIN_DIR',str(ROOT/'target'/os.environ.get('PROFILE','release'))))/'decthemectl'
assert BIN.is_file(), 'Build decthemectl before this test'
CAT=json.loads((ROOT/'data/wallhaven-curated.json').read_text())
def png():
 def chunk(kind,data):return struct.pack('!I',len(data))+kind+data+struct.pack('!I',zlib.crc32(kind+data)&0xffffffff)
 return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('!IIBBBBB',8,6,8,6,0,0,0))+chunk(b'IDAT',zlib.compress((b'\0'+b'\x0a\x78\xc8\xff'*8)*6))+chunk(b'IEND',b'')
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
 def __init__(self,code=200,ai=False,delay=False):
  self.temp=tempfile.TemporaryDirectory(prefix='decebal-follow-cli-');self.root=pathlib.Path(self.temp.name);self.events=[];self.applied=[];self.code=code;self.ai=ai;self.delay=delay;self.started=threading.Event();self.stop=threading.Event();self.wall={'image':'/old.jpg','mode':'fit','color':'#142527','set':True};self.lock=threading.Lock()
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
     if case.delay and id=='m9wr19':time.sleep(.6)
     data=json.dumps({'data':{'id':id,'purity':'sfw','category':'general','dimension_x':8,'dimension_y':6,'file_type':'image/png','path':case.base+'/full/'+id+'.png','thumbs':{'small':case.base+'/thumb.png','large':case.base+'/thumb.png'},'ai_art':case.ai,'tags':[{'id':1,'name':'photography'}]}}).encode();code=case.code;kind='application/json'
    else:data=png();code=200;kind='image/png'
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
     request=json.loads(data)
     with self.lock:
      if request['cmd']=='get_state':reply={'ok':True,'state':{'wallpaper':self.wall.copy(),'monitors':[],'windows':[]}}
      elif request['cmd']=='wallpaper':self.wall.update(image=request['image'],mode=request.get('mode','fill'));self.applied.append(request['image']);reply={'ok':True}
      elif request['cmd']=='get_config':reply={'ok':True,'config':{}}
      else:reply={'ok':True}
     stream.sendall(json.dumps(reply).encode()+b'\n')
    except (OSError,ValueError):pass
 def run(self,*args):
  p=subprocess.run([str(BIN),*args],env=self.env,text=True,capture_output=True,timeout=15)
  assert p.returncode==0,(args,p.stdout,p.stderr)
  return p.stdout
 def status(self):return json.loads((self.shell/'wallpaper-follow/status.json').read_text())
 def done(self):return wait(lambda:(s if not s['pending'] else None) if (s:=self.status()) else None,'wallpaper worker did not finish')
 def close(self):
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
