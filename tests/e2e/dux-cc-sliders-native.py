#!/usr/bin/env python3
"""Private real-X full-row slider regression; requires BAR_BINARY and output dir.
Only fixture wpctl/brightnessctl setters run. No live display, bus or audio.
"""
from pathlib import Path
import hashlib,json,os,subprocess,sys,tempfile,time
from PIL import ImageGrab
binary=Path(os.environ['BAR_BINARY']).resolve()
out=Path(sys.argv[1]).resolve();out.mkdir(parents=True,exist_ok=True)
result={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'cases':[]}
def wait(fn,label):
 for _ in range(200):
  try:
   value=fn()
   if value:return value
  except (OSError,ValueError,subprocess.CalledProcessError):pass
  time.sleep(.05)
 raise AssertionError(label)
for scale,height in [(1,720),(2,240)]:
 with tempfile.TemporaryDirectory(prefix='dux-slider-x11-') as tmp:
  root=Path(tmp);env=dict(os.environ);case=out/str(scale*100);case.mkdir(exist_ok=True)
  for k in list(env):
   if k.startswith(('DECEBAL_','DECBAR_','XDG_','DBUS_','RDWM')) or k in ('HOME','DISPLAY','WAYLAND_DISPLAY','XAUTHORITY','AT_SPI_BUS_ADDRESS','SESSION_MANAGER'):env.pop(k)
  for k in ('HOME','XDG_CONFIG_HOME','XDG_CACHE_HOME','XDG_DATA_HOME','XDG_STATE_HOME','XDG_RUNTIME_DIR','XDG_CONFIG_DIRS','XDG_DATA_DIRS','DECEBAL_CONFIG','DECEBAL_THEMES','DECEBAL_SYSTEM_THEMES'):
   p=root/k;p.mkdir(mode=0o700);env[k]=str(p)
  (Path(env['DECEBAL_CONFIG'])/'decebal.json').write_text(json.dumps({'theme':{'name':'macos','mode':'dark'},'control_center':{'style':'dux','position':'top-left','tiles':['sound','display']}}))
  tools=root/'bin';tools.mkdir();calls=root/'calls'
  def tool(name,body):
   p=tools/name;p.write_text('#!/bin/sh\n'+body+'\n');p.chmod(0o755)
  tool('wpctl','case "$1" in get-volume) echo "Volume: 0.50";; inspect) echo \'node.description = "Fixture sink"\';; set-volume|set-mute) printf "wpctl %s\\n" "$*" >> "$DUX_SLIDER_CALLS";; *) exit 1;; esac')
  tool('brightnessctl','case "$*" in *set*) printf "brightnessctl %s\\n" "$*" >> "$DUX_SLIDER_CALLS";; *) echo "fixture,backlight,50,50%,100";; esac')
  for name in ('curl','nmcli','pactl','playerctl','bluetoothctl','decutil','rdwmctl','decsettings'):tool(name,'exit 1')
  display=next(f':{n}' for n in range(780,820) if not Path(f'/tmp/.X11-unix/X{n}').exists() and not Path(f'/tmp/.X{n}-lock').exists())
  env.update(DISPLAY=display,DECEBAL_BACKEND='x11',DECEBAL_FONT_SCALE=str(scale),DECEBAL_SYSTEM_AUTOMATIC='0',DECBAR_DEBUG_PANEL=str(root/'panel'),DUX_SLIDER_CALLS=str(calls),DBUS_SESSION_BUS_ADDRESS='unix:path='+tmp+'/no-session',DBUS_SYSTEM_BUS_ADDRESS='unix:path='+tmp+'/no-system',PATH=str(tools)+':/usr/bin:/bin')
  def xd(*args):subprocess.run(['xdotool',*map(str,args)],env=env,check=True,timeout=5)
  def setters():return calls.read_text().splitlines() if calls.exists() else []
  def metadata():
   lines=(root/'panel').read_text().splitlines();rect=list(map(int,lines[0].split()[1:]));scroll=float(lines[2].split()[1]);return rect,scroll
  with (case/'native.log').open('w') as log:
   server=subprocess.Popen(['Xvfb',display,'-screen','0',f'960x{height}x24','-nolisten','tcp','-noreset'],env=env,stdout=log,stderr=log);app=None
   try:
    wait(lambda:server.poll() is None and Path(f'/tmp/.X{display[1:]}-lock').exists() and int(Path(f'/tmp/.X{display[1:]}-lock').read_text())==server.pid,'owned Xvfb')
    app=subprocess.Popen([str(binary),'--control-center','open'],env=env,stdout=log,stderr=log)
    wait(lambda:(root/'panel').exists(),'panel metadata')
    wait(lambda:ImageGrab.grab(xdisplay=display).getbbox(),'presented panel');time.sleep(.2)
    rect,scroll=metadata();px,py,pw,ph=rect
    # Source CcSliderRow: y=pad+header+gap; full row48/68; horizontal132*min(scale,1.5).
    row_h=48+(scale-1)*20;first_y=16+72+(scale-1)*24+8
    track_x=16+132*min(scale,1.5);track_w=pw-32-132*min(scale,1.5)-16
    checks=[]
    for name,index,prefix in [('volume',0,'wpctl set-volume'),('brightness',1,'brightnessctl')]:
     row_y=first_y+index*(row_h+8)
     # Scroll over the noninteractive label until the complete row is visible.
     for _ in range(8):
      rect,scroll=metadata();px,py,pw,ph=rect
      if row_y-scroll>=0 and row_y+row_h-scroll<=ph:break
      xd('mousemove',px+25,py+ph//2,'click',5);time.sleep(.08)
     assert row_y-scroll>=0 and row_y+row_h-scroll<=ph,(name,'full row not visible',metadata())
     for edge,offset,fraction in [('top',1,.25),('bottom',row_h-2,.75)]:
      x=round(px+track_x+track_w*fraction);y=round(py+row_y-scroll+offset)
      before=len(setters());xd('mousemove',x,y,'mousedown',1);time.sleep(.05);xd('mouseup',1)
      expected=f'{fraction:.2f}' if name=='volume' else f'{round(fraction*100)}%'
      wait(lambda:any(line.startswith(prefix) and line.split()[-1]==expected for line in setters()[before:]),name+' '+edge+' setter')
      emitted=setters()[before:];checks.append({'target':name,'edge':edge,'x':x,'y':y,'setters':emitted})
      # Wheel at that same full-row edge must reach the fixture, not just scroll.
      before=len(setters());xd('click',4)
      expected=f'{fraction+.05:.2f}' if name=='volume' else f'{round(fraction*100)+5}%'
      wait(lambda:any(line.startswith(prefix) and line.split()[-1]==expected for line in setters()[before:]),name+' '+edge+' wheel')
      time.sleep(.1)
      checks[-1]['wheel_setters']=setters()[before:]
      before=len(setters());xd('key','Return');time.sleep(.15)
      assert len(setters())==before and app.poll() is None,'stale pointer Enter changed slider'
     ImageGrab.grab(xdisplay=display).save(case/(name+'.png'))
    # Source target order with only sound/display: Settings, Volume, Brightness.
    # First Down chooses Settings from no focus; second Down reveals Volume.
    before=len(setters());xd('key','Down');time.sleep(.1)
    baseline=ImageGrab.grab(xdisplay=display).convert('RGB');baseline_rect,baseline_scroll=metadata()
    xd('key','Down')
    def focused_ring():
     focus_rect,focus_scroll=metadata();fx,fy,fw,fh=focus_rect
     assert first_y-focus_scroll>=0 and first_y+row_h-focus_scroll<=fh,'keyboard did not reveal complete Volume target'
     image=ImageGrab.grab(xdisplay=display).convert('RGB')
     # Focus stroke is on the row perimeter, away from its middle painted track.
     points=[(round(fx+track_x+dx),round(fy+first_y-focus_scroll+dy)) for dx in range(16,round(track_w)-16) for dy in (1,2)]
     blue=lambda pixel:pixel[2]>pixel[0]+40 and pixel[2]>pixel[1]+20
     count=sum(blue(image.getpixel(point)) for point in points)
     baseline_count=sum(blue(baseline.getpixel(point)) for point in points) if baseline_scroll==focus_scroll else 0
     return (image,focus_rect,focus_scroll,count,baseline_count) if count>baseline_count+30 else None
    focused,focus_rect,focus_scroll,ring_pixels,baseline_ring_pixels=wait(focused_ring,'visible keyboard Volume focus ring')
    focused.save(case/'volume-keyboard-focus.png')
    assert len(setters())==before,'keyboard navigation unexpectedly changed a slider'
    xd('key','Return');time.sleep(.15)
    assert len(setters())==before and app.poll() is None,'Enter on focused slider issued a setter'
    keyboard={'target':'volume','revealed':True,'rect':focus_rect,'scroll':focus_scroll,'ring_pixels':ring_pixels,'baseline_ring_pixels':baseline_ring_pixels,'navigation_and_enter_no_setter':True}
    (case/'setters.log').write_text('\n'.join(setters())+'\n')
    result['cases'].append({'font_percent':scale*100,'screen_height':height,'panel':rect,'scroll':scroll,'checks':checks,'stale_enter_inert':True,'keyboard_focus':keyboard})
    (out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
   finally:
    if app is not None and app.poll() is None:app.terminate();app.wait(timeout=5)
    server.terminate();server.wait(timeout=5)
print(json.dumps(result))
