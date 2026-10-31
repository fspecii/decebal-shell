#!/usr/bin/env python3
"""Real tab controls: per-tab history and explicit layout, keyboard switching, clipboard
across tabs, monitoring of the active tab, refresh on switch, and closing tabs.
Private reserved-display Xvfb/bus/HOME/XDG only. DECFILES_BIN / DECFILES_TABS_EVIDENCE."""
import os,sys,time
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from harness import Session
s=Session(os.environ.get('DECFILES_BIN',str(Path.home()/'.cache/decebal-files-target/debug/decfiles')),os.environ.get('DECFILES_TABS_EVIDENCE'),'decfiles-tabs-native-')
try:
    base=s.home/'Documents';base.mkdir();a=base/'A';a.mkdir();(base/'note.txt').write_bytes(b'note across tabs')
    s.start_x();s.launch(folder=base)
    tabs=lambda:s.state().get('tabs',{})
    assert tabs()['paths']==[str(base)]
    s.key('ctrl+t');s.wait(lambda:tabs().get('paths')==[str(base),str(base)] and tabs()['active']==1 and s.at(base),'Ctrl+T did not open the current folder in a new tab')
    s.row_top=143;s.snap('two-tabs');s.cases.append('Ctrl+T opens the current folder in a new tab and shows the tab bar')
    s.address(a);s.layout(True)
    assert tabs()['paths']==[str(base),str(a)] and tabs()['layouts']==['details','columns']
    s.key('ctrl+shift+Tab');s.wait(lambda:tabs().get('active')==0 and s.at(base),'Ctrl+Shift+Tab did not switch')
    assert s.state()['layout']=='details' and s.state()['model']['history']==[str(base)],s.state()['model']['history']
    s.key('ctrl+Tab');s.wait(lambda:tabs().get('active')==1 and s.at(a),'Ctrl+Tab did not switch')
    assert s.state()['layout']=='columns' and s.state()['model']['history']==[str(base),str(a)]
    s.key('alt+Left');s.wait(lambda:s.at(base),'Back within the tab failed');s.key('alt+Right');s.wait(lambda:s.at(a),'Forward within the tab failed')
    assert tabs()['paths'][0]==str(base)
    s.cases.append('each tab keeps its own history and explicit layout across keyboard switching')
    s.click(100,71);s.wait(lambda:tabs().get('active')==0 and s.at(base),'clicking the first tab did not switch')
    s.select_item(base/'note.txt');s.key('ctrl+c')
    s.click(319,71);s.wait(lambda:tabs().get('active')==1 and s.at(a),'clicking the second tab did not switch')
    previous=s.last_id();s.button('Paste');s.finished(previous,'success')
    assert (a/'note.txt').read_bytes()==b'note across tabs' and (base/'note.txt').exists()
    s.cases.append('copy in one tab and paste into another through actual tab buttons')
    s.click(100,71);s.wait(lambda:tabs().get('active')==0 and s.at(base),'switch back failed')
    s.wait(lambda:s.state().get('monitor',{}).get('watching')==str(base),'monitor did not follow the active tab')
    (base/'while-active.txt').write_bytes(b'x');(a/'while-inactive.txt').write_bytes(b'x')
    s.wait(lambda:str(base/'while-active.txt') in (s.state().get('view_order') or []),'active tab did not refresh')
    s.click(319,71);s.wait(lambda:tabs().get('active')==1 and any(e['path']==str(a/'while-inactive.txt') for e in s.state()['model']['columns'][-1]['entries']),'inactive tab was not refreshed on switch')
    s.cases.append('monitoring follows the active tab and a switched-to tab is refreshed')
    s.key('ctrl+t');s.wait(lambda:len(tabs().get('paths',[]))==3 and s.at(a),'third tab missing')
    s.key('ctrl+w');s.wait(lambda:len(tabs().get('paths',[]))==2,'Ctrl+W did not close the tab');assert s.app.poll() is None
    s.cases.append('Ctrl+W closes the active tab, not the window, when tabs remain')
    s.click(207,71);s.wait(lambda:len(tabs().get('paths',[]))==1,'close button did not close the first tab')
    assert tabs()['paths']==[str(a)] and s.at(a) and s.state()['layout']=='columns'
    s.row_top=103;s.snap('one-tab-after-close');s.cases.append('tab close button removes that tab; the remaining tab keeps its folder and layout')
    s.finish({'features':[],'scope':'Private X11 actual GTK tab controls; no Orca/Wayland acceptance'})
finally:
    s.close()
