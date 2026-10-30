#!/usr/bin/python3
"""Persistent native toast/history repaint from resolved themes; private Xvfb/D-Bus only."""
import json, os, re, signal, shutil, subprocess, tempfile, time
from pathlib import Path
import dbus
from PIL import Image
repo=Path(__file__).resolve().parents[2]
bins=Path(os.environ.get('MS_BIN_DIR', repo/'target/debug'))
for command in ('Xvfb','dbus-daemon','xdotool','import'):
    if not shutil.which(command): raise SystemExit('Missing '+command)
number=os.environ.get('E2E_DISPLAY', ':367')
assert re.fullmatch(r':[1-9][0-9]{1,3}',number)
assert not Path('/tmp/.X11-unix/X'+number[1:]).exists()
assert not Path('/tmp/.X'+number[1:]+'-lock').exists()
root=Path(tempfile.mkdtemp(prefix='decebal-notify-theme.'))
evidence=Path(os.environ.get('NOTIFY_THEME_EVIDENCE_DIR', root/'evidence'));evidence.mkdir(parents=True,exist_ok=True)
env=dict(os.environ)
for key in list(env):
    if key in ('WAYLAND_DISPLAY','XAUTHORITY','RDWM_SOCK','SESSION_MANAGER','AT_SPI_BUS_ADDRESS','DECEBAL_FONT_SCALE','DECEBAL_HIGH_CONTRAST','DECEBAL_REDUCE_MOTION') or key.startswith('DECNOTIFY_DEBUG_'): env.pop(key,None)
for key, child in {'HOME':'home','XDG_CONFIG_HOME':'cfg','XDG_CONFIG_DIRS':'noconfig','XDG_RUNTIME_DIR':'run','XDG_STATE_HOME':'state','XDG_DATA_HOME':'data','XDG_DATA_DIRS':'nodata','XDG_CACHE_HOME':'cache','DECEBAL_CONFIG':'shell','DECEBAL_THEMES':'themes','DECEBAL_SYSTEM_THEMES':'system-themes'}.items():
    env[key]=str(root/child);Path(env[key]).mkdir(parents=True,exist_ok=True)
os.chmod(env['XDG_RUNTIME_DIR'],0o700)
env.update(DISPLAY=number,DBUS_SESSION_BUS_ADDRESS='unix:path='+str(root/'bus'),DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(root/'none'),DECNOTIFY_FORCE='1',DECEBAL_BACKEND='x11',DECNOTIFY_DEBUG_HISTORY=str(root/'history-debug.json'))
settings=root/'shell/decebal.json';history=root/'state/decebal/notification-history.json'
(root/'cfg/decebal').mkdir();(root/'cfg/decebal/notifications.toml').write_text('dnd = false\nbanner_seconds = 120\n')
processes=[];files=[]
def start(args,label):
    f=(root/(label+'.log')).open('w');files.append(f)
    p=subprocess.Popen(list(map(str,args)),env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True);processes.append(p);return p
def run(*args): return subprocess.check_output(list(map(str,args)),env=env,stderr=subprocess.DEVNULL,text=True).strip()
def wait(check,message):
    end=time.monotonic()+10;last=None
    while time.monotonic()<end:
        try:
            value=check()
            if value:return value
        except (OSError,ValueError,KeyError,dbus.DBusException,subprocess.CalledProcessError,AssertionError) as e:last=e
        time.sleep(.06)
    raise AssertionError(message+': '+str(last))
def selection(value):
    tmp=settings.with_suffix('.new');tmp.write_text(json.dumps({'theme':value}));os.replace(tmp,settings)
def resolved(): return json.loads(run(bins/'decthemectl','get','--json'))
def expected():
    h=resolved()['colors']['popup'];return tuple(bytes.fromhex(h.lstrip('#')))
def capture(win,label):
    p=evidence/(label+'.png');run('import','-window',str(win),p);return Image.open(p).convert('RGB')
def has_background(win,wanted,label):
    image=capture(win,label)
    # An untouched interior fill, away from icon/text/buttons and rounded border.
    return image.getpixel((4,image.height//2))==wanted
B='org.freedesktop.Notifications';P='/org/freedesktop/Notifications';E='org.decebal.Notifications'
failed=True
try:
    start(['dbus-daemon','--session','--nofork','--address='+env['DBUS_SESSION_BUS_ADDRESS']],'bus')
    wait(lambda:(root/'bus').is_socket(),'private bus')
    start(['Xvfb',number,'-screen','0','1280x900x24','-nolisten','tcp'],'xvfb')
    wait(lambda:run('xdotool','getdisplaygeometry')=='1280 900','private Xvfb')
    bus=dbus.bus.BusConnection(env['DBUS_SESSION_BUS_ADDRESS'])
    selection({'name':'nord','mode':'dark'})
    daemon=start([bins/'decnotify'],'daemon');wait(lambda:bus.name_has_owner(B),'daemon owns private name')
    proxy=bus.get_object(B,P);notify=dbus.Interface(proxy,B);extension=dbus.Interface(proxy,E)
    ident=int(notify.Notify('Theme QA',dbus.UInt32(0),'','Theme coherence','Synthetic message',dbus.Array([],signature='s'),dbus.Dictionary({},signature='sv'),dbus.Int32(0)))
    banner=int(wait(lambda:run('xdotool','search','--onlyvisible','--classname','^decnotify$'),'toast visible').splitlines()[0])
    wait(lambda:history.exists(),'history saved');original=history.read_bytes()
    first=expected();wait(lambda:has_background(banner,first,'toast-nord'),'toast matches resolved Nord')
    selection({'name':'rose-pine','mode':'dark'})
    second=expected();assert first!=second
    wait(lambda:has_background(banner,second,'toast-rose-pine'),'same existing toast repainted')
    assert daemon.poll() is None and history.read_bytes()==original
    print('PASS persistent native toast repaints Nord→Rose Pine without replacement/history writes',flush=True)
    extension.ShowHistory()
    debug=root/'history-debug.json'
    panel=int(wait(lambda:json.loads(debug.read_text()).get('window'),'history opens'))
    wait(lambda:has_background(panel,second,'history-rose-pine'),'history uses same resolved palette')
    extension.SetDnd(dbus.Boolean(True));assert extension.GetDnd()
    for label,value in [
        ('independent-palette',{'name':'macos','mode':'dark','palette':'nord'}),
        ('dynamic',{'name':'macos','mode':'dark','dynamic':{'enabled':True,'surfaces':'full','strength':1,'scheme':'vibrant','source':'#c51772','seed':'#c51772','tone':35,'mode':'keep'}}),
        ('light',{'name':'macos','mode':'light'}),
        ('missing-theme',{'name':'missing-private-theme','mode':'dark'})]:
        selection(value);want=expected()
        wait(lambda:has_background(panel,want,'history-'+label),'persistent history '+label+' matches resolved popup')
        assert daemon.poll() is None and extension.GetDnd() and history.read_bytes()==original
        assert json.loads(debug.read_text())['window']==panel
        assert ident in [item['id'] for item in json.loads(debug.read_text())['items']]
        assert not extension.GetHistoryStatus()[0]
        assert notify.GetServerInformation()[0]=='decnotify' and 'body' in notify.GetCapabilities()
        if label=='missing-theme':assert resolved()['problem']
        print('PASS persistent history '+label+'; DND/history/window identity preserved',flush=True)
    (evidence/'result.json').write_text(json.dumps({'daemon_pid':daemon.pid,'toast_window':banner,'history_window':panel,'notification_id':ident,'history_unchanged':True,'dnd_preserved':True},indent=2)+'\n')
    failed=False
    print('PASS notification theme native QA; evidence '+str(evidence),flush=True)
finally:
    for p in reversed(processes):
        if p.poll() is None:
            os.killpg(p.pid,signal.SIGTERM)
            try:p.wait(timeout=3)
            except subprocess.TimeoutExpired:os.killpg(p.pid,signal.SIGKILL);p.wait()
    for f in files:f.close()
    if failed:print('Retained failure resources '+str(root),flush=True)
    elif not os.environ.get('KEEP_TMP'):shutil.rmtree(root)
