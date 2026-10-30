#!/usr/bin/env python3
"""Private EWMH title-only redraw regression; no live display/input/probes.
BAR_BINARY=/absolute/decbar python3 ... /absolute/dux-package /tmp/evidence
Set EXPECT_STALE=1 to record the unfixed baseline.
"""
import ctypes as C
import hashlib,json,os,subprocess,sys,tempfile,time,shutil
from pathlib import Path
from PIL import ImageGrab
binary=Path(os.environ['BAR_BINARY']).resolve();package=Path(sys.argv[1]).resolve();out=Path(sys.argv[2]).resolve();out.mkdir(parents=True,exist_ok=True)
def wait(fn,label,seconds=10):
 end=time.monotonic()+seconds
 while time.monotonic()<end:
  try:
   value=fn()
   if value:return value
  except (OSError,ValueError,subprocess.CalledProcessError):pass
  time.sleep(.04)
 raise AssertionError(label)
with tempfile.TemporaryDirectory(prefix='dux-title-redraw-') as tmp:
 root=Path(tmp);env=dict(os.environ)
 for k in list(env):
  if k.startswith(('DECEBAL_','DECBAR_','XDG_','DBUS_','RDWM')) or k in ('DISPLAY','WAYLAND_DISPLAY','HOME','XAUTHORITY','AT_SPI_BUS_ADDRESS','SESSION_MANAGER'):env.pop(k)
 for k in ('HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_STATE_HOME','XDG_CACHE_HOME','XDG_RUNTIME_DIR','XDG_DATA_DIRS','XDG_CONFIG_DIRS','DECEBAL_CONFIG','DECEBAL_THEMES','DECEBAL_SYSTEM_THEMES'):
  p=root/k;p.mkdir(mode=0o700);env[k]=str(p)
 shutil.copytree(package,Path(env['DECEBAL_THEMES'])/'dux-lavender')
 (Path(env['DECEBAL_CONFIG'])/'decebal.json').write_text(json.dumps({'theme':{'name':'dux-lavender','mode':'dark'},'bar':{'enabled':True,'left':['focusedWindow'],'center':[],'right':[]}}))
 (root/'bin').mkdir()
 for name in ('curl','nmcli','pactl','wpctl','playerctl','bluetoothctl','brightnessctl','decutil','rdwmctl'):
  p=root/'bin'/name;p.write_text('#!/bin/sh\nexit 1\n');p.chmod(0o755)
 display=next(f':{n}' for n in range(700,740) if not Path(f'/tmp/.X11-unix/X{n}').exists() and not Path(f'/tmp/.X{n}-lock').exists())
 env.update(DECBAR_DEBUG_TRAY=str(root/'tray.txt'),DISPLAY=display,DECEBAL_BACKEND='x11',DECEBAL_SYSTEM_AUTOMATIC='0',DBUS_SESSION_BUS_ADDRESS='unix:path='+tmp+'/no-session',DBUS_SYSTEM_BUS_ADDRESS='unix:path='+tmp+'/no-system',PATH=str(root/'bin')+':'+os.environ['PATH'])
 x=C.CDLL('libX11.so.6');P=C.c_void_p;U=C.c_ulong
 x.XOpenDisplay.argtypes=[C.c_char_p];x.XOpenDisplay.restype=P;x.XDefaultRootWindow.argtypes=[P];x.XDefaultRootWindow.restype=U
 x.XCreateSimpleWindow.argtypes=[P,U,C.c_int,C.c_int,C.c_uint,C.c_uint,C.c_uint,U,U];x.XCreateSimpleWindow.restype=U
 x.XInternAtom.argtypes=[P,C.c_char_p,C.c_int];x.XInternAtom.restype=U
 x.XChangeProperty.argtypes=[P,U,U,U,C.c_int,C.c_int,P,C.c_int];x.XMapWindow.argtypes=[P,U];x.XSync.argtypes=[P,C.c_int];x.XCloseDisplay.argtypes=[P]
 app=None;conn=None
 with (out/'native.log').open('w') as log:
  server=subprocess.Popen(['Xvfb',display,'-screen','0','1280x720x24','-nolisten','tcp','-noreset'],env=env,stdout=log,stderr=log)
  try:
   wait(lambda:server.poll() is None and Path(f'/tmp/.X{display[1:]}-lock').exists() and int(Path(f'/tmp/.X{display[1:]}-lock').read_text().strip())==server.pid,'ownedXvfb')
   conn=wait(lambda:x.XOpenDisplay(display.encode()),'Xconnection');r=x.XDefaultRootWindow(conn);w=x.XCreateSimpleWindow(conn,r,100,100,500,300,0,0,0);x.XMapWindow(conn,w)
   def atom(name):return x.XInternAtom(conn,name.encode(),0)
   def prop32(window,name,kind,values):
    data=(U*len(values))(*values);x.XChangeProperty(conn,window,atom(name),atom(kind),32,0,C.cast(data,P),len(values));x.XSync(conn,0)
   def prop8(window,name,kind,value):
    data=C.create_string_buffer(value);x.XChangeProperty(conn,window,atom(name),atom(kind),8,0,C.cast(data,P),len(value));x.XSync(conn,0)
   prop8(w,'WM_CLASS','STRING',b'fixture\0Fixture\0');prop8(w,'_NET_WM_NAME','UTF8_STRING',b'AAAAAAAAAAAAAAAAAAAA')
   prop32(r,'_NET_CLIENT_LIST','WINDOW',[w]);prop32(r,'_NET_ACTIVE_WINDOW','WINDOW',[w]);prop32(r,'_NET_NUMBER_OF_DESKTOPS','CARDINAL',[1]);prop32(r,'_NET_CURRENT_DESKTOP','CARDINAL',[0]);prop8(r,'_NET_DESKTOP_NAMES','UTF8_STRING',b'1\0')
   app=subprocess.Popen([str(binary)],env=env,stdout=log,stderr=log)
   def run(*args):assert server.poll() is None;return subprocess.check_output(args,env=env,text=True,stderr=subprocess.DEVNULL).strip()
   wait(lambda:run('xdotool','search','--name','^decebal-bar$'),'barwindow')
   wait(lambda:(root/'tray.txt').exists() and 'Window ' in (root/'tray.txt').read_text(),'title painted')
   time.sleep(2)
   # Avoid a minute rollover causing a coincidental redraw in the baseline.
   if time.localtime().tm_sec>53:time.sleep(8)
   def frame():return ImageGrab.grab(xdisplay=display).convert('RGB').crop((0,0,600,46))
   before=frame();before.save(out/'before.png');time.sleep(.3);assert before.tobytes()==frame().tobytes(),'initialframeunstable'
   prop8(w,'_NET_WM_NAME','UTF8_STRING',b'ZZZZZZZZZZZZZZZZZZZZ')
   start=time.monotonic();changed=False
   while time.monotonic()-start<3:
    after=frame()
    if after.tobytes()!=before.tobytes():changed=True;break
    time.sleep(.04)
   after.save(out/'after.png');result={'binary_sha':hashlib.sha256(binary.read_bytes()).hexdigest(),'title_only_redraw':changed,'seconds':time.monotonic()-start,'same_app_id':True,'display':display};(out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
   assert app.poll() is None,'barcrashed'
   assert changed != (os.environ.get('EXPECT_STALE')=='1'),result
   print(json.dumps(result))
  finally:
   if app is not None and app.poll() is None:app.terminate();app.wait(timeout=5)
   if conn:x.XCloseDisplay(conn)
   server.terminate();server.wait(timeout=5)
