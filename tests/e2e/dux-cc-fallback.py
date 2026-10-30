#!/usr/bin/env python3
"""A Dux-look standalone CC remains legible with a classic theme and no package fonts.
BAR_BINARY=/absolute/decbar python3 ... /tmp/evidence. Private Xvfb and fake tools.
"""
from pathlib import Path
import os,json,subprocess,tempfile,time,sys,hashlib
from PIL import ImageGrab
out=Path(sys.argv[1]).resolve();out.mkdir(parents=True,exist_ok=True);binary=Path(os.environ['BAR_BINARY']).resolve()
def wait(fn,label):
 for _ in range(200):
  try:
   v=fn()
   if v:return v
  except (OSError,ValueError,subprocess.CalledProcessError):pass
  time.sleep(.05)
 raise AssertionError(label)
with tempfile.TemporaryDirectory(prefix='dux-cc-fallback-') as tmp:
 r=Path(tmp);env=dict(os.environ)
 for k in list(env):
  if k.startswith(('DECEBAL_','DECBAR_','XDG_','DBUS_','RDWM')) or k in ('DISPLAY','WAYLAND_DISPLAY','HOME','XAUTHORITY','AT_SPI_BUS_ADDRESS','SESSION_MANAGER'):env.pop(k)
 for k in ('HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_STATE_HOME','XDG_CACHE_HOME','XDG_RUNTIME_DIR','XDG_DATA_DIRS','XDG_CONFIG_DIRS','DECEBAL_CONFIG','DECEBAL_THEMES','DECEBAL_SYSTEM_THEMES'):
  p=r/k;p.mkdir(mode=0o700);env[k]=str(p)
 (Path(env['DECEBAL_CONFIG'])/'decebal.json').write_text(json.dumps({'theme':{'name':'macos','mode':'dark'},'control_center':{'style':'dux'}}))
 (r/'bin').mkdir()
 settings=r/'bin/decsettings';settings.write_text('#!/bin/sh\necho settings >> \"$DUX_FALLBACK_CALLS\"\n');settings.chmod(0o755)
 for name in ('curl','nmcli','pactl','wpctl','playerctl','bluetoothctl','brightnessctl','decutil','rdwmctl'):
  p=r/'bin'/name;p.write_text('#!/bin/sh\nexit 1\n');p.chmod(0o755)
 display=next(f':{n}' for n in range(740,780) if not Path(f'/tmp/.X11-unix/X{n}').exists() and not Path(f'/tmp/.X{n}-lock').exists())
 env.update(DUX_FALLBACK_CALLS=str(r/'calls'),DISPLAY=display,DECEBAL_BACKEND='x11',DECEBAL_SYSTEM_AUTOMATIC='0',DBUS_SESSION_BUS_ADDRESS='unix:path='+tmp+'/no-session',DBUS_SYSTEM_BUS_ADDRESS='unix:path='+tmp+'/no-system',PATH=str(r/'bin')+':'+os.environ['PATH'],DECBAR_DEBUG_PANEL=str(r/'panel.txt'))
 with (out/'native.log').open('w') as log:
  server=subprocess.Popen(['Xvfb',display,'-screen','0','960x720x24','-nolisten','tcp','-noreset'],env=env,stdout=log,stderr=log);app=None
  try:
   wait(lambda:server.poll() is None and Path(f'/tmp/.X{display[1:]}-lock').exists() and int(Path(f'/tmp/.X{display[1:]}-lock').read_text().strip())==server.pid,'owned Xvfb')
   app=subprocess.Popen([str(binary),'--control-center','open'],env=env,stdout=log,stderr=log)
   wait(lambda:(r/'panel.txt').exists() and (r/'panel.txt').read_text().startswith('panel '),'panel painted')
   rect=list(map(int,(r/'panel.txt').read_text().splitlines()[0].split()[1:]));x,y,w,h=rect
   def painted():
    image=ImageGrab.grab(xdisplay=display).convert('RGB')
    return image if image.getbbox() else None
   image=wait(painted,'panel presented');image.save(out/'classic-theme-dux-look.png')
   checks={}
   for label,cx,cy in [('person',52,52),('settings',556,52),('network',52,128)]:
    crop=image.crop((x+cx-10,y+cy-10,x+cx+10,y+cy+10));checks[label]=len(set(crop.get_flattened_data()))>1
    crop.save(out/(label+'.png'))
    (out/'result.json').write_text(json.dumps({'sha':hashlib.sha256(binary.read_bytes()).hexdigest(),'theme':'macos','package_fonts':False,'icons':checks,'rect':rect},indent=2)+'\n')
    assert checks[label],label+' icon blank without package fonts'
   assert app.poll() is None
   subprocess.run(['xdotool','key','Return'],env=env,check=True);time.sleep(.15)
   assert app.poll() is None and not (r/'calls').exists(),'unfocused Enter launched an invisible target'
   subprocess.run(['xdotool','key','Down','Return'],env=env,check=True)
   wait(lambda:(r/'calls').exists(),'focused Settings action')
   (out/'keyboard.json').write_text(json.dumps({'unfocused_enter_inert':True,'focused_settings_works':True})+'\n')
   (out/'result.json').write_text(json.dumps({'sha':hashlib.sha256(binary.read_bytes()).hexdigest(),'theme':'macos','package_fonts':False,'icons':checks,'rect':rect},indent=2)+'\n');print('PASS classic-theme Dux CC has visible semantic symbols')
  finally:
   if app is not None and app.poll() is None:app.terminate();app.wait(timeout=5)
   server.terminate();server.wait(timeout=5)
