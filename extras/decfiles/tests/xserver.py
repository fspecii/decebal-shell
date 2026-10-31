"""Private Xvfb transport for decfiles native suites.

Rules: only explicitly reserved HIGH display numbers (700-899), never auto-probing from
:0 with -displayfd; before any input, prove the server is ours in BOTH socket namespaces
(filesystem /tmp/.X11-unix/XN and Linux abstract @/tmp/.X11-unix/XN) and through its lock
file; never unlink a foreign X socket or lock."""
import fcntl,os,re,subprocess,time
from pathlib import Path

FIRST,LAST=700,899
SCREEN='1400x900'
RESERVATIONS=Path('/var/tmp/decfiles-claude-isolation/displays')
_held=[]  # reservation locks stay held until this process exits

def _taken(n):
    """Any trace of display n, ours or foreign, in either namespace or its lock."""
    if os.path.lexists(f'/tmp/.X{n}-lock') or os.path.lexists(f'/tmp/.X11-unix/X{n}'):return True
    try:return f'@/tmp/.X11-unix/X{n}' in Path('/proc/net/unix').read_text()
    except OSError:return True

def _socket_inodes(name):
    """Inodes of listening unix sockets whose path (or @abstract name) is exactly name."""
    found=set()
    for line in Path('/proc/net/unix').read_text().splitlines()[1:]:
        parts=line.split()
        if len(parts)>=8 and parts[7]==name:found.add(parts[6])
    return found

def _process_socket_inodes(pid):
    inodes=set()
    for fd in Path(f'/proc/{pid}/fd').iterdir():
        try:target=os.readlink(fd)
        except OSError:continue
        match=re.fullmatch(r'socket:\[(\d+)\]',target)
        if match:inodes.add(match.group(1))
    return inodes

def owned_proof(n,process):
    """True only if display n's lock names our pid and both sockets belong to our process."""
    try:
        lock_pid=int(Path(f'/tmp/.X{n}-lock').read_text().strip())
        ours=_process_socket_inodes(process.pid)
    except (OSError,ValueError):return False
    path=_socket_inodes(f'/tmp/.X11-unix/X{n}');abstract=_socket_inodes(f'@/tmp/.X11-unix/X{n}')
    return lock_pid==process.pid and bool(path) and bool(abstract) and path<=ours and abstract<=ours

def start(env,log,owned):
    """Start a private Xvfb on a reserved high display and set env['DISPLAY']. Returns (n, process)."""
    RESERVATIONS.mkdir(parents=True,exist_ok=True)
    start_at=FIRST+os.getpid()%(LAST-FIRST)
    for offset in range(LAST-FIRST+1):
        n=FIRST+(start_at-FIRST+offset)%(LAST-FIRST+1)
        if _taken(n):continue
        # Serialise among our own concurrent fixtures; held for the fixture's lifetime.
        reservation=(RESERVATIONS/f'{n}.lock').open('w')
        try:fcntl.flock(reservation,fcntl.LOCK_EX|fcntl.LOCK_NB)
        except OSError:reservation.close();continue
        if _taken(n):reservation.close();continue
        x=subprocess.Popen(['Xvfb',f':{n}','-screen','0',SCREEN+'x24','-noreset','-nolisten','tcp'],env=env,stdout=log(f'xvfb-{n}'),stderr=subprocess.STDOUT)
        end=time.monotonic()+10
        while time.monotonic()<end and x.poll() is None and not owned_proof(n,x):time.sleep(.05)
        if x.poll() is None and owned_proof(n,x):
            probe=subprocess.run(['xdotool','getdisplaygeometry'],env=dict(env,DISPLAY=f':{n}'),capture_output=True,text=True,timeout=5)
            if probe.returncode==0 and probe.stdout.split()==SCREEN.split('x') and owned_proof(n,x):
                owned.append(x);_held.append(reservation);env['DISPLAY']=f':{n}';assert n>=FIRST
                return n,x
        # Not provably ours (lost a race or failed): stop only our own process; unlink nothing.
        if x.poll() is None:x.terminate()
        try:x.wait(timeout=5)
        except subprocess.TimeoutExpired:x.kill();x.wait()
        reservation.close()
    raise AssertionError('no reserved high private display could be proven ours')
