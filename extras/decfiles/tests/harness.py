"""Shared private-session harness for decfiles native suites: owned Xvfb, private bus,
HOME/XDG directories and fixture trees. Never touches the live display, bus or user files."""
from pathlib import Path
import hashlib,json,os,subprocess,sys,tempfile,time
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
import xserver

class Session:
    def __init__(self,binary,out,prefix,extra_env=None):
        self.binary=Path(binary).resolve()
        self.root=Path(tempfile.mkdtemp(prefix=prefix,dir='/var/tmp'));self.root.chmod(0o700)
        self.out=Path(out or self.root/'evidence');self.out.mkdir(parents=True,exist_ok=True);(self.out/'result.json').unlink(missing_ok=True)
        blocked=('DISPLAY','WAYLAND_DISPLAY','XAUTHORITY','DBUS_SESSION_BUS_ADDRESS','DBUS_SYSTEM_BUS_ADDRESS','AT_SPI_BUS_ADDRESS','RDWM_SOCK','SESSION_MANAGER')
        self.env={k:v for k,v in os.environ.items() if k not in blocked and not k.startswith(('DECEBAL_','DECFILES_','GTK_','GDK_','GSK_'))}
        for key,part in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_STATE_HOME','state'),('XDG_DATA_HOME','data'),('XDG_RUNTIME_DIR','run')]:
            self.env[key]=str(self.root/part);Path(self.env[key]).mkdir();Path(self.env[key]).chmod(0o700)
        self.env.update(GDK_BACKEND='x11',GSK_RENDERER='cairo',GTK_A11Y='none',NO_AT_BRIDGE='1',GSETTINGS_BACKEND='memory',GIO_USE_VFS='local',
            DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(self.root/'no-system'),DECFILES_EVIDENCE_STATE=str(self.root/'state.json'))
        self.env.update(extra_env or {})
        self.row_top=103  # first details row; the tab bar adds 40 px when shown
        self.home=Path(self.env['HOME']);self.owned=[];self.logs=[];self.app=None;self.window=None;self.cases=[]

    def log(self,name):
        f=(self.out/(name+'.log')).open('w');self.logs.append(f);return f

    def start_x(self):
        # Reserved high display, both socket namespaces proven ours before any input.
        self.display=xserver.start(self.env,self.log,self.owned)[0]
        bus=subprocess.Popen(['dbus-daemon','--session','--nofork','--print-address=1'],env=self.env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True);self.owned.append(bus)
        self.env['DBUS_SESSION_BUS_ADDRESS']=bus.stdout.readline().strip();assert self.env['DBUS_SESSION_BUS_ADDRESS'].startswith('unix:')

    def launch(self,*args,folder):
        self.app=subprocess.Popen([str(self.binary),*args,str(folder)],env=self.env,stdout=self.log('app'),stderr=subprocess.STDOUT);self.owned.append(self.app)
        self.wait(lambda:self.at(folder),'initial folder missing',20)
        self.window=self.wait(lambda:self.find('^Decebal Files'),'main window missing')[0];self.focus(self.window)

    def run(self,args,check=True):return subprocess.run(args,env=self.env,check=check,capture_output=True,timeout=8)
    def wait(self,fn,message,seconds=10):
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            value=fn()
            if value:return value
            if self.app and self.app.poll() is not None:raise AssertionError('app exited '+str(self.app.returncode))
            time.sleep(.02)
        raise AssertionError(message)
    def state(self):
        try:return json.loads((self.root/'state.json').read_text())
        except (FileNotFoundError,json.JSONDecodeError):return {}
    def op(self):return self.state().get('operation',{})
    def at(self,path):
        s=self.state();m=s.get('model',{});columns=m.get('columns',[])
        return s if not m.get('pending') and columns and columns[-1]['path']==str(path) else None
    def find(self,name):
        q=subprocess.run(['xdotool','search','--all','--onlyvisible','--pid',str(self.app.pid),'--name',name],env=self.env,capture_output=True,text=True,timeout=3)
        return q.stdout.strip().splitlines() if q.returncode==0 else None
    def key(self,*keys):self.run(['xdotool','key','--clearmodifiers',*keys])
    def focus(self,win):self.run(['xdotool','windowfocus','--sync',win])
    def click(self,x,y,win=None):
        time.sleep(.2);self.run(['xdotool','mousemove','--window',win or self.window,str(x),str(y),'click','1'])
    def geometry(self,win=None):
        text=self.run(['xdotool','getwindowgeometry','--shell',win or self.window]).stdout.decode()
        return {k:int(v) for k,v in (line.split('=',1) for line in text.splitlines()) if k in ('X','Y','WIDTH','HEIGHT')}
    def snap(self,name,win=None):
        time.sleep(.25);self.run(['import','-window',win or self.window,str(self.out/(name+'.png'))])
    def address(self,path):
        self.focus(self.window);self.key('ctrl+l');self.run(['xdotool','type','--clearmodifiers','--delay','1',str(path)]);self.key('Return');self.wait(lambda:self.at(path),'address did not load '+str(path))
    def select_item(self,path):
        """Click the row by its visible position in the sorted details view."""
        self.focus(self.window);wanted=str(path);index=self.state()['view_order'].index(wanted)
        self.click(310,self.row_top+50*index);self.wait(lambda:self.op().get('selected')==wanted,'selection did not match '+wanted)
    def layout(self,columns):
        self.focus(self.window);self.click(980,26);self.key('End' if columns else 'Home');self.key('Return')
        self.wait(lambda:self.state().get('layout')==('columns' if columns else 'details'),'layout selector did not switch')

    def clipboard(self,target):
        """Read the private X server's clipboard; None when no owner offers that target."""
        p=self.run(['xclip','-selection','clipboard','-o','-t',target],check=False);return p.stdout if p.returncode==0 else None
    def offer(self,target,text):
        """Become the private X clipboard owner (an owned, terminated-at-exit xclip)."""
        p=subprocess.Popen(['xclip','-quiet','-selection','clipboard','-t',target,'-i'],env=self.env,stdin=subprocess.PIPE,stdout=self.log('xclip'),stderr=subprocess.STDOUT);self.owned.append(p)
        p.stdin.write(text.encode());p.stdin.close();self.wait(lambda:self.clipboard(target)==text.encode(),'external clipboard owner not ready');return p
    def last_id(self):return (self.op().get('last_result') or {}).get('id',0)
    def finished(self,previous,status,seconds=20):
        def ready():
            s=self.state();o=s.get('operation',{});r=o.get('last_result') or {}
            return r if not o.get('busy') and not o.get('dialog') and not s.get('model',{}).get('pending') and r.get('id',0)>previous else None
        result=self.wait(ready,'operation did not finish',seconds);assert result['status']==status,result;return result
    def button(self,label):
        self.focus(self.window);self.click({'Cut':413,'Copy':484,'Paste':562}[label],self.geometry()['HEIGHT']-52)

    def finish(self,extra):
        (self.out/'state-final.json').write_text(json.dumps(self.state(),indent=2)+'\n')
        self.focus(self.window);self.key('ctrl+w');self.app.wait(timeout=8);assert self.app.returncode==0
        source_root=Path(__file__).resolve().parents[1]
        sources={str(p.relative_to(source_root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source_root.rglob('*')) if p.is_file() and 'target' not in p.parts and (p.suffix=='.rs' or p.name in ('Cargo.toml','Cargo.lock'))}
        result={'passed':True,'cases':self.cases,'display':self.env.get('DISPLAY'),'binary':str(self.binary),'sha256':hashlib.sha256(self.binary.read_bytes()).hexdigest(),'source_hashes':sources,'private_root':str(self.root),**extra}
        (self.out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))

    def close(self):
        for p in reversed(self.owned):
            if p.poll() is None:p.terminate()
            try:p.wait(timeout=5)
            except subprocess.TimeoutExpired:p.kill();p.wait()
        for f in self.logs:f.close()
