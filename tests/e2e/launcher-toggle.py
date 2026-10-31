#!/usr/bin/env python3
"""Private launcher singleton/launch-monitor regression; never sends live input.
LAUNCHER_BIN_DIR chooses the binaries; LAUNCHAPP_BIN the monitored launch helper.
EXPECT_TOGGLE_FAILURE=1 proves the regression against an old installed binary.
"""
from pathlib import Path
import fcntl, hashlib, json, os, shutil, signal, subprocess, tempfile, time
# -I omits the script directory; import only this trusted fixture directory.
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
from private_x11 import start_private_xvfb
repo=Path(__file__).resolve().parents[2]
bins=Path(os.environ.get('LAUNCHER_BIN_DIR',repo/'target/debug'))
helper=Path(os.environ.get('LAUNCHAPP_BIN',repo/'target/debug/examples/launchapp'))
root=Path(tempfile.mkdtemp(prefix='decebal-launcher-toggle-'));root.chmod(0o700)
evidence=Path(os.environ.get('LAUNCHER_TOGGLE_EVIDENCE',root/'evidence'));evidence.mkdir(parents=True,exist_ok=True)
env=dict(os.environ)
for k in list(env):
    if k in ('DISPLAY','WAYLAND_DISPLAY','XAUTHORITY','RDWM_SOCK','SESSION_MANAGER','AT_SPI_BUS_ADDRESS') or k.startswith(('DECEBAL_','DECSPOT_','DECSETTINGS_')):env.pop(k,None)
for k,v in {'HOME':'home','XDG_CONFIG_HOME':'cfg','XDG_CONFIG_DIRS':'nocfg','XDG_DATA_HOME':'data','XDG_DATA_DIRS':'nodata','XDG_CACHE_HOME':'cache','XDG_STATE_HOME':'state','XDG_RUNTIME_DIR':'run','DECEBAL_CONFIG':'shell','DECEBAL_THEMES':'themes','DECEBAL_SYSTEM_THEMES':'systemthemes'}.items():
    env[k]=str(root/v);Path(env[k]).mkdir(parents=True,exist_ok=True)
os.chmod(env['XDG_RUNTIME_DIR'],0o700)
env.update(DECEBAL_BACKEND='x11',DBUS_SESSION_BUS_ADDRESS='unix:path='+str(root/'none-bus'),DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(root/'none-system'))
notice=root/'notify.sh';notice.write_text('#!/bin/sh\nprintf "%s\\n" "$@" >> "$XDG_STATE_HOME/notices"\n');notice.chmod(0o755);env['DECEBAL_NOTIFY']=str(notice)
applications=Path(env['XDG_DATA_HOME'])/'applications';applications.mkdir()
owned=[];logs=[];known=set()
def start(args,label,masked_toggle=False):
    f=(root/(label+'.log')).open('w');logs.append(f)
    p=subprocess.Popen(list(map(str,args)),env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True,preexec_fn=(lambda:signal.pthread_sigmask(signal.SIG_BLOCK,{signal.SIGUSR1})) if masked_toggle else None);owned.append(p);return p
def wait(fn,message,seconds=8):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        value=fn()
        if value:return value
        time.sleep(.01)
    raise AssertionError(message)
def holder(name):
    path=Path(env['XDG_RUNTIME_DIR'])/(name+'.lock')
    try:
        with path.open('r') as f:
            try:fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB);return None
            except BlockingIOError:
                pid=int(f.read().strip());known.add(pid);return pid
    except (FileNotFoundError,ValueError):return None
def launchlog():
    p=Path(env['XDG_STATE_HOME'])/'decebal/launch.log';return p.read_text() if p.exists() else ''
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
records=[]
try:
    log=(root/'xvfb.log').open('w');logs.append(log)
    xvfb,env['DISPLAY']=start_private_xvfb(env=env,log=log);owned.append(xvfb)
    for name in ['declaunch','decspot']:
        binary=bins/name
        entry=applications/(name+'-toggle.desktop');entry.write_text('[Desktop Entry]\nType=Application\nName=Toggle fixture\nExec='+str(binary)+'\n')
        shutil.rmtree(Path(env["XDG_CACHE_HOME"])/"decebal",ignore_errors=True)
        # The fastest close is sent immediately after lock publication, before
        # font/display setup; the target may not have created its window yet.
        for i in range(8):
            before=launchlog();monitor=start([helper,entry.stem,'2'],name+'-monitor-'+str(i),masked_toggle=i==0)
            pid=wait(lambda:holder(name),name+' never acquired lock')
            assert subprocess.run([str(binary)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,timeout=8).returncode==0
            wait(lambda:holder(name) is None,name+' toggle did not release lock')
            records.append({'app':name,'round':i,'pid':pid,'early_close':True,'inherited_usr1_blocked':i==0})
        # Let one actual window reach the display, then close it repeatedly too.
        for i in range(3):
            app=start([binary],name+'-window-'+str(i));wait(lambda:holder(name),name+' window lock missing')
            def mapped():
                found=subprocess.run(['xdotool','search','--onlyvisible','--class',name],env=env,capture_output=True,text=True,timeout=3)
                return bool(found.stdout.strip())
            wait(mapped,name+' did not map a visible window')
            subprocess.run([str(binary)],env=env,check=True,timeout=8)
            status=app.wait(timeout=8)
            if os.environ.get('EXPECT_TOGGLE_FAILURE')!='1':assert status==0,(name,status)
            records.append({'app':name,'window_round':i,'exit':status})
    for p in owned[1:]:
        p.wait(timeout=8)
    # A real app launched from the grid must not inherit the launcher's handler
    # or a blocked toggle/termination signal across exec.
    for entry in applications.glob('*.desktop'):entry.unlink()
    child=root/'signal-child.py';child_report=root/'signal-child.json'
    child.write_text('import json,signal\nfrom pathlib import Path\nmask=signal.pthread_sigmask(signal.SIG_BLOCK,[])\nPath('+repr(str(child_report))+').write_text(json.dumps({"usr1_blocked":signal.SIGUSR1 in mask,"term_blocked":signal.SIGTERM in mask,"usr1_default":signal.getsignal(signal.SIGUSR1)==signal.SIG_DFL}))\n')
    (applications/'signal-inheritance.desktop').write_text('[Desktop Entry]\nType=Application\nName=Signal inheritance fixture\nExec=/usr/bin/python3 '+str(child)+'\n')
    shutil.rmtree(Path(env["XDG_CACHE_HOME"])/"decebal",ignore_errors=True)
    grid=start([bins/'declaunch'],'signal-inheritance-grid')
    wait(lambda:subprocess.run(['xdotool','search','--onlyvisible','--class','declaunch'],env=env,capture_output=True,timeout=3).stdout,'signal inheritance grid did not map')
    subprocess.run(['xdotool','type','--clearmodifiers','--delay','1','Signal inheritance fixture'],env=env,check=True,timeout=5)
    subprocess.run(['xdotool','key','Return'],env=env,check=True,timeout=5)
    wait(child_report.exists,'grid did not launch signal inheritance fixture')
    inheritance=json.loads(child_report.read_text())
    assert inheritance=={'usr1_blocked':False,'term_blocked':False,'usr1_default':True},inheritance
    assert grid.wait(timeout=8)==0
    text=launchlog();toggle_errors=text.count('EXITED EARLY')
    expected=os.environ.get('EXPECT_TOGGLE_FAILURE')=='1'
    assert (toggle_errors>0) if expected else toggle_errors==0,('toggle monitor errors',toggle_errors,text)
    # Genuine termination is still reported by the exact same launch monitor.
    failure=applications/'genuine-term.desktop';failure.write_text('[Desktop Entry]\nType=Application\nName=Genuine termination fixture\nExec=/bin/sh -c "kill -TERM $$"\n')
    shutil.rmtree(Path(env["XDG_CACHE_HOME"])/"decebal",ignore_errors=True)
    p=start([helper,failure.stem,'2'],'genuine-term');p.wait(timeout=8)
    after=launchlog();assert after.count('EXITED EARLY')==toggle_errors+1,after
    assert 'signal: 15' in after,after
    result={'passed':True,'expected_old_failure':expected,'toggle_early_errors':toggle_errors,'genuine_sigterm_reported':True,'spawned_app_signal_inheritance':inheritance,'rounds':records,'binaries':{n:sha(bins/n) for n in ['declaunch','decspot']},'monitor_sha256':sha(helper),'private_root':str(root)}
    (evidence/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
finally:
    for pid in known:
        try:
            path=Path('/proc')/str(pid)/'environ'
            if path.exists() and (b'XDG_RUNTIME_DIR='+env['XDG_RUNTIME_DIR'].encode()+b'\0') in path.read_bytes():os.kill(pid,signal.SIGKILL)
        except (FileNotFoundError,ProcessLookupError):pass
    for p in reversed(owned):
        if p.poll() is None:p.kill()
        try:p.wait(timeout=5)
        except subprocess.TimeoutExpired:pass
    for f in logs:f.close()
