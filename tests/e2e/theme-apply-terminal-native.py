#!/usr/bin/python3
"""Installed/private native Settings Terminal Minimal widget-choice regression."""
import json,os,re,shutil,signal,subprocess,tempfile,time,tomllib,shlex
from pathlib import Path
from PIL import Image
repo=Path(os.environ['TERMINAL_THEME_REPO']); cli=Path(os.environ['TERMINAL_THEME_BIN']).resolve()
settings_binary=Path(os.environ.get('TERMINAL_SETTINGS_BIN',cli.with_name('decsettings'))).resolve()
wm_binary=Path(os.environ.get('TERMINAL_WM_BIN',str(Path.home()/'.local/share/rdwm-decebal/bin/rdwm')))
number=os.environ.get('E2E_DISPLAY',':349')
match=re.fullmatch(r':([0-9]{1,4})',number)
if not match or int(match[1])<2: raise SystemExit('Refusing live or invalid display')
number=int(match[1])
if any(p.exists() for p in (Path('/tmp/.X11-unix/X'+str(number)),Path('/tmp/.X'+str(number)+'-lock'))): raise SystemExit(77)
root=Path(tempfile.mkdtemp(prefix='decebal-terminal-native.'))
evidence=Path(os.environ.get('TERMINAL_THEME_EVIDENCE_DIR',root/'evidence'));evidence.mkdir(parents=True,exist_ok=True)
env=dict(os.environ)
for key in list(env):
 if key in ('DISPLAY','WAYLAND_DISPLAY','RDWM_SOCK','SESSION_MANAGER','AT_SPI_BUS_ADDRESS','XAUTHORITY') or key.startswith(('DECBAR_DEBUG_','DECDOCK_DEBUG_','DECSETTINGS_DEBUG_')): env.pop(key,None)
for key,name in {'HOME':'home','XDG_CONFIG_HOME':'cfg','XDG_CONFIG_DIRS':'noconfig','XDG_STATE_HOME':'state','XDG_CACHE_HOME':'cache','XDG_DATA_HOME':'data','XDG_DATA_DIRS':'nodata','XDG_RUNTIME_DIR':'run','DECEBAL_CONFIG':'cfg/omarchy','DECEBAL_THEMES':'themes','DECEBAL_SYSTEM_THEMES':'systemthemes'}.items():
 path=root/name;path.mkdir(parents=True,exist_ok=True);env[key]=str(path)
os.chmod(env['XDG_RUNTIME_DIR'],0o700)
env.update(DISPLAY=':'+str(number),XDG_SESSION_TYPE='x11',DECEBAL_BACKEND='x11',DBUS_SESSION_BUS_ADDRESS='unix:path='+str(root/'missingbus'),DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(root/'missingsystem'),DECEBAL_SYSTEM_AUTOMATIC='0',DECSETTINGS_CONFIG=str(root/'rdwm.toml'),RDWM_CONFIG=str(root/'rdwm.toml'),RDWMCTL=str(wm_binary.with_name('rdwmctl')),RDWM_SOCK=str(root/'run/no-live.sock'),DECSETTINGS_DEBUG_LAYOUT=str(root/'layout.json'))
config=Path(env['DECSETTINGS_CONFIG']);config.write_text('[general]\n[decoration]\nenabled=true\n[bar]\nshow=false\n')
boot=root/'boot.toml';boot.write_bytes(config.read_bytes())
live_copy=os.environ.get('TERMINAL_THEME_LIVE_COPY')=='1'
if live_copy:
 user_home=Path.home()
 original_shell=(user_home/'.config/omarchy/decebal.json').read_bytes()
 original_dock=(user_home/'.config/omarchy/dock-settings.json').read_bytes()
 config.write_bytes((user_home/'.config/rdwm/config.toml').read_bytes())
 env['RDWMCTL']=str(wm_binary.with_name('rdwmctl'))
 env['RDWM_SOCK']=str(root/'run/no-live.sock')
force_reject=os.environ.get('TERMINAL_FORCE_REJECT')=='1'
if force_reject:
 checker=root/'checker'
 checker.write_text('#!/bin/sh\nif [ "$1" = check-config ]; then echo "QA checker rejected configuration" >&2; exit 1; fi\nexec '+shlex.quote(str(wm_binary.with_name('rdwmctl')))+' "$@"\n')
 checker.chmod(0o700);env['RDWMCTL']=str(checker)
builtin_only=os.environ.get('TERMINAL_THEME_BUILTIN')=='1'
expect_builtin_clock=os.environ.get('TERMINAL_EXPECT_MISSING_CLOCK')!='1'
if not builtin_only:shutil.copytree(repo/'themes/terminal-minimal',Path(env['DECEBAL_THEMES'])/'terminal-minimal')
processes=[];streams=[];failed=False
custom={'enabled':False,'allow_exec':False,'future':7,'items':[{'use':'builtin:clock-card','x':123}]}
shell=Path(env['DECEBAL_CONFIG'])/'decebal.json';dump=Path(env['DECSETTINGS_DEBUG_LAYOUT'])
def start(argv,label,extra=None):
 output=(root/(label+'.log')).open('w');streams.append(output);child_env=dict(env);child_env.update(extra or {})
 p=subprocess.Popen(list(map(str,argv)),env=child_env,stdout=output,stderr=subprocess.STDOUT,start_new_session=True);processes.append(p);return p
def stop(p):
 if p.poll() is None:
  os.killpg(p.pid,signal.SIGTERM)
  try:p.wait(timeout=4)
  except subprocess.TimeoutExpired:os.killpg(p.pid,signal.SIGKILL);p.wait()
def run(*argv):return subprocess.check_output(list(map(str,argv)),env=env,text=True,stderr=subprocess.DEVNULL).strip()
def wait(fn,reason,seconds=20):
 until=time.monotonic()+seconds;last=None
 while time.monotonic()<until:
  try:
   value=fn()
   if value:return value
  except (OSError,ValueError,KeyError,subprocess.CalledProcessError) as error:last=error
  time.sleep(.06)
 raise AssertionError(reason+(': '+str(last) if last else ''))
def data():return json.loads(dump.read_text())
def control(identifier):return next((c for c in data()['ctls'] if c['id']==identifier),None)
def geometry(win):return dict(line.split('=',1) for line in run('xdotool','getwindowgeometry','--shell',win).splitlines())
def click(win,rect):
 x,y,w,h=rect;run('xdotool','mousemove','--window',win,int(x+w/2),int(y+h/2),'click','1')
def shot(name):
 time.sleep(.15);run('import','-window','root',evidence/(name+'.png'))
def visible_control(win,identifier):
 for _ in range(120):
  ctl=wait(lambda:control(identifier),'control not discovered '+identifier)
  height=int(geometry(win)['HEIGHT']);x,y,w,h=ctl['rect']
  if y>=70 and y+h<height-20:return ctl
  distance=y-height/2
  before_scroll=data()['scroll']
  run('xdotool','mousemove','--window',win,800,int(height/2),'click',5 if distance>0 else 4)
  wait(lambda:data()['scroll']!=before_scroll,'wheel event was not consumed before next action')
 raise AssertionError('could not scroll control into view '+identifier)
try:
 xvfb=start(['Xvfb',env['DISPLAY'],'-screen','0','1400x1000x24','-nolisten','tcp'],'xvfb')
 wait(lambda:Path('/tmp/.X11-unix/X'+str(number)).is_socket() and xvfb.poll() is None,'private Xvfb absent')
 wait(lambda:run('xdotool','getdisplaygeometry')=='1400 1000','private X server not ready')
 wm=start([wm_binary,'--backend','x11','--config',boot],'rdwm')
 wait(lambda:'window id' in run('xprop','-root','_NET_SUPPORTING_WM_CHECK'),'private WM absent')
 if live_copy:
  shell.write_bytes(original_shell)
  (Path(env['DECEBAL_CONFIG'])/'dock-settings.json').write_bytes(original_dock)
  app=start([settings_binary,'--config',config,'appearance'],'settings-actual-copy')
  win=wait(lambda:run('xdotool','search','--onlyvisible','--name','^System Settings$').splitlines()[0],'Settings window absent')
  run('xdotool','windowsize',win,1200,900)
  apply=visible_control(win,'l:tcard:apply:terminal-minimal')
  before=shell.read_bytes();before_rdwm=config.read_bytes()
  (evidence/'before-apply.json').write_text(json.dumps({'scroll':data()['scroll'],'rect':apply['rect'],'loaded_thumbs':data()['loaded_thumbs']}))
  if os.environ.get('TERMINAL_THEME_EARLY')!='1':shot('actual-copy-before-apply')
  click(win,apply['rect'])
  if os.environ.get('TERMINAL_EXPECT_ERROR')=='1':
   notice=wait(lambda:(data().get('notice') or '').startswith('Not applied:') and data()['notice'],'old native failure not reproduced')
   assert ('QA checker rejected configuration' if force_reject else 'decoration radius must be <= height') in notice
   if force_reject:assert data()['scroll']==0, 'new error was left above the scrolled viewport'
   assert shell.read_bytes()==before and config.read_bytes()==before_rdwm
   shot('actual-copy-failure')
   print('PASS native checker-failure visibility:' if force_reject else 'REPRODUCED old installed native error:',notice,'scroll',data()['scroll'],flush=True)
  else:
   wait(lambda:json.loads(shell.read_text())['theme']['name']=='terminal-minimal','fixed native exact-copy apply did not succeed')
   wait(lambda:(data().get('notice') or '').startswith('Applied terminal-minimal'), 'native success feedback absent')
   assert json.loads(shell.read_text())['theme']['radius']==24
   decoration=tomllib.loads(config.read_text())['decoration']
   assert decoration['height']==20 and decoration['radius']==20
   shot('actual-copy-applied')
   print('PASS exact user-config private native Apply preserves radius24, composes SSD20/20',flush=True)
  stop(app)
 else:
  pass
 for choice in (() if live_copy else ('keep','replace')):
  shell.write_text(json.dumps({'theme':{'name':'macos','radius':24,'sync_rdwm':True},'dock':{'enabled':False},'widgets':custom}))
  dump.unlink(missing_ok=True)
  app=start([settings_binary,'--config',config,'appearance'],'settings-'+choice)
  win=wait(lambda:run('xdotool','search','--onlyvisible','--name','^System Settings$').splitlines()[0],'Settings window absent')
  run('xdotool','windowsize',win,1200,900)
  apply=visible_control(win,'l:tcard:apply:terminal-minimal')
  (evidence/('before-'+choice+'.json')).write_text(json.dumps({'geometry':geometry(win),'control':apply,'scroll':data()['scroll'],'frames':data()['frames']}))
  if os.environ.get('TERMINAL_THEME_EARLY')!='1':shot('before-'+choice)
  before=shell.read_bytes();click(win,apply['rect'])
  prompt=wait(lambda:control('wc:'+choice),'widget-choice prompt absent')
  height=int(geometry(win)['HEIGHT']);assert 0<=prompt['rect'][1] and prompt['rect'][1]+prompt['rect'][3]<height,'widget choice offscreen'
  assert shell.read_bytes()==before,'theme changed before widget choice'
  shot('prompt-'+choice);click(win,prompt['rect'])
  wait(lambda:json.loads(shell.read_text())['theme']['name']=='terminal-minimal','Terminal Minimal did not apply after '+choice)
  wait(lambda:(data().get('notice') or '').startswith('Applied terminal-minimal'), 'native widget-choice success feedback absent')
  value=json.loads(shell.read_text())['widgets']
  if choice=='keep':assert value['items']==custom['items']
  elif builtin_only and expect_builtin_clock:assert value['items'][0]['use']=='builtin:tty-clock'
  elif not builtin_only:assert value['items'][0]['use']=='tty-clock' and value['items'][0]['source']=='terminal-minimal'
  else:assert value['items']==custom['items'] and value.get('theme_widgets_absent'), 'missing built-in package behavior changed'
  assert value['enabled'] is (choice == 'replace' and (not builtin_only or expect_builtin_clock)) and value['future']==7
  assert json.loads(shell.read_text())['theme']['radius']==24
  decoration=tomllib.loads(config.read_text())['decoration']
  assert decoration['height']==20 and decoration['radius']==20
  shot('applied-'+choice)
  if choice=='replace' and (not builtin_only or expect_builtin_clock):
   run('xdotool','windowminimize',win)
   clock=wait(lambda:run('xdotool','search','--onlyvisible','--name','^decwidgets: TTY clock$').splitlines()[0], 'theme clock did not render on native desktop')
   def clock_painted():
    path=evidence/'tty-clock-native.png'
    run('import','-window',clock,path)
    image=Image.open(path).convert('RGB')
    pixels=image.get_flattened_data() if hasattr(image,'get_flattened_data') else image.getdata()
    return sum(max(pixel)>90 for pixel in pixels)>100
   wait(clock_painted,'compiled clock mapped but no text pixels were painted')
  print('PASS actual native Terminal Minimal Apply → '+choice+' widget choice',flush=True)
  stop(app)
 if not live_copy:print('PASS native private Settings widget choices')
except Exception:
 try:shot('failure')
 except Exception:pass
 failed=True;print('Retained failed native run',root,flush=True);raise
finally:
 for p in reversed(processes):stop(p)
 for stream in streams:stream.close()
 for path in root.glob('*.log'):shutil.copy2(path,evidence/path.name)
 if dump.exists():shutil.copy2(dump,evidence/dump.name)
 if not failed:shutil.rmtree(root)
