#!/usr/bin/env python3
"""Private native Welcome evidence with actual effective rdwm fixture shortcuts.
SETTINGS_BIN=/absolute/decsettings RDWM_BIN=/absolute/rdwm python3 ... OUT
No fixture binding command is invoked. Only owned private processes are stopped.
"""
import hashlib, json, os, signal, socket, subprocess, sys, tempfile, time
from pathlib import Path
binary = Path(os.environ['SETTINGS_BIN']).resolve()
wm_binary = Path(os.environ['RDWM_BIN']).resolve()
out = Path(sys.argv[1]).resolve(); out.mkdir(parents=True, exist_ok=True)
owned, logs = [], []
def wait(fn, seconds=15):
    end = time.monotonic()+seconds
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError,IndexError,subprocess.CalledProcessError):pass
        time.sleep(.05)
    raise AssertionError('private native wait timed out')
def start(args, env, name):
    f=(out/(name+'.log')).open('w');logs.append(f)
    p=subprocess.Popen(list(map(str,args)),env=env,stdout=f,stderr=f,start_new_session=True);owned.append(p);return p
with tempfile.TemporaryDirectory(prefix='welcome-shortcuts-') as t:
    root=Path(t);env={k:v for k,v in os.environ.items() if not k.startswith(('DECEBAL_','DECSETTINGS_','XDG_','RDWM','DBUS_')) and k not in ('DISPLAY','WAYLAND_DISPLAY','XAUTHORITY','SESSION_MANAGER','AT_SPI_BUS_ADDRESS')}
    for key in ('HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_STATE_HOME','XDG_CACHE_HOME','XDG_RUNTIME_DIR','XDG_CONFIG_DIRS','XDG_DATA_DIRS','DECEBAL_CONFIG','DECEBAL_THEMES','DECEBAL_SYSTEM_THEMES','DECEBAL_PALETTES','DECEBAL_WIDGETS'):
        p=root/key;p.mkdir(mode=0o700);env[key]=str(p)
    number=next(n for n in range(710,760) if not Path(f'/tmp/.X11-unix/X{n}').exists() and not Path(f'/tmp/.X{n}-lock').exists())
    env.update(DISPLAY=f':{number}',XDG_SESSION_TYPE='x11',DECEBAL_BACKEND='x11',DECEBAL_SYSTEM_AUTOMATIC='0',DBUS_SESSION_BUS_ADDRESS='unix:path='+t+'/no-session',DBUS_SYSTEM_BUS_ADDRESS='unix:path='+t+'/no-system',RDWM_SOCK=env['XDG_RUNTIME_DIR']+'/rdwm.sock',RDWMCTL=str(wm_binary.with_name('rdwmctl')))
    cfg=root/'config.toml';cfg.write_text('''[general]
default_binds = false
[decoration]
enabled = false
[[bind]]
keys = ["alt+space"]
action = "spawn"
args = ["decspot"]
[[bind]]
keys = ["super+ctrl+alt+shift+x", "shift+F12", "ctrl+f"]
action = "fullscreen_center"
[[bind]]
keys = ["super+t"]
action = "spawn"
args = ["rdwm-terminal"]
[[bind]]
keys = ["ctrl+print"]
action = "spawn"
args = ["decutil", "screenshot"]
[[mode]]
name = "resize-long-mode-name"
[[mode.bind]]
keys = ["super+ctrl+alt+shift+F11"]
action = "togglefloating"
''')
    def run(*args):return subprocess.check_output(args,env=env,text=True,stderr=subprocess.DEVNULL).strip()
    def ipc(obj):
        with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as s:
            s.settimeout(1);s.connect(env['RDWM_SOCK']);s.sendall(json.dumps(obj).encode()+b'\n');data=b''
            while b'\n' not in data:data+=s.recv(65536)
            reply=json.loads(data.split(b'\n')[0]);assert reply.get('ok'),reply;return reply
    def state():return ipc({'cmd':'get_state'})['state']
    cases=[]
    try:
        xvfb=start(['Xvfb',env['DISPLAY'],'-screen','0','1400x1800x24','-nolisten','tcp'],env,'xvfb')
        def display_ready():
            assert xvfb.poll() is None
            p=Path(f'/tmp/.X{number}-lock')
            return p.exists() and int(p.read_text().strip())==xvfb.pid and Path(f'/tmp/.X11-unix/X{number}').exists()
        wait(display_ready)
        wm=start([wm_binary,'--backend','x11','--config',cfg],env,'rdwm');wait(state)
        binds=ipc({'cmd':'bind','op':'list'})['binds'];(out/'effective-bind-fixture.json').write_text(json.dumps(binds,indent=2))
        assert any('alt+space' in b['keys'] for b in binds)
        for scale in (1,2):
            for size,w,h in [('normal',1100,1000),('short',920,440)]:
                name=f'text{scale*100}-{size}';layout=root/(name+'.json');e=dict(env,DECSETTINGS_DEBUG_LAYOUT=str(layout),DECEBAL_FONT_SCALE=str(scale))
                app=start([binary,'--config',cfg,'welcome'],e,name)
                wid=wait(lambda:run('xdotool','search','--class','DecSettings').splitlines()[0]);wid=int(wid)
                wait(lambda: any(x['id']==wid for x in state()['windows']))
                ipc({'action':'focuswindow','args':[str(wid)]})
                if not next(x for x in state()['windows'] if x['id']==wid)['floating']:ipc({'action':'togglefloating'})
                run('xdotool','windowsize',str(wid),str(w),str(h));run('xdotool','windowmove',str(wid),'0','0')
                def ready():
                    d=json.loads(layout.read_text())
                    if d['pane']!='welcome':return None
                    allkeys=next(c for c in d['ctls'] if c['label']=='All shortcuts and modes')
                    return d if allkeys['rect'][1]>700 else None
                top=wait(ready);time.sleep(.4);run('import','-window',str(wid),str(out/(name+'-top.png')))
                (out/(name+'-top.json')).write_text(json.dumps(top,indent=2))
                if scale == 2:
                    # Capture the long sequence/mode notes partway down, rather than
                    # relying only on top/bottom captures to establish enlarged wrapping.
                    for step in range(1, 7):
                        run('xdotool','mousemove','--window',str(wid),str(w-150),str(h//2),'click','--repeat','4','--delay','4','5');time.sleep(.08)
                        run('import','-window',str(wid),str(out/(name+f'-middle{step*4}.png')))
                run('xdotool','mousemove','--window',str(wid),str(w-150),str(h//2),'click','--repeat','100','--delay','4','5');time.sleep(.3)
                bottom=json.loads(layout.read_text());recovery=next(c for c in bottom['ctls'] if c['id']=='w:go:general');x,y,cw,ch=recovery['rect']
                assert 0<=x<w and 0<=y and y+ch<=h,(name,'recovery unreachable',recovery)
                run('import','-window',str(wid),str(out/(name+'-bottom.png')));(out/(name+'-bottom.json')).write_text(json.dumps(bottom,indent=2))
                cases.append({'name':name,'pane':bottom['pane'],'scroll':bottom['scroll'],'recovery_control_reachable':True})
                app.terminate();app.wait(timeout=5);wait(lambda:not any(x['id']==wid for x in state()['windows']))
        # A fresh disconnected Welcome must not present cached/sample defaults as live.
        wm.terminate();wm.wait(timeout=5)
        e=dict(env,DECSETTINGS_DEBUG_LAYOUT=str(root/'disconnected.json'),DECEBAL_FONT_SCALE='2')
        app=start([binary,'--config',cfg,'welcome'],e,'disconnected')
        wid=wait(lambda:run('xdotool','search','--class','DecSettings').splitlines()[0]);run('xdotool','windowsize',wid,'1100','900');time.sleep(1.5)
        run('import','-window',wid,str(out/'disconnected-text200.png'))
        d=json.loads((root/'disconnected.json').read_text());(out/'disconnected-text200.json').write_text(json.dumps(d,indent=2));assert d['pane']=='welcome'
        (out/'result.json').write_text(json.dumps({'settings_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'rdwm_sha256':hashlib.sha256(wm_binary.read_bytes()).hexdigest(),'display':env['DISPLAY'],'cases':cases,'disconnected_capture':True},indent=2));print('PASS private Welcome native matrix',out)
    finally:
        for p in reversed(owned):
            if p.poll() is None:
                os.killpg(p.pid,signal.SIGTERM)
                try:p.wait(timeout=5)
                except subprocess.TimeoutExpired:os.killpg(p.pid,signal.SIGKILL);p.wait(timeout=5)
        for f in logs:f.close()
