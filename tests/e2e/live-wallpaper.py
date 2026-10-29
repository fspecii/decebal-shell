#!/usr/bin/env python3
"""Private Xvfb/rdwm playback test; never installs or touches the current desktop."""
import json, os, pathlib, signal, socket, subprocess, tempfile, time
ROOT = pathlib.Path(__file__).resolve().parents[2]
RDWM = pathlib.Path(os.environ.get('RDWM_BIN_DIR', str(pathlib.Path.home() / 'Projects/rdwm/target/debug')))
BIN = pathlib.Path(os.environ.get('MS_BIN_DIR', str(ROOT / 'target/debug')))
procs = []
checks = 0

def check(value, why):
    global checks
    assert value, why
    checks += 1
    print('PASS:', why, flush=True)

def wait(fn, timeout=15):
    end = time.monotonic() + timeout
    last = None
    while time.monotonic() < end:
        try:
            last = fn()
            if last:
                return last
        except (OSError, ValueError, KeyError, IndexError, subprocess.CalledProcessError):
            pass
        time.sleep(.1)
    raise AssertionError(f'timed out; last value {last!r}')

with tempfile.TemporaryDirectory(prefix='decebal-live-test-') as tmp:
    d = pathlib.Path(tmp)
    for folder in ['home', 'config/rdwm', 'run', 'data', 'cache']:
        (d / folder).mkdir(parents=True, exist_ok=True)
    (d / 'run').chmod(0o700)
    env = dict(os.environ, HOME=str(d/'home'), XDG_CONFIG_HOME=str(d/'config'), XDG_DATA_HOME=str(d/'data'), XDG_CACHE_HOME=str(d/'cache'), XDG_RUNTIME_DIR=str(d/'run'), DECEBAL_BACKEND='x11', XDG_SESSION_TYPE='x11', DECEBAL_SYSTEM_AUTOMATIC='0', RDWMCTL=str(RDWM/'rdwmctl'), DECSETTINGS_CONFIG=str(d/'config/rdwm/config.toml'))
    for k in ['WAYLAND_DISPLAY', 'RDWM_SOCK', 'RDWM_CONFIG', 'DECEBAL_CONFIG', 'DBUS_SESSION_BUS_ADDRESS', 'DECEBAL_PALETTES']:
        env.pop(k, None)
    cfg = d / 'config/rdwm/config.toml'
    cfg.write_text('[general]\ntags = ["1", "2"]\n[decoration]\nenabled = true\n[wallpaper]\ncolor = "#123456"\n')
    def run(*args):
        return subprocess.run([str(a) for a in args], env=env, check=True, capture_output=True, text=True, timeout=30).stdout
    def ipc(*args):
        v = json.loads(run(RDWM/'rdwmctl', *args))
        return v.get('state', v)
    def status():
        return json.loads((d/'run/decwall-status.json').read_text())
    def player():
        return status()['players'][0]
    def mpv(command):
        with socket.socket(socket.AF_UNIX) as s:
            s.settimeout(2)
            s.connect(str(next((d/'run').glob('decwall-*-*.sock'))))
            s.sendall((json.dumps({'command':command,'request_id':97})+'\n').encode())
            with s.makefile('r') as f:
                for line in f:
                    v=json.loads(line)
                    if v.get('request_id') == 97:
                        assert v['error'] == 'success', v
                        return v.get('data')
    try:
        r,w=os.pipe()
        xv=subprocess.Popen(['Xvfb','-displayfd',str(w),'-screen','0','960x640x24','-nolisten','tcp'],pass_fds=(w,),stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        procs.append(xv);os.close(w)
        with os.fdopen(r) as f: env['DISPLAY']=':'+f.readline().strip()
        wm=subprocess.Popen([str(RDWM/'rdwm'),'--backend','x11','--config',str(cfg)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);procs.append(wm)
        wait(lambda:(d/'run/rdwm.sock').exists())
        run('ffmpeg','-nostdin','-v','error','-f','lavfi','-i','testsrc2=size=320x180:rate=24','-t','20','-c:v','libvpx','-threads','1','-y',d/'loop.webm')
        run(BIN/'decwall','set',d/'loop.webm')
        wait(lambda:player()['paused'] is False)
        check(player()['alive'], 'real mpv player starts on private desktop')
        check(not status()['errors'], 'no playback errors')
        saved=json.loads((d/'config/decebal/live-wallpaper.json').read_text())
        check(pathlib.Path(saved['poster']).is_file(), 'video still frame is persisted')
        check('poster-' in cfg.read_text(), 'rdwm static wallpaper points at the still frame')
        t1=mpv(['get_property','time-pos']);time.sleep(1)
        check(mpv(['get_property','time-pos'])>t1+.3,'video advances while desktop is visible')
        app=subprocess.Popen([str(RDWM/'examples/xclient'),'--title','Live wallpaper test'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);procs.append(app)
        wait(lambda:ipc('state')['windows'])
        check(all(w['class'] != 'Decwall' for w in ipc('state')['windows']), 'wallpaper is not tiled or managed as an app')
        run(RDWM/'rdwmctl','action','togglefullscreen')
        wait(lambda:player()['paused'] is True)
        check(mpv(['get_property','pause']) is True,'fullscreen pauses the decoder')
        time.sleep(.3); t1=mpv(['get_property','time-pos']);time.sleep(1)
        check(abs(mpv(['get_property','time-pos'])-t1)<.05,'fullscreen keeps a frozen frame without decoding')
        mpv(['screenshot-to-file',str(d/'frame1.png'),'video']);time.sleep(.3)
        mpv(['screenshot-to-file',str(d/'frame2.png'),'video'])
        check((d/'frame1.png').read_bytes()==(d/'frame2.png').read_bytes(),'paused wallpaper retains identical visible frame')
        run(RDWM/'rdwmctl','action','view','2')
        wait(lambda:player()['paused'] is False)
        check(mpv(['get_property','pause']) is False,'fullscreen on hidden workspace does not pause playback')
        run(RDWM/'rdwmctl','action','view','1')
        wait(lambda:player()['paused'] is True)
        run(RDWM/'rdwmctl','action','togglefullscreen')
        wait(lambda:player()['paused'] is False)
        check(mpv(['get_property','pause']) is False,'leaving fullscreen resumes playback')
        pid=player()['pid']
        # A normal wallpaper choice supersedes animation, including runtime IPC choices.
        run(RDWM/'rdwmctl','wallpaper','--clear')
        wait(lambda:json.loads((d/'config/decebal/live-wallpaper.json').read_text())['enabled'] is False)
        wait(lambda:not pathlib.Path(f'/proc/{pid}').exists())
        check(True,'selecting static wallpaper stops and reaps the video player')
        check(not (d/'run/decwall-status.json').exists(),'host cleans up runtime status on stop')
        # Invalid media must not change either persistent configuration.
        before=cfg.read_text(); (d/'bad.webm').write_text('not a video')
        bad=subprocess.run([str(BIN/'decwall'),'set',str(d/'bad.webm')],env=env,capture_output=True,timeout=30)
        check(bad.returncode!=0 and cfg.read_text()==before,'invalid video leaves current wallpaper unchanged')
        # Exercise the real Settings tab without using the public network.
        env['DECSETTINGS_DEBUG_LAYOUT']=str(d/'layout.json')
        settings=subprocess.Popen([str(BIN/'decsettings'),'wallpaper'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);procs.append(settings)
        def layout(): return json.loads((d/'layout.json').read_text())
        tabs=wait(lambda:next((c for c in layout()['ctls'] if c['id']=='wh:tab'),None))
        check(layout()['pane']=='wallpaper','Wallpaper opens as a standalone sidebar section')
        wid=str(wait(lambda:next((w['id'] for w in ipc('state')['windows'] if w['class']=='DecSettings'),None)))
        run('xdotool','windowactivate','--sync',wid)
        time.sleep(.3)
        tabs=next(c for c in layout()['ctls'] if c['id']=='wh:tab')
        r=tabs['parts'][2]
        run('xdotool','mousemove','--window',wid,int(r[0]+r[2]/2),int(r[1]+r[3]/2))
        run('xdotool','click','1')
        wait(lambda:any(c['id']=='lw:path' for c in layout()['ctls']))
        check(True,'Settings opens the Live wallpapers tab')
        check(any(c['id']=='lw:pause' for c in layout()['ctls']),'Settings exposes fullscreen power control')
        check(any(c['id']=='th:dynamic' for c in layout()['ctls']),'Wallpaper has a direct colour-matching box')
        def click_control(cid):
            for _ in range(35):
                candidate=next((c for c in layout()['ctls'] if c['id']==cid),None)
                if candidate:
                    r=candidate['rect']
                    if 80 < r[1] < 550:
                        run('xdotool','mousemove','--window',wid,int(r[0]+r[2]/2),int(r[1]+r[3]/2));run('xdotool','click','1');return
                run('xdotool','mousemove','--window',wid,490,520);run('xdotool','click','5');time.sleep(.1)
            raise AssertionError('control not reachable: '+cid)
        shellpath=d/'config/omarchy/decebal.json'
        click_control('th:dynamic')
        wait(lambda:json.loads(shellpath.read_text())['theme']['dynamic']['enabled'] is True)
        base_theme=json.loads(shellpath.read_text())['theme']['name']
        check(True,'matching wallpaper colours can be enabled directly from Wallpaper')
        click_control('th:dyn-palette-save')
        wait(lambda:list((d/'config/decebal/palettes').glob('wallpaper-*.toml')))
        import tomllib
        palette=tomllib.loads(next((d/'config/decebal/palettes').glob('wallpaper-*.toml')).read_text())
        check('dark' in palette and 'light' in palette,'Create and apply saves both palette variants')
        wait(lambda:json.loads(shellpath.read_text())['theme'].get('palette','').startswith('wallpaper-'))
        check(json.loads(shellpath.read_text())['theme']['name']==base_theme,'generated wallpaper palette is applied without changing the theme design')
        shot=pathlib.Path(os.environ.get('LIVE_WALLPAPER_SHOT','/tmp/decebal-live-wallpapers-settings.png'))
        run('import','-window',wid,shot)
        print('Settings screenshot:',shot)
        print(f'{checks}/{checks} checks passed')
    finally:
        # Only signal the daemon in this private runtime directory.
        lock=d/'run/decwall.lock'
        if lock.exists():
            try: os.kill(int(lock.read_text().strip()),signal.SIGTERM)
            except (OSError,ValueError): pass
        for p in reversed(procs):
            if p.poll() is None:
                p.terminate()
                try:p.wait(timeout=5)
                except subprocess.TimeoutExpired:p.kill();p.wait()
