#!/usr/bin/env python3
# Native Files operations; invoked only inside the shell harness private session.
import os,pathlib,subprocess,time,signal
import gi
gi.require_version('Gtk','4.0')
from gi.repository import Gtk
provider=Gtk.CssProvider();errors=[]
provider.connect('parsing-error',lambda _,section,error:errors.append(str(error)))
provider.load_from_path(os.environ['XDG_CONFIG_HOME']+'/gtk-4.0/gtk.css')
assert not errors,errors
home=pathlib.Path.home();root=home/'Example folder';profile=os.environ['PROFILE']
def run(*a):return subprocess.check_output(a,text=True).strip()
def wait(fn,why):
 for _ in range(100):
  try:
   result=fn()
   if result:return result
  except (subprocess.CalledProcessError,FileNotFoundError,IndexError):pass
  time.sleep(.1)
 raise AssertionError(why)
def interrupted(signum, frame): raise KeyboardInterrupt
signal.signal(signal.SIGTERM, interrupted)
p=subprocess.Popen(['nautilus','--new-window',str(root)])
try:
 w=wait(lambda:run('xdotool','search','--onlyvisible','--class','org.gnome.Nautilus').splitlines()[0],'Files window not visible')
 run('xdotool','windowfocus',w);time.sleep(1.5)
 def key(*keys):run('xdotool','key','--clearmodifiers',*keys);time.sleep(.35)
 def type(text):run('xdotool','type','--clearmodifiers',str(text))
 def title(text):wait(lambda:text in run('xdotool','getwindowname',w),'navigation title '+text)
 def shot(mode):
  time.sleep(.6);subprocess.run(['import','-window',w,str(pathlib.Path(os.environ['FILE_MANAGER_EVIDENCE_DIR']) / f'{profile}-{mode}.png')],check=True,timeout=5)
 key('ctrl+1');time.sleep(1);shot('list')
 key('ctrl+l');type(root/'Child folder');key('Return');title('Child folder')
 key('alt+Left');title('Example folder')
 key('alt+Right');title('Child folder')
 key('alt+Up');title('Example folder')
 # New-folder dialog and file-system proof, entirely inside private HOME.
 key('ctrl+shift+n');type('Created '+profile);key('Return')
 wait(lambda:(root/('Created '+profile)).is_dir(),'native folder creation failed')
 key('ctrl+2');time.sleep(1);shot('grid')
 print('PASS',profile,'native CSS parse, list/grid, Ctrl+L, back/forward/parent, create folder')
finally:
 p.terminate()
 try:p.wait(timeout=5)
 except subprocess.TimeoutExpired:p.kill();p.wait()
