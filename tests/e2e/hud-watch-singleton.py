#!/usr/bin/env python3
"""Private fake-rdwm/Xvfb proof of the workspace HUD watcher's lifecycle.
HUD_BIN selects a binary; EXPECT_DUPLICATES=1 demonstrates the old regression.
Never reads live clipboard, audio, display, configuration or IPC.
"""
from pathlib import Path
import fcntl, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, threading, time

repo = Path(__file__).resolve().parents[2]
binary = Path(os.environ.get('HUD_BIN', repo / 'target/debug/dechud')).resolve()
root = Path(tempfile.mkdtemp(prefix='decebal-hud-watch-', dir='/var/tmp'))
root.chmod(0o700)
evidence = Path(os.environ.get('HUD_WATCH_EVIDENCE', root / 'evidence'))
evidence.mkdir(parents=True, exist_ok=True)
env = {k:v for k,v in os.environ.items() if not k.startswith(('DECEBAL_', 'DECHUD_', 'MAC', 'DECSETTINGS_'))}
for key in ('DISPLAY', 'WAYLAND_DISPLAY', 'XAUTHORITY', 'RDWM_SOCK', 'SESSION_MANAGER', 'AT_SPI_BUS_ADDRESS'):
    env.pop(key, None)
for key, child in {'HOME':'home', 'XDG_CONFIG_HOME':'cfg', 'XDG_CONFIG_DIRS':'nocfg', 'XDG_DATA_HOME':'data', 'XDG_DATA_DIRS':'nodata', 'XDG_CACHE_HOME':'cache', 'XDG_STATE_HOME':'state', 'XDG_RUNTIME_DIR':'run', 'DECEBAL_CONFIG':'shell', 'DECEBAL_THEMES':'themes', 'DECEBAL_SYSTEM_THEMES':'system-themes'}.items():
    env[key] = str(root / child)
    Path(env[key]).mkdir()
os.chmod(env['XDG_RUNTIME_DIR'], 0o700)
env.update(DBUS_SESSION_BUS_ADDRESS='unix:path='+str(root/'none-bus'), DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(root/'none-system-bus'), DECEBAL_BACKEND='x11', DECEBAL_SYSTEM_AUTOMATIC='0', TMPDIR='/var/tmp')
number = next(n for n in range(550,580) if not Path(f'/tmp/.X11-unix/X{n}').exists() and not Path(f'/tmp/.X{n}-lock').exists())
env['DISPLAY'] = f':{number}'
owned = []
logs = []
servers = []

def start(args, label):
    log = (root / (label+'.log')).open('w')
    logs.append(log)
    p = subprocess.Popen(list(map(str,args)), env=env, stdout=log, stderr=log, start_new_session=True)
    owned.append(p)
    return p

def wait(fn, label, seconds=7):
    end = time.monotonic()+seconds
    while time.monotonic()<end:
        result = fn()
        if result:
            return result
        time.sleep(.03)
    raise AssertionError(label)

class FakeWm:
    def __init__(self, view):
        self.path = root/'run/rdwm.sock'
        self.path.unlink(missing_ok=True)
        self.socket = socket.socket(socket.AF_UNIX)
        self.socket.bind(str(self.path))
        self.socket.listen()
        self.socket.settimeout(.15)
        self.view = view
        self.gets = 0
        self.subscriptions = 0
        self.subscribers = []
        self.connections = []
        self.lock = threading.Lock()
        self.stop = threading.Event()
        self.handlers = []
        self.thread = threading.Thread(target=self.accept, daemon=True)
        self.thread.start()
        servers.append(self)
    def accept(self):
        while not self.stop.is_set():
            try: conn,_ = self.socket.accept()
            except socket.timeout: continue
            except OSError: break
            self.connections.append(conn)
            t = threading.Thread(target=self.handle,args=(conn,),daemon=True)
            self.handlers.append(t)
            t.start()
    def handle(self, conn):
        try:
            with conn.makefile('rb') as stream:
                for line in stream:
                    req = json.loads(line)
                    if req.get('cmd') == 'get_state':
                        with self.lock:
                            self.gets += 1
                            response = {'ok':True,'state':{'selected_mon':0,'monitors':[{'id':0,'view':self.view}]}}
                        conn.sendall((json.dumps(response)+'\n').encode())
                    elif req.get('cmd') == 'subscribe':
                        assert req['events'] == ['tag_view']
                        with self.lock:
                            self.subscriptions += 1
                            self.subscribers.append(conn)
                        conn.sendall(b'{"ok":true}\n')
        except (OSError, ValueError): pass
        finally:
            with self.lock:
                if conn in self.subscribers: self.subscribers.remove(conn)
            conn.close()
    def change(self, view):
        self.view = view
        with self.lock:
            clients = list(self.subscribers)
        for conn in clients:
            conn.sendall(b'{"event":"tag_view"}\n')
    def close(self):
        if self.stop.is_set(): return
        self.stop.set()
        self.socket.close()
        for conn in self.connections:
            try: conn.shutdown(socket.SHUT_RDWR)
            except OSError: pass
        self.thread.join(timeout=2)
        for t in self.handlers: t.join(timeout=2)
        self.path.unlink(missing_ok=True)

def windows():
    p = subprocess.run(['xdotool','search','--onlyvisible','--class','^dechud$'],env=env,text=True,capture_output=True,timeout=3)
    return p.stdout.split()

try:
    # Missing WM must not make the singleton guard transient. Launch a race of
    # watchers first, then connect the fake WM once one stable holder remains.
    watchers = [start([binary,'watch'], 'watch-'+str(i)) for i in range(16)]
    old = os.environ.get('EXPECT_DUPLICATES') == '1'
    if not old:
        wait(lambda:sum(p.poll() is None for p in watchers)==1,'duplicate watchers must exit')
    else:
        time.sleep(.6)
    alive = [p for p in watchers if p.poll() is None]
    if old:
        assert len(alive)>1, 'baseline no longer duplicates watchers'
        result = {'passed':True,'expected_baseline_failure':True,'watchers_before_wm':len(alive),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'private_root':str(root)}
    else:
        assert len(alive)==1, ('one watcher while WM is absent',len(alive))
        winner = alive[0]
        assert all(p.wait(timeout=3)==0 for p in watchers if p is not winner)
        lock = root/'run/dechud-watch.lock'
        assert int(lock.read_text())==winner.pid
        # The watch guard does not take or steal the temporary card lock.
        assert not (root/'run/dechud.lock').exists()
        wm = FakeWm(1)
        wait(lambda:wm.subscriptions==1,'one watcher must subscribe after reconnect')
        extra = [start([binary,'watch'],'extra-'+str(i)) for i in range(8)]
        assert all(p.wait(timeout=3)==0 for p in extra)
        time.sleep(.15)
        assert wm.subscriptions==1 and wm.gets==1
        xvfb = start(['Xvfb', env['DISPLAY'],'-screen','0','800x600x24','-nolisten','tcp'],'xvfb')
        wait(lambda:Path(f'/tmp/.X11-unix/X{number}').exists(),'private Xvfb')
        wm.change(2)
        card = wait(windows,'one workspace HUD card must map')
        assert len(card)==1
        wait(lambda:wm.gets>=3,'one watcher refresh plus one emitted card probe')
        assert wm.gets==3, ('duplicate emission requests',wm.gets)
        card_pid = int((root/'run/dechud.lock').read_text())
        wm.change(4)
        wait(lambda:wm.gets>=5,'second workspace refresh')
        assert wm.gets==5 and windows()==card and int((root/'run/dechud.lock').read_text())==card_pid, 'card updates reuse their independent singleton'
        wait(lambda:not windows(),'card expires normally')
        wm.close()
        wm2 = FakeWm(4)
        wait(lambda:wm2.subscriptions==1,'watcher resubscribes after WM restart')
        assert winner.poll() is None
        winner.terminate()
        winner.wait(timeout=3)
        wait(lambda:not wm2.subscribers,'old subscription closes')
        replacement = start([binary,'watch'],'replacement')
        wait(lambda:wm2.subscriptions==2,'guard released after exit')
        assert int(lock.read_text())==replacement.pid
        wm2.change(8)
        assert len(wait(windows,'replacement emits a card'))==1
        wait(lambda:wm2.gets>=3,'replacement requests')
        assert len(wm2.subscribers)==1
        wait(lambda:not windows(),'replacement card expires and is reaped')
        result = {'passed':True,'watchers_before_wm':1,'duplicate_invocations_exit_zero':23,'initial_subscriptions':wm.subscriptions,'workspace_event_requests':wm.gets,'card_pid_reused':True,'reconnected_subscriptions':wm2.subscriptions,'replacement_lock_holder':replacement.pid,'normal_card_updates':True,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'private_root':str(root)}
    (evidence/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result))
finally:
    for server in servers: server.close()
    for p in owned:
        if p.poll() is None: p.terminate()
    for p in owned:
        try: p.wait(timeout=3)
        except subprocess.TimeoutExpired: p.kill();p.wait(timeout=3)
    # Workspace cards are children of owned watchers. Match their private runtime
    # identity before terminating, never just their name or PID number.
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit(): continue
        try:
            if proc.stat().st_uid!=os.getuid() or proc.joinpath('comm').read_text().strip()!='dechud': continue
            entries=proc.joinpath('environ').read_bytes().split(b'\0')
            if ('XDG_RUNTIME_DIR='+env['XDG_RUNTIME_DIR']).encode() in entries:
                fd=os.pidfd_open(int(proc.name))
                try: signal.pidfd_send_signal(fd,signal.SIGTERM)
                finally:os.close(fd)
        except (FileNotFoundError,PermissionError,ProcessLookupError):pass
    for log in logs:log.close()
