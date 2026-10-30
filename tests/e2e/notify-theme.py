#!/usr/bin/python3
"""Persistent native toast/history repaint from resolved themes; private Xvfb/D-Bus only."""
import hashlib, json, os, re, signal, shutil, subprocess, tempfile, time
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
env.update(DISPLAY=number,DBUS_SESSION_BUS_ADDRESS='unix:path='+str(root/'bus'),DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(root/'none'),DECNOTIFY_FORCE='1',DECEBAL_BACKEND='x11',DECNOTIFY_DEBUG_HISTORY=str(root/'history-debug.json'),DECNOTIFY_DEBUG_BANNERS=str(root/'banner-debug.json'))
settings=root/'shell/decebal.json';history=root/'state/decebal/notification-history.json'
(root/'cfg/decebal').mkdir();(root/'cfg/decebal/notifications.toml').write_text('dnd = false\nbanner_seconds = 120\n')
processes=[];files=[]
def start(args,label):
    f=(root/(label+'.log')).open('w');files.append(f)
    p=subprocess.Popen(list(map(str,args)),env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True);processes.append(p);return p
def run(*args): return subprocess.check_output(list(map(str,args)),env=env,stderr=subprocess.DEVNULL,text=True,timeout=5).strip()
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
    # Synthetic installed icon packs make pack selection observable without
    # depending on any machine-wide theme, app icon or notification artwork.
    extension.ToggleHistory()
    extension.SetDnd(dbus.Boolean(False))
    notify.CloseNotification(dbus.UInt32(ident))
    colors = {'a': (231, 17, 123), 'b': (17, 231, 123), 'new': (231, 123, 17), 'raw': (17, 123, 231)}
    for pack in ('a', 'b'):
        directory = root/'data/icons'/('NotifyQA-'+pack)
        (directory/'36x36/apps').mkdir(parents=True)
        (directory/'index.theme').write_text('[Icon Theme]\nName=NotifyQA-'+pack+'\nDirectories=36x36/apps\n\n[36x36/apps]\nSize=36\nType=Fixed\nContext=Applications\n')
        Image.new('RGB', (36, 36), colors[pack]).save(directory/'36x36/apps/notify-qa.png')
        if pack == 'b':
            Image.new('RGB', (36, 36), colors['new']).save(directory/'36x36/apps/notify-missing-qa.png')
    def icon_selection(pack):
        tmp=settings.with_suffix('.new')
        tmp.write_text(json.dumps({'theme': {'name':'nord','mode':'dark'}, 'icons': {'theme':'NotifyQA-'+pack,'fallback':'none'}}))
        os.replace(tmp, settings)
    def send_icon(app, icon, hints=None):
        return int(notify.Notify(app, dbus.UInt32(0), icon, 'Icon refresh', 'Synthetic icon test', dbus.Array([],signature='s'), dbus.Dictionary(hints or {},signature='sv'), dbus.Int32(90000)))
    def active():
        return {item['id']:item for item in json.loads((root/'banner-debug.json').read_text())['active']}
    def icon_color(window, wanted, label):
        return sum(count for count, pixel in capture(window,label).getcolors(1 << 24) if pixel == wanted) > 300
    icon_selection('a')
    # Wait for the watcher to load A before populating its positive/negative cache.
    wait(lambda:json.loads(run(bins/'decthemectl','icons','status','--json'))['active']=='NotifyQA-a','icon pack A resolves')
    time.sleep(1.0)
    pack_id=send_icon('Icon Pack QA', 'notify-qa')
    missing_id=send_icon('Missing Icon QA', 'notify-missing-qa')
    rgba=dbus.ByteArray(bytes(colors['raw']+(255,))*64)
    image_hint=dbus.Struct((dbus.Int32(8),dbus.Int32(8),dbus.Int32(32),dbus.Boolean(True),dbus.Int32(8),dbus.Int32(4),rgba),signature='iiibiiay')
    raw_id=send_icon('Image Hint QA', 'notify-qa', {'image-data':image_hint})
    before=wait(lambda: active() if {pack_id,missing_id,raw_id} <= active().keys() else None,'three icon banners')
    wait(lambda:icon_color(before[pack_id]['window'],colors['a'],'icon-pack-a'),'initial A artwork')
    wait(lambda:icon_color(before[raw_id]['window'],colors['raw'],'icon-raw-a'),'image hint overrides A')
    assert not icon_color(before[missing_id]['window'],colors['new'],'icon-missing-a')
    wait(lambda:{pack_id,missing_id,raw_id} <= {item['id'] for item in json.loads(history.read_text())},'icon notifications persisted')
    saved_icons=history.read_bytes()
    icon_selection('b')
    wait(lambda:icon_color(before[pack_id]['window'],colors['b'],'icon-pack-b'),'same existing banner follows B')
    wait(lambda:icon_color(before[missing_id]['window'],colors['new'],'icon-missing-b'),'same missing banner gains icon')
    wait(lambda:icon_color(before[raw_id]['window'],colors['raw'],'icon-raw-b'),'image hint survives icon refresh')
    after=active()
    for item in (pack_id,missing_id,raw_id):
        assert (after[item]['window'],after[item]['deadline']) == (before[item]['window'],before[item]['deadline'])
    assert history.read_bytes()==saved_icons
    next_id=send_icon('Icon Pack QA', 'notify-qa')
    next_banner=wait(lambda:active().get(next_id),'new banner after icon refresh')
    wait(lambda:icon_color(next_banner['window'],colors['b'],'icon-next-b'),'subsequent matching notification uses B')
    # Opening history deliberately dismisses banners; verify its cache separately
    # rather than pretending banner identities survive that existing UI action.
    wait(lambda:next_id in {item['id'] for item in json.loads(history.read_text())},'subsequent notification persisted')
    saved_icons=history.read_bytes()
    old_panel=panel
    extension.ShowHistory()
    panel=int(wait(lambda:json.loads(debug.read_text()).get('window') if json.loads(debug.read_text()).get('window') != old_panel else None,'fresh icon history opens'))
    wait(lambda:icon_color(panel,colors['b'],'icon-history-b'),'history initially uses B')
    icon_selection('a')
    wait(lambda:icon_color(panel,colors['a'],'icon-history-a'),'existing history follows A')
    assert not icon_color(panel,colors['new'],'icon-history-missing-a')
    icon_selection('b')
    wait(lambda:icon_color(panel,colors['b'],'icon-history-b-again'),'existing history follows B again')
    wait(lambda:icon_color(panel,colors['new'],'icon-history-new'),'history retries previous missing icon')
    assert history.read_bytes()==saved_icons
    assert daemon.poll() is None
    print('PASS icon A→B refreshes retained banners/history, missing lookup, subsequent notifications; image hint/IDs/deadlines/history preserved',flush=True)
    (evidence/'result.json').write_text(json.dumps({'daemon_sha256':hashlib.sha256((bins/'decnotify').read_bytes()).hexdigest(),'icon_pack_refresh':True,'negative_cache_refresh':True,'supplied_image_preserved':True,'banner_identity_deadline_preserved':True,'daemon_pid':daemon.pid,'toast_window':banner,'history_window':panel,'notification_id':ident,'history_unchanged':True,'dnd_preserved':True},indent=2)+'\n')
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
