#!/usr/bin/env python3
"""Owned Xvfb/process proof that concurrent launcher toggles do not coalesce.
LAUNCHER_BIN_DIR selects pinned binaries; EXPECT_COALESCING=1 proves the old bug.
A stopped owned holder models delayed shutdown; no live input or IPC is used.
"""
from pathlib import Path
import fcntl, hashlib, json, os, signal, subprocess, tempfile, time
# -I omits the script directory; import only this trusted fixture directory.
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
from private_x11 import start_private_xvfb
repo=Path(__file__).resolve().parents[2]
bins=Path(os.environ.get('LAUNCHER_BIN_DIR',repo/'target/debug')).resolve()
root=Path(tempfile.mkdtemp(prefix='launcher-concurrent-',dir='/var/tmp'));root.chmod(0o700)
out=Path(os.environ.get('CONCURRENT_EVIDENCE',root/'evidence'));out.mkdir(parents=True,exist_ok=True)
env={k:v for k,v in os.environ.items() if not k.startswith(('DECEBAL_','DECSPOT_','DECSETTINGS_','MAC'))}
for k in ['DISPLAY','WAYLAND_DISPLAY','XAUTHORITY','RDWM_SOCK','SESSION_MANAGER','AT_SPI_BUS_ADDRESS']:env.pop(k,None)
for k,d in {'HOME':'home','XDG_CONFIG_HOME':'cfg','XDG_CONFIG_DIRS':'nocfg','XDG_DATA_HOME':'data','XDG_DATA_DIRS':'nodata','XDG_CACHE_HOME':'cache','XDG_STATE_HOME':'state','XDG_RUNTIME_DIR':'run','DECEBAL_CONFIG':'shell','DECEBAL_THEMES':'themes','DECEBAL_SYSTEM_THEMES':'system-themes'}.items():
 env[k]=str(root/d);Path(env[k]).mkdir()
os.chmod(env['XDG_RUNTIME_DIR'],0o700)
env.update(DBUS_SESSION_BUS_ADDRESS='unix:path='+str(root/'no-bus'),DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(root/'no-system'),DECEBAL_BACKEND='x11',TMPDIR='/var/tmp')
owned=[];logs=[];records=[]
def start(args,label,pass_fds=()):
 f=(root/(label+'.log')).open('w');logs.append(f);p=subprocess.Popen(list(map(str,args)),env=env,stdout=f,stderr=f,start_new_session=True,pass_fds=pass_fds);owned.append(p);return p
def wait(fn,label,seconds=5):
 until=time.monotonic()+seconds
 while time.monotonic()<until:
  v=fn()
  if v:return v
  time.sleep(.01)
 raise AssertionError(label)
def holder(name):
 try:
  with (Path(env['XDG_RUNTIME_DIR'])/(name+'.lock')).open() as f:
   try:fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB);return None
   except BlockingIOError:
    text=f.read().strip();return int(text) if text.isdigit() else None
 except FileNotFoundError:return None
try:
 log=(root/'xvfb.log').open('w');logs.append(log)
 server,env['DISPLAY']=start_private_xvfb(env=env,log=log);owned.append(server)
 expected_old=os.environ.get('EXPECT_COALESCING')=='1'
 for name in ['declaunch','decspot']:
  app=bins/name
  first=start([app],name+'-first');pid=wait(lambda:holder(name),name+' first lock');assert pid==first.pid
  # Only our own isolated process can receive this deliberate test stop.
  procenv=dict(x.split(b'=',1) for x in Path(f'/proc/{pid}/environ').read_bytes().split(b'\0') if b'=' in x);assert procenv[b'XDG_RUNTIME_DIR'].decode()==env['XDG_RUNTIME_DIR']
  os.kill(pid,signal.SIGSTOP)
  second=start([app],name+'-second');third=start([app],name+'-third')
  if expected_old:wait(lambda:second.poll() is not None and third.poll() is not None,'old requests must return before holder exits')
  else:
   time.sleep(.2);assert second.poll() is None and third.poll() is None,'requests must wait for serialized shutdown'
  os.kill(pid,signal.SIGCONT);assert first.wait(timeout=5)==0
  if expected_old:
   time.sleep(.1);assert holder(name) is None,'baseline must demonstrate coalesced odd toggles';survivor=None
  else:
   survivor=wait(lambda:holder(name),'three toggles must leave one launcher open');assert survivor in [second.pid,third.pid]
   request=second if survivor==third.pid else third;assert request.wait(timeout=5)==0
   fourth=start([app],name+'-fourth');assert fourth.wait(timeout=5)==0
   wait(lambda:holder(name) is None,'fourth toggle closes the final holder')
   assert (third if survivor==third.pid else second).wait(timeout=5)==0
   # A real concurrent burst must preserve toggle parity too, with no input.
   for count in [7,8]:
    burst=[start([app],name+'-burst-'+str(count)+'-'+str(i)) for i in range(count)]
    deadline=time.monotonic()+10
    while time.monotonic()<deadline:
     live=[p for p in burst if p.poll() is None]
     if len(live)==count%2 and (holder(name) is not None)==bool(count%2):break
     time.sleep(.01)
    else:raise AssertionError((name,count,'burst parity or shutdown did not settle'))
    if count%2:
     close=start([app],name+'-burst-close');assert close.wait(timeout=5)==0
    for p in burst:assert p.wait(timeout=5)==0
    assert holder(name) is None
  records.append({'app':name,'odd_three_survives':survivor is not None,'baseline_coalescing':expected_old})
 result={'passed':True,'expected_old_coalescing':expected_old,'cases':records,'binaries':{n:hashlib.sha256((bins/n).read_bytes()).hexdigest() for n in ['declaunch','decspot']},'private_root':str(root)}
 (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
finally:
 for p in reversed(owned):
  if p.poll() is None:
   try:os.kill(p.pid,signal.SIGCONT);p.terminate();p.wait(timeout=3)
   except (ProcessLookupError,subprocess.TimeoutExpired):
    if p.poll() is None:p.kill();p.wait()
 for f in logs:f.close()
