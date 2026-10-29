#!/usr/bin/env python3
"""Stay Awake controls the real X screensaver on an isolated display, not just its icon."""
import json, os, pathlib, re, signal, subprocess, tempfile, time
ROOT=pathlib.Path(__file__).resolve().parents[2]
BIN=pathlib.Path(os.environ.get('MS_BIN_DIR',str(ROOT/'target/debug')))
with tempfile.TemporaryDirectory(prefix='decebal-idle-test-') as tmp:
    d=pathlib.Path(tmp)
    for folder in ['run','home','config','state']:(d/folder).mkdir()
    (d/'run').chmod(0o700)
    env=dict(os.environ,HOME=str(d/'home'),XDG_RUNTIME_DIR=str(d/'run'),XDG_STATE_HOME=str(d/'state'),XDG_CONFIG_HOME=str(d/'config'),DECEBAL_CONFIG=str(d/'config'),DECEBAL_BACKEND='x11',DECEBAL_SYSTEM_AUTOMATIC='0',DECBAR_DEBUG_TRAY=str(d/'tray'),DECUTIL_QUIET='1',XDG_DATA_HOME=str(d/'data'),XDG_CACHE_HOME=str(d/'cache'))
    for key in ['WAYLAND_DISPLAY','DBUS_SESSION_BUS_ADDRESS']:env.pop(key,None)
    def run(*a):return subprocess.run([str(x) for x in a],env=env,capture_output=True,text=True,check=True,timeout=15).stdout
    def timeout():return int(re.search(r'timeout:\s+(\d+)',run('xset','q'))[1])
    def wait(f):
        end=time.monotonic()+12
        while time.monotonic()<end:
            try:
                if f():return
            except (OSError,ValueError,subprocess.CalledProcessError):pass
            time.sleep(.1)
        raise AssertionError('timed out')
    count=0
    def check(v,t):
        global count
        assert v,t
        count+=1;print('PASS:',t,flush=True)
    bar=None;r,w=os.pipe()
    xv=subprocess.Popen(['Xvfb','-noreset','-displayfd',str(w),'-screen','0','1200x800x24','-nolisten','tcp'],pass_fds=(w,),stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    os.close(w)
    with os.fdopen(r) as f:env['DISPLAY']=':'+f.readline().strip()
    indicator=d/'state/omarchy/indicators/stay-awake'
    def startbar():return subprocess.Popen([str(BIN/'decbar')],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    try:
        run('xset','s','700','10')
        run(BIN/'decutil','idle-inhibit','on')
        check(timeout()==0 and indicator.exists(),'enabling disables the real screensaver and sets the indicator')
        indicator.unlink();run(BIN/'decutil','idle-inhibit','on')
        check(timeout()==0 and indicator.exists(),'an explicit on repairs stale indicator state without losing the original timeout')
        run(BIN/'decutil','idle-inhibit','off')
        check(timeout()==700 and not indicator.exists(),'disabling restores the original timeout and clears the indicator')
        indicator.parent.mkdir(parents=True,exist_ok=True);indicator.touch()
        bar=startbar();wait(lambda:timeout()==0)
        check(True,'bar reconciles an existing Stay Awake indicator into real inhibition')
        wait(lambda:(d/'tray').exists() and 'Awake ' in (d/'tray').read_text())
        cup=next(l.split() for l in (d/'tray').read_text().splitlines() if l.startswith('Awake '))
        run('xdotool','mousemove',cup[1],cup[2] if len(cup)>2 else '12');run('xdotool','click','1')
        wait(lambda:not indicator.exists() and timeout()==700)
        check(True,'clicking the coffee cup restores real idle locking')
        bar.terminate();bar.wait(timeout=5);bar=None
        indicator.touch();until=d/'state/decebal/stay-awake-until';until.parent.mkdir(parents=True,exist_ok=True);until.write_text(str(int(time.time())-5))
        bar=startbar();wait(lambda:not indicator.exists())
        check(timeout()==700 and not until.exists(),'expired timer stays off and leaves the original timeout intact')
        print(f'{count}/{count} checks passed')
    finally:
        if bar and bar.poll() is None:bar.terminate();bar.wait(timeout=5)
        xv.terminate();xv.wait(timeout=5)
