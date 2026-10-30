#!/usr/bin/env python3
"""Actual nested rdwm capability -> Settings controls, forced X11 rendering.

No live bus/display/input. WAYLAND_RUN_BIN, RDWMCTL_BIN and SETTINGS_BIN select
pinned builds; EVIDENCE_DIR retains private UI screenshots and artifact hashes.
This tests Wayland runtime support independently of Settings' render backend.
"""
from pathlib import Path
import hashlib,json,os,re,shutil,socket,subprocess,tempfile,time

ROOT=Path(__file__).resolve().parents[2]
SETTINGS=Path(os.environ.get('SETTINGS_BIN',ROOT/'target/release/decsettings')).resolve()
CTL=Path(os.environ.get('RDWMCTL_BIN',ROOT.parent/'rdwm/target/release/rdwmctl')).resolve()
NORMAL_WM=bool(os.environ.get('RDWM_BIN'))
WM=Path(os.environ.get('RDWM_BIN') or os.environ.get('WAYLAND_RUN_BIN',ROOT.parent/'rdwm/target/debug/examples/run')).resolve()
OUT=Path(os.environ.get('EVIDENCE_DIR','/tmp/wayland-shadow-settings')).resolve()
OUT.mkdir(parents=True,exist_ok=True)


def wait(fn,label,seconds=12):
    until=time.monotonic()+seconds
    while time.monotonic()<until:
        try:
            result=fn()
            if result:return result
        except (OSError,ValueError,KeyError,subprocess.CalledProcessError):pass
        time.sleep(.05)
    raise AssertionError(label)


if __name__=='__main__':
    for name in ['sway','swaymsg','Xvfb','xdotool','import']:
        if not shutil.which(name):raise SystemExit(77)
    processes=[];logs=[]
    with tempfile.TemporaryDirectory(prefix='shadow-settings-private-') as temp:
        base=Path(temp);env=dict(os.environ)
        for key in list(env):
            if key.startswith(('DECEBAL_','DECSETTINGS_','RDWM','XDG_','DBUS_')) or key in ['HOME','DISPLAY','WAYLAND_DISPLAY','SWAYSOCK','XAUTHORITY','SESSION_MANAGER','AT_SPI_BUS_ADDRESS']:
                env.pop(key)
        for key,name in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_CONFIG_DIRS','configdirs'),('XDG_DATA_HOME','data'),('XDG_DATA_DIRS','datadirs'),('XDG_CACHE_HOME','cache'),('XDG_STATE_HOME','state'),('XDG_RUNTIME_DIR','run')]:
            p=base/name;p.mkdir(mode=0o700);env[key]=str(p)
        env.update(DBUS_SESSION_BUS_ADDRESS='unix:path='+str(base/'no-session'),DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(base/'no-system'),DECEBAL_SYSTEM_AUTOMATIC='0',RDWM_RENDERER='pixman',RDWM_SOCK=str(base/'run/rdwm.sock'))
        config=base/'config/rdwm/config.toml';config.parent.mkdir();config.write_text('[general]\nlayouts=["floating"]\n[bar]\nshow=false\n[decoration]\nenabled=true\nshadow=true\n')
        env['RDWM_CONFIG']=str(config)
        def start(args,name,e=None):
            f=(OUT/(name+'.log')).open('w');logs.append(f)
            p=subprocess.Popen(list(map(str,args)),env=e or env,stdout=f,stderr=f,start_new_session=True);processes.append(p);return p
        def run(*args):return subprocess.check_output(list(map(str,args)),env=env,text=True,timeout=8).strip()
        def query(value):
            with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as s:
                s.settimeout(3);s.connect(env['RDWM_SOCK']);s.sendall(json.dumps(value).encode()+b'\n');buf=b''
                while b'\n' not in buf:buf+=s.recv(65536)
                return json.loads(buf.split(b'\n')[0])
        try:
            shutil.copy2(shutil.which('sway'),base/'private-host')
            hostcfg=base/'host.conf';hostcfg.write_text('xwayland disable\ndefault_border none\n')
            start([base/'private-host','-c',hostcfg],'host',dict(env,LIBSEAT_BACKEND='noop',WLR_BACKENDS='headless',WLR_RENDERER='pixman',WLR_LIBINPUT_NO_DEVICES='1'))
            runtime=Path(env['XDG_RUNTIME_DIR'])
            host_socket=wait(lambda:next((p.name for p in runtime.glob('wayland-*') if p.is_socket()),None),'host socket')
            env['SWAYSOCK']=str(wait(lambda:next(runtime.glob('sway-ipc.*.sock'),None),'host IPC'))
            run('swaymsg','output','HEADLESS-1','resolution','1280x900')
            pinned=base/'private-compositor';shutil.copy2(WM,pinned)
            wm=start([pinned,*(['--backend','wayland'] if NORMAL_WM else ['nested'])],'rdwm',dict(env,WAYLAND_DISPLAY=host_socket))
            actual_wm_sha=hashlib.sha256(Path(f'/proc/{wm.pid}/exe').read_bytes()).hexdigest()
            build=None
            if os.environ.get('RDWM_BUILD_MANIFEST'):
                build=json.loads(Path(os.environ['RDWM_BUILD_MANIFEST']).read_text())
                assert build['binaries']['rdwm']==actual_wm_sha
            env['WAYLAND_DISPLAY']=wait(lambda:(re.search(r'listening on WAYLAND_DISPLAY=(\S+)',(OUT/'rdwm.log').read_text()) or [None,None])[1],'nested compositor')
            effects=query({'cmd':'output_query'})
            (OUT/'effects.json').write_text(json.dumps(effects,indent=2))
            (OUT/'runtime-config.json').write_text(json.dumps(query({'cmd':'get_config'}),indent=2))
            (OUT/'state.json').write_text(json.dumps(query({'cmd':'get_state'}),indent=2))
            assert effects['effects']['ssd_shadows'] is True
            # X11 rendering deliberately does not change the actual Wayland capability gate.
            xr,xw=os.pipe()
            xenv=dict(env);xenv.pop('WAYLAND_DISPLAY',None)
            xf=(OUT/'xvfb.log').open('w');logs.append(xf)
            xvfb=subprocess.Popen(['Xvfb','-displayfd',str(xw),'-screen','0','1300x2000x24','-nolisten','tcp'],env=xenv,pass_fds=[xw],stdout=xf,stderr=xf);processes.append(xvfb);os.close(xw)
            with os.fdopen(xr) as f:env['DISPLAY']=':'+f.readline().strip()
            env.update(DECEBAL_BACKEND='x11',RDWMCTL=str(CTL),DECSETTINGS_DEBUG_LAYOUT=str(base/'layout.json'))
            probe=os.environ.get('FRAME_SOURCE_PROBE')
            if probe:(OUT/'runtime-probe.json').write_text(run(probe,'--runtime',env['RDWM_SOCK']))
            pinned_settings=base/'decsettings';shutil.copy2(SETTINGS,pinned_settings)
            app=start([pinned_settings,'--config',config,'windowstyle'],'settings')
            actual_settings_sha=hashlib.sha256(Path(f'/proc/{app.pid}/exe').read_bytes()).hexdigest()
            wid=wait(lambda:run('xdotool','search','--class','^DecSettings$').splitlines()[0],'Settings')
            run('xdotool','windowsize',wid,'1080','1800');run('xdotool','windowmove',wid,'0','0');run('xdotool','windowfocus',wid)
            def geometry():return dict(line.split('=',1) for line in run('xdotool','getwindowgeometry','--shell',wid).splitlines())
            wait(lambda:geometry()['WIDTH']=='1080' and geometry()['HEIGHT']=='1800' and geometry()['X']=='0' and geometry()['Y']=='0','Settings client geometry')
            def layout():return json.loads((base/'layout.json').read_text())
            def controls():return [c for c in layout()['ctls'] if c['id'].startswith('shadow:')]
            # ConfigureNotify and painting are asynchronous; a startup-sized
            # layout can already contain four controls outside its viewport.
            # Force fresh motion/redraw after the server's resize and require
            # stable control geometry before using its coordinates.
            run('xdotool','mousemove','--window',wid,'5','5')
            run('xdotool','mousemove','--window',wid,'500','400')
            time.sleep(.35)
            def stable_controls():
                first=controls()
                if len(first)!=4:return None
                time.sleep(.1)
                second=controls()
                return second if first==second else None
            ctls=wait(stable_controls,'four stable live-capability shadow controls')
            (OUT/'before-click-layout.json').write_text(json.dumps(layout(),indent=2))
            toggle=next(c for c in ctls if c['id']=='shadow:shadow')
            x,y,w,h=toggle['rect'];assert y>=0 and y+h<=1800,toggle
            before=config.read_text();run('xdotool','mousemove','--window',wid,round(x+w/2),round(y+h/2),'click','1')
            wait(lambda:query({'cmd':'get_config'})['config']['decoration']['shadow'] is False,'actual Wayland shadow toggle reload')
            assert config.read_text()!=before
            run('import','-window',wid,OUT/'supported.png')
            wm.terminate();wm.wait(timeout=5)
            wait(lambda:not controls(),'disconnect must remove stale shadow controls')
            assert app.poll() is None
            run('import','-window',wid,OUT/'disconnected.png')
            (OUT/'result.json').write_text(json.dumps({'settings_sha256':actual_settings_sha,'wm_sha256':actual_wm_sha,'build_revision':build['head'] if build else None,'live_effects':effects['effects'],'forced_x11_on_wayland':True,'controls':4,'toggle_reloaded_real_wayland':True,'disconnect_clears_controls':True},indent=2)+'\n')
            print('PASS actual Wayland capability, forced-X11 Settings toggle and disconnect guard')
        finally:
            if (base/'layout.json').exists():shutil.copy2(base/'layout.json',OUT/'final-layout.json')
            for p in reversed(processes):
                if p.poll() is None:p.terminate()
            for p in reversed(processes):
                try:p.wait(timeout=3)
                except subprocess.TimeoutExpired:p.kill();p.wait(timeout=3)
            for f in logs:f.close()
