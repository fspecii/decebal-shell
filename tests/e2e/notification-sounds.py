#!/usr/bin/env python3
"""Private native bus/X11 notification sound test. A fake player NEVER emits audio.
NOTIFY_BINARY=/absolute/decnotify python3 tests/e2e/notification-sounds.py [OUT]
"""
import hashlib, json, os, signal, subprocess, sys, tempfile, time
from pathlib import Path
import dbus
binary = Path(os.environ['NOTIFY_BINARY']).resolve()
out = Path(sys.argv[1] if len(sys.argv) > 1 else tempfile.mkdtemp(prefix='notification-sounds-')).resolve()
out.mkdir(parents=True, exist_ok=True)
procs, logs, results = [], [], []
def check(label, ok):
    results.append({'check':label, 'passed':bool(ok)})
    assert ok, label
    print('PASS',label,flush=True)
def wait(fn, timeout=8):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            r=fn()
            if r:return r
        except (OSError,ValueError,dbus.DBusException):pass
        time.sleep(.03)
    raise AssertionError('private wait timed out')
with tempfile.TemporaryDirectory(prefix='notification-sound-session-') as tmp:
    root=Path(tmp);env=dict(os.environ)
    for key in list(env):
        if key.startswith(('DECEBAL_','DECNOTIFY_','XDG_','DBUS_','RDWM')) or key in ('WAYLAND_DISPLAY','XAUTHORITY','DISPLAY','SESSION_MANAGER','AT_SPI_BUS_ADDRESS'):env.pop(key,None)
    for key,name in {'HOME':'home','XDG_CONFIG_HOME':'config','XDG_DATA_HOME':'data','XDG_STATE_HOME':'state','XDG_CACHE_HOME':'cache','XDG_RUNTIME_DIR':'run','XDG_CONFIG_DIRS':'empty-cfg','XDG_DATA_DIRS':'empty-data','DECEBAL_CONFIG':'shell','DECEBAL_THEMES':'themes','DECEBAL_SYSTEM_THEMES':'system-themes'}.items():
        p=root/name;p.mkdir(mode=0o700);env[key]=str(p)
    number=next(n for n in range(610,690) if not Path(f'/tmp/.X11-unix/X{n}').exists() and not Path(f'/tmp/.X{n}-lock').exists())
    env.update(DISPLAY=f':{number}',XDG_SESSION_TYPE='x11',DECEBAL_BACKEND='x11',DBUS_SESSION_BUS_ADDRESS='unix:path='+str(root/'bus'),DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(root/'no-system'),DECEBAL_SYSTEM_AUTOMATIC='0',DECNOTIFY_FORCE='1',SOUND_LOG=str(root/'sounds.jsonl'),SOUND_MODE=str(root/'mode'))
    shims=root/'bin';shims.mkdir();env['PATH']=str(shims)+':/usr/bin:/bin'
    player=shims/'canberra-gtk-play'
    player.write_text('''#!/usr/bin/python3
import os,sys,json,time
from pathlib import Path
record={'pid':os.getpid(),'args':sys.argv[1:]}
if '--file' in sys.argv:record['header']=open(sys.argv[-1],'rb').read(12).hex()
mode=Path(os.environ['SOUND_MODE']).read_text()
if mode=='hang':
 child=os.fork()
 if child==0:
  time.sleep(60);os._exit(0)
 record['child']=child
with open(os.environ['SOUND_LOG'],'a') as f:f.write(json.dumps(record)+'\\n')
if mode=='hang':time.sleep(60)
if mode=='fail':sys.exit(2)
''');player.chmod(0o755);(root/'mode').write_text('ok')
    cfg=root/'config/decebal/notifications.toml';cfg.parent.mkdir()
    def config(sounds=False,dnd=False,muted=False):
        t=cfg.with_suffix('.new');t.write_text(f'sounds = {str(sounds).lower()}\ndnd = {str(dnd).lower()}\nmuted_apps = '+('["Silent"]' if muted else '[]')+'\nfuture_key = "keep"\n');os.replace(t,cfg)
    def start(args,label):
        log=(out/(label+'.log')).open('w');logs.append(log);p=subprocess.Popen(args,env=env,stdout=log,stderr=log,start_new_session=True);procs.append(p);return p
    def records():
        p=root/'sounds.jsonl';return [json.loads(x) for x in p.read_text().splitlines()] if p.exists() else []
    try:
        config()
        start(['dbus-daemon','--session','--nofork','--address='+env['DBUS_SESSION_BUS_ADDRESS']],'bus');wait(lambda:(root/'bus').is_socket())
        xvfb=start(['Xvfb',env['DISPLAY'],'-screen','0','1024x768x24','-nolisten','tcp'],'xvfb')
        wait(lambda:Path(f'/tmp/.X{number}-lock').exists())
        check('owned private display',int(Path(f'/tmp/.X{number}-lock').read_text().strip())==xvfb.pid and xvfb.poll() is None)
        daemon=start([str(binary)],'daemon');bus=dbus.bus.BusConnection(env['DBUS_SESSION_BUS_ADDRESS']);wait(lambda:bus.name_has_owner('org.freedesktop.Notifications'))
        interface=dbus.Interface(bus.get_object('org.freedesktop.Notifications','/org/freedesktop/Notifications'),'org.freedesktop.Notifications')
        check('sound capability with optional backend','sound' in interface.GetCapabilities())
        def send(hints=None,replaces=0,app='Test',timeout=5000):return int(interface.Notify(app,dbus.UInt32(replaces),'','Sound test','Private synthetic data',dbus.Array([],signature='s'),dbus.Dictionary(hints or {},signature='sv'),dbus.Int32(timeout)))
        send();time.sleep(.2);check('default off is silent',not records())
        config(True);identifier=send({'sound-name':'complete'});wait(lambda:len(records())==1);check('named event uses argv',records()[0]['args']==['--id','complete'])
        send({'sound-name':'message-new-instant'},identifier);send({'suppress-sound':dbus.Boolean(True)});config(True,True);send({'urgency':dbus.Byte(2)});config(True,False,True);send(app='Silent');time.sleep(1.1);check('replacement suppress-sound DND critical and muted stay silent',len(records())==1)
        config(True)
        wav=root/'sample.wav';wav.write_bytes(b'RIFF\x04\x00\x00\x00WAVE')
        send({'sound-file':str(wav)});wait(lambda:len(records())==2);check('validated file descriptor reaches player',records()[1]['args']==['--file','/proc/self/fd/3'] and records()[1]['header']==wav.read_bytes().hex())
        time.sleep(1.1);send({'sound-name':'complete','sound-file':str(wav)});wait(lambda:len(records())==3);check('sound-name wins over sound-file',records()[2]['args']==['--id','complete'])
        for _ in range(50):send({'sound-name':'complete'})
        time.sleep(.3);check('flood rate limited without backlog',len(records())==3)
        time.sleep(1.1)
        bad=root/'not-audio';bad.write_bytes(b'not audio at all');link=root/'symlink.wav';link.symlink_to(wav)
        send({'sound-file':str(bad)});time.sleep(1.1);send({'sound-file':str(link)});time.sleep(1.1);send({'sound-file':'https://invalid.example/audio.wav'});time.sleep(.2);check('invalid symlink and remote files rejected',len(records())==3)
        (root/'mode').write_text('fail');send({'sound-name':'complete'});wait(lambda:len(records())==4);time.sleep(1.1);(root/'mode').write_text('ok');send({'sound-name':'complete'});wait(lambda:len(records())==5);check('failed player does not stop later sounds',daemon.poll() is None)
        time.sleep(1.1);(root/'mode').write_text('hang');send({'sound-name':'complete'});wait(lambda:len(records())==6);hung=records()[-1];send({'sound-name':'complete'},timeout=1)
        def gone(pid):
            p=Path(f'/proc/{pid}/stat');return not p.exists() or p.read_text().rsplit(')',1)[1].split()[0]=='Z'
        wait(lambda:gone(hung['pid']) and gone(hung['child']),7)
        check('hung player group terminated and direct child reaped',not Path(f"/proc/{hung['pid']}").exists() and gone(hung['child']))
        (root/'mode').write_text('ok');time.sleep(.3);check('expired queued sound never plays',len(records())==6)
        send({'sound-name':'complete'});wait(lambda:len(records())==7);check('worker recovers after timeout',daemon.poll() is None)
        config(False);time.sleep(1.1);send();time.sleep(.2);check('runtime disable is silent',len(records())==7)
        (out/'player-records.json').write_text(json.dumps(records(),indent=2)+'\n')
    finally:
        for p in reversed(procs):
            if p.poll() is None:
                os.killpg(p.pid,signal.SIGTERM)
                try:p.wait(timeout=3)
                except subprocess.TimeoutExpired:os.killpg(p.pid,signal.SIGKILL);p.wait()
        for log in logs:log.close()
(out/'result.json').write_text(json.dumps({'binary':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'checks':results,'passed':all(x['passed'] for x in results)},indent=2)+'\n')
print('PASS notification sounds private native regression',out)
