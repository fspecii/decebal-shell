#!/usr/bin/env python3
"""Independent native Settings input/draft QA; private Xvfb and paths only.

SETTINGS_BIN=/absolute/decsettings DUX_FLOW_EVIDENCE_DIR=/tmp/evidence python3 ...
No build, desktop bus, live app, WM restart or system setting is used.
"""
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import tempfile
import time
import tomllib
from PIL import Image, ImageStat

repo = Path(__file__).resolve().parents[2]
binary = Path(os.environ.get('SETTINGS_BIN', repo / 'target/debug/decsettings')).resolve()
assert os.access(binary, os.X_OK), binary
requested = os.environ.get('E2E_DISPLAY')
if requested:
    match = re.fullmatch(r':([0-9]{1,4})', requested)
    assert match and int(match[1]) >= 2, 'Refusing a live/invalid display'
    number = int(match[1])
else:
    number = next(n for n in range(366, 391) if not Path(f'/tmp/.X11-unix/X{n}').exists() and not Path(f'/tmp/.X{n}-lock').exists())
assert not Path(f'/tmp/.X11-unix/X{number}').exists() and not Path(f'/tmp/.X{number}-lock').exists(), 'Occupied display'
root = Path(tempfile.mkdtemp(prefix='decebal-dux-flow.'))
evidence = Path(os.environ.get('DUX_FLOW_EVIDENCE_DIR', '/tmp/decebal-dux-independent-settings')).resolve()
evidence.mkdir(parents=True, exist_ok=True)
env = dict(os.environ)
for key in list(env):
    if key in ('WAYLAND_DISPLAY','RDWM_SOCK','HYPRLAND_INSTANCE_SIGNATURE','DBUS_STARTER_ADDRESS','DBUS_STARTER_BUS_TYPE','AT_SPI_BUS_ADDRESS','XAUTHORITY','SESSION_MANAGER') or key.startswith(('DECBAR_DEBUG_','DECDOCK_DEBUG_','DECSETTINGS_DEBUG_')):
        env.pop(key, None)
for key, child in {'HOME':'home','XDG_CONFIG_HOME':'cfg','XDG_CONFIG_DIRS':'noconfig',
    'XDG_RUNTIME_DIR':'run','XDG_CACHE_HOME':'cache','XDG_STATE_HOME':'state','XDG_DATA_HOME':'data',
    'XDG_DATA_DIRS':'nodata','DECEBAL_CONFIG':'cfg/omarchy','DECEBAL_THEMES':'themes',
    'DECEBAL_SYSTEM_THEMES':'system-themes','DECEBAL_PALETTES':'palettes','DECEBAL_WIDGETS':'widgets'}.items():
    path = root / child; path.mkdir(parents=True, exist_ok=True); env[key] = str(path)
os.chmod(env['XDG_RUNTIME_DIR'], 0o700)
env.update(DISPLAY=f':{number}', DECEBAL_BACKEND='x11', XDG_SESSION_TYPE='x11',
    DBUS_SESSION_BUS_ADDRESS='unix:path='+str(root/'missing-session'),
    DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(root/'missing-system'),
    DECEBAL_SYSTEM_AUTOMATIC='0', DECEBAL_HIGH_CONTRAST='0', DECEBAL_REDUCE_MOTION='1',
    RDWMCTL='/nonexistent/rdwmctl', RDWM_SOCK=str(root/'missing-wm.sock'))
config = root/'rdwm.toml'
shell = Path(env['DECEBAL_CONFIG'])/'decebal.json'
dump = root/'layout.json'; env['DECSETTINGS_DEBUG_LAYOUT'] = str(dump)
processes = []; streams = []; failed = False


def start(args, label):
    output = (evidence/(label+'.log')).open('w'); streams.append(output)
    process = subprocess.Popen(list(map(str,args)), env=env, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
    processes.append(process); return process


def stop(process):
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try: process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL); process.wait(timeout=2)


def run(*args):
    return subprocess.check_output(list(map(str,args)), env=env, text=True, stderr=subprocess.DEVNULL).strip()


def wait(fn, reason, seconds=15):
    end = time.monotonic()+seconds; last = None
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError,subprocess.CalledProcessError) as error:last=error
        time.sleep(.04)
    raise AssertionError(reason+(': '+str(last) if last else ''))


def state():return json.loads(dump.read_text())
def control(identifier):return next((c for c in state()['ctls'] if c['id']==identifier),None)
def key(win, value):run('xdotool','key','--clearmodifiers','--window',win,value)
def navigate(route):
    expected=route.split('/')[0]
    # Do not queue a no-op request and mistake the existing dump for its ACK;
    # a later keyboard event could otherwise be overwritten by that old request.
    if state()['pane']==expected and ('/' not in route or state()['section']==route.split('/')[-1]):return
    subprocess.run([str(binary),'--config',str(config),route],env=env,check=True,capture_output=True,timeout=8)
    wait(lambda:state()['pane']==expected and ('/' not in route or state()['section']==route.split('/')[-1]), 'IPC navigation failed: '+route)


def visible(win, identifier, height):
    for _ in range(150):
        ctl=wait(lambda:control(identifier),'Missing control '+identifier)
        x,y,w,h=ctl['rect']
        if y>=0 and y+h<=height:return ctl
        before=state()['scroll']
        run('xdotool','mousemove','--window',win,int(x),height//2,'click',5 if y+h>height else 4)
        wait(lambda:state()['scroll']!=before,'Wheel event not consumed')
    raise AssertionError('Cannot reveal '+identifier)


def capture(win,label,width,height,mode):
    path=evidence/(label+'.png')
    def painted():
        run('import','-window',win,path)
        image=Image.open(path).convert('RGB')
        if image.size!=(width,height) or max(ImageStat.Stat(image).stddev)<=8:return False
        # Resize is asynchronous: a mapped larger window can still contain the
        # previous smaller frame surrounded by black until its next upload.
        # macOS test backgrounds are non-black in both modes, so validate the
        # newly exposed bottom/right as well as light/dark identity.
        luminance=sum(image.getpixel((width-24,height-24)))/3
        return luminance>4 and (luminance>150)==(mode=='light')
    wait(painted,'Native resized frame did not finish painting: '+label)
    (evidence/(label+'.json')).write_text(json.dumps(state(),indent=2))


try:
    server=start(['Xvfb',env['DISPLAY'],'-screen','0','2400x1800x24','-nolisten','tcp'],'xvfb')
    def ready():
        if server.poll() is not None:raise RuntimeError('Our private Xvfb exited')
        lock=Path(f'/tmp/.X{number}-lock')
        return lock.exists() and int(lock.read_text().strip())==server.pid and run('xdotool','getdisplaygeometry')=='2400 1800'
    wait(ready,'Private Xvfb not ready')
    results=[]
    for scale,text in ((1,1),(2,1),(1,2)):
        for mode in ('dark','light'):
            label=f'ui{scale}00-text{text}00-{mode}'
            config.write_text('[general]\nborder_px=2\ngap_inner=6\ntags=["1","2","3"]\n')
            shell.write_text(json.dumps({'theme':{'name':'macos','mode':mode},'widgets':{'enabled':False,'items':[]}}))
            env.update(DECSETTINGS_SCALE=str(scale),DECEBAL_FONT_SCALE=str(text),DECEBAL_COLOR_SCHEME=mode)
            dump.unlink(missing_ok=True)
            app=start([binary,'--config',config,'appearance/fonts'],label)
            win=wait(lambda:run('xdotool','search','--onlyvisible','--name','^System Settings$').splitlines()[0],'Settings window not mapped')
            run('xdotool','windowfocus',win)
            for kind,width,height in (('normal',1040*scale,740*scale),('short',900*scale,420*scale)):
                run('xdotool','windowsize',win,width,height,'windowmove',win,0,0)
                wait(lambda:state()['pane']=='appearance' and state()['section']=='fonts','Fonts route not ready')
                time.sleep(.12)
                capture(win,label+'-'+kind,width,height,mode)
                before=state()['scroll']
                run('xdotool','mousemove','--window',win,width*3//4,height//2,'click','5')
                wait(lambda:state()['scroll']>before,'Font content cannot scroll')
                capture(win,label+'-'+kind+'-scrolled',width,height,mode)
            height=420*scale
            # Actual uncommitted field -> IPC destination -> keyboard search ->
            # return -> Enter. File checks prove that suspended drafts neither
            # disappear nor capture keys while their page is hidden.
            if scale==text==1 and mode=='dark':
                navigate('general')
                ctl=visible(win,'r:general:tags',height);x,y,w,h=ctl['rect']
                run('xdotool','mousemove','--window',win,int(x+w/2),int(y+h/2),'click','1')
                key(win,'ctrl+u');run('xdotool','type','--window',win,'--delay','2','QA, DRAFT, KEEP')
                unchanged=config.read_bytes()
                assert 'QA' not in config.read_text()
                navigate('appearance/fonts');key(win,'ctrl+f')
                run('xdotool','type','--window',win,'--delay','2','Icons');key(win,'Return')
                wait(lambda:state()['pane']=='icons','Keyboard search did not reach Icons')
                assert config.read_bytes()==unchanged,'Hidden draft captured destination keys or committed'
                navigate('general');key(win,'Return')
                wait(lambda:tomllib.loads(config.read_text())['general']['tags']==['QA','DRAFT','KEEP'],'Uncommitted draft was lost across navigation')
                capture(win,'draft-return-committed',900,420,mode)
                print('PASS actual uncommitted inline draft survived navigation/search/return',flush=True)
            navigate('general')
            run('xdotool','windowfocus',win)
            # Escape merely restores sidebar focus if a control retained focus.
            if state()['zone']=='Content' and state().get('focus'):
                key(win,'Escape');wait(lambda:state()['zone']=='Sidebar','Escape did not restore sidebar focus')
            ordered=[p['id'] for p in state()['panes']]
            key(win,'Home');wait(lambda:state()['pane']==ordered[0],'Keyboard Home did not select first route')
            key(win,'Down');wait(lambda:state()['pane']==ordered[1],'Keyboard Down route mismatch')
            capture(win,label+'-sidebar-keyboard',900*scale,height,mode)
            navigate('general');key(win,'Tab')
            focused=wait(lambda:state().get('focus'),'Keyboard Tab did not enter content')
            ctl=wait(lambda:control(focused),'Focused control absent')
            assert ctl['enabled'] and 0<=ctl['rect'][1] and ctl['rect'][1]+ctl['rect'][3]<=height,'Keyboard focus outside short viewport'
            capture(win,label+'-content-keyboard',900*scale,height,mode)
            results.append({'ui_scale':scale,'text_scale':text,'mode':mode,'focused':focused,'pid':app.pid})
            stop(app)
            wait(lambda:not subprocess.run(['xdotool','search','--onlyvisible','--name','^System Settings$'],env=env,capture_output=True).stdout,'Old native Settings window remained')
            print('PASS',label,'normal/short pixels, scrolling, keyboard routes/focus',flush=True)
    (evidence/'checks.json').write_text(json.dumps(results,indent=2))
    print('PASS actual inline draft survives IPC/search/return and commits only on Enter',flush=True)
except Exception:
    failed=True
    try:run('import','-window','root',evidence/'failure.png')
    except Exception:pass
    print('Private failure retained:',root,flush=True)
    raise
finally:
    for process in reversed(processes):stop(process)
    for stream in streams:stream.close()
    if dump.exists():shutil.copy2(dump,evidence/'last-layout.json')
    if not failed:shutil.rmtree(root)
