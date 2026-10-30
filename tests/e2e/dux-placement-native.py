#!/usr/bin/env python3
"""Actual private Dux band/banner/process-popup placement. No live inputs/audio.
BIN_DIR=/absolute/bin python3 tests/e2e/dux-placement-native.py /tmp/new-evidence
EXPECT_OVERLAP=1 records the pre-fix baseline (only the expected overlap passes).
"""
from pathlib import Path
import hashlib
import json
import os
import signal
import shutil
import subprocess
import sys
import tempfile
import time
import dbus
from PIL import Image

repo = Path(__file__).resolve().parents[2]
bins = Path(os.environ['BIN_DIR']).resolve()
out = Path(sys.argv[1]).resolve()
out.mkdir(mode=0o700, parents=True, exist_ok=True)
expected = os.environ.get('EXPECT_OVERLAP') == '1'
result = {'binaries': {n: hashlib.sha256((bins/n).read_bytes()).hexdigest() for n in ('decbar','decnotify','decmon')}, 'expected_overlap': expected}
procs, logs = [], []
def wait(fn, label, seconds=15):
    end = time.monotonic()+seconds
    while time.monotonic()<end:
        try:
            value = fn()
            if value:return value
        except (OSError,ValueError,subprocess.CalledProcessError,dbus.DBusException):pass
        time.sleep(.04)
    raise AssertionError(label)
try:
    with tempfile.TemporaryDirectory(prefix='dux-placement-') as tmp:
        root=Path(tmp);env=dict(os.environ)
        for k in list(env):
            if k.startswith(('DECEBAL_','DECNOTIFY_','DECBAR_','XDG_','DBUS_','RDWM')) or k in ('HOME','DISPLAY','WAYLAND_DISPLAY','XAUTHORITY','SESSION_MANAGER','AT_SPI_BUS_ADDRESS'):env.pop(k)
        for key in ('HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_STATE_HOME','XDG_CACHE_HOME','XDG_RUNTIME_DIR','XDG_CONFIG_DIRS','XDG_DATA_DIRS','DECEBAL_CONFIG','DECEBAL_THEMES','DECEBAL_SYSTEM_THEMES'):
            p=root/key;p.mkdir(mode=0o700);env[key]=str(p)
        shutil.copytree(repo/'themes/dux-lavender',Path(env['DECEBAL_THEMES'])/'dux-lavender')
        (Path(env['DECEBAL_CONFIG'])/'decebal.json').write_text(json.dumps({'theme':{'name':'dux-lavender','mode':'dark'},'bar':{'enabled':True},'dock':{'enabled':False}}))
        (root/'bin').mkdir()
        for n in ('curl','nmcli','pactl','wpctl','playerctl','bluetoothctl','brightnessctl','decutil','rdwmctl'):
            p=root/'bin'/n;p.write_text('#!/bin/sh\nexit 1\n');p.chmod(0o755)
        display=next(f':{n}' for n in range(700,780) if not Path(f'/tmp/.X11-unix/X{n}').exists() and not Path(f'/tmp/.X{n}-lock').exists())
        env.update(DISPLAY=display,DECEBAL_BACKEND='x11',XDG_SESSION_TYPE='x11',DECEBAL_SYSTEM_AUTOMATIC='0',DBUS_SESSION_BUS_ADDRESS='unix:path='+tmp+'/bus',DBUS_SYSTEM_BUS_ADDRESS='unix:path='+tmp+'/no-system',PATH=str(root/'bin')+':/usr/bin:/bin')
        def start(args,name):
            log=(out/(name+'.log')).open('w');logs.append(log)
            p=subprocess.Popen(list(map(str,args)),env=env,stdout=log,stderr=log,start_new_session=True);procs.append(p);return p
        def run(*args):return subprocess.check_output(args,env=env,text=True,stderr=subprocess.DEVNULL).strip()
        server=start(['Xvfb',display,'-screen','0','1280x900x24','-nolisten','tcp','-noreset'],'xvfb')
        wait(lambda:server.poll() is None and Path(f'/tmp/.X{display[1:]}-lock').exists() and int(Path(f'/tmp/.X{display[1:]}-lock').read_text().strip())==server.pid and Path(f'/tmp/.X11-unix/X{display[1:]}').exists(),'owned Xvfb')
        busp=start(['dbus-daemon','--session','--nofork','--address='+env['DBUS_SESSION_BUS_ADDRESS']],'bus')
        wait(lambda:(root/'bus').exists(),'private bus')
        bus=dbus.bus.BusConnection(env['DBUS_SESSION_BUS_ADDRESS'])
        bar=start([bins/'decbar','--app','Native placement fixture'],'bar')
        bw=wait(lambda:run('xdotool','search','--name','^decebal-bar$').splitlines()[0],'native bar')
        def geometry(w):
            x=dict(v.split('=',1) for v in run('xdotool','getwindowgeometry','--shell',w).splitlines() if '=' in v)
            return {k:int(x[k]) for k in ('X','Y','WIDTH','HEIGHT')}
        bg=geometry(bw)
        time.sleep(.25)
        run('import','-window','root',str(out/'bar-before-popups.png'))
        image=Image.open(out/'bar-before-popups.png').convert('RGB')
        painted=[y for y in range(80) if sum(image.getpixel((x,y))!=(0,0,0) for x in range(1280))>640]
        assert painted and max(painted)==41,'actual38px Dux band must painty4..42'
        # Bar surfaces include transparent room for popups; their X window height
        # is not the reserved edge extent. The band plus bottom margin ends at46.
        bottom=max(painted)+1+4
        notifier=start([bins/'decnotify'],'notify')
        wait(lambda:bus.name_has_owner('org.freedesktop.Notifications'),'private notifier')
        proxy=dbus.Interface(bus.get_object('org.freedesktop.Notifications','/org/freedesktop/Notifications'),'org.freedesktop.Notifications')
        nid=proxy.Notify('Placement fixture',dbus.UInt32(0),'','Native placement fixture','Must clear the complete Dux band',dbus.Array([],signature='s'),dbus.Dictionary({'suppress-sound':True},signature='sv'),dbus.Int32(60000))
        nw=wait(lambda:run('xdotool','search','--class','^Decnotify$').splitlines()[0],'native banner')
        ng=geometry(nw);time.sleep(.2);run('import','-window','root',str(out/'banner-under-bar.png'))
        proxy.CloseNotification(nid)
        monitor=start([bins/'decmon','performance','--popup'],'monitor')
        mw=wait(lambda:run('xdotool','search','--class','decmon').splitlines()[0],'native process monitor')
        mg=geometry(mw);time.sleep(.2);run('import','-window','root',str(out/'monitor-under-bar.png'))
        result['bar_visual_bottom']=max(painted)+1
        result.update(bar=bg,banner=ng,monitor=mg,bar_bottom=bottom,banner_clears=ng['Y']>=bottom+8,monitor_clears=mg['Y']>=bottom+4)
        assert all(p.poll() is None for p in (server,busp,bar,notifier,monitor)),'a private component exited'
        if expected:assert not result['banner_clears'] and not result['monitor_clears'],'baseline must reproduce both expected overlaps'
        else:assert result['banner_clears'] and result['monitor_clears'],'popup overlaps Dux band'
        result['passed']=True
finally:
    for p in reversed(procs):
        if p.poll() is None:
            os.killpg(p.pid,signal.SIGTERM)
    for p in procs:
        try:p.wait(timeout=3)
        except subprocess.TimeoutExpired:os.killpg(p.pid,signal.SIGKILL);p.wait(timeout=3)
    for f in logs:f.close()
    (out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result))
