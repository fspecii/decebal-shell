#!/usr/bin/env python3
"""DRAFT (not yet run), fixture-delay binary only: DECFILES_FIXTURE_OPEN_DELAY_MS holds every
open request in its probe stage (capped at 10 s) so stale-request guards are observable.
Proves that a changed or vanished selection, navigation and window close each supersede a
pending open, and that no stale completion launches anything. Private Xvfb/bus/HOME/XDG and a
fake registered opener only (tests/openfix.py)."""
import json,os,sys,time
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from harness import Session
from openfix import Registry
DELAY_MS=2500
binary=os.environ.get('DECFILES_FIXTURE_BIN','/var/tmp/decfiles-operations-artifacts/fixture-delay/decfiles')
s=Session(binary,os.environ.get('DECFILES_OPEN_RACES_EVIDENCE'),'decfiles-open-races-',{'DECFILES_FIXTURE_OPEN_DELAY_MS':str(DELAY_MS)})
opened=lambda:s.state().get('open',{})
last=lambda:opened().get('last') or {}
def touch(path):
    fd=os.open(path,os.O_CREAT|os.O_WRONLY,0o644);os.write(fd,b'x');os.close(fd)
def pending(path=None):
    p=opened().get('pending');return p if p and (path is None or p['path']==str(path)) else None
def outlive_delay():time.sleep(DELAY_MS/1000+1.0)
try:
    s.start_x();reg=Registry(s)
    base=s.home/'Documents';base.mkdir()
    first,second,third=base/'first.dfopen',base/'second.dfopen',base/'third.dfopen'
    for p in (first,second,third):touch(p)
    s.launch(folder=base)

    # 1. The selection moves to another file while the first request is held: nothing is launched for either.
    s.select_item(first);s.key('Return');s.wait(lambda:pending(first),'open request did not become pending')
    s.select_item(second);s.wait(lambda:pending() is None and last().get('status')=='cancelled','changed selection did not supersede the request')
    assert last()['path']==str(first);outlive_delay();assert not reg.records(),reg.records()
    s.cases.append('changing the selection supersedes a pending open; the late probe launches nothing')
    # The newer, explicit request for the now-selected file is the only one that launches.
    s.key('Return');s.wait(lambda:pending(second),'second request missing');s.wait(lambda:len(reg.records())==1,'second file did not open',DELAY_MS/1000+10)
    assert reg.argv(reg.records()[0])==[os.fsencode(second)],reg.records();assert last()['status']=='launched'
    s.cases.append('only the newest request for the exact selected file launches')

    # 2. The target is renamed on disk while the request is held: the monitor refresh clears the selection.
    s.select_item(third);s.key('Return');s.wait(lambda:pending(third),'third request missing')
    renamed=base/'renamed.dfopen';os.rename(third,renamed)
    s.wait(lambda:pending() is None and last().get('status')=='cancelled','vanished target did not cancel the request',DELAY_MS/1000+10)
    outlive_delay();assert len(reg.records())==1,reg.records();assert not os.path.exists(third)
    s.cases.append('a target renamed during the request is never launched, under neither name')

    # 3. Navigation supersedes a pending request.
    s.select_item(first);s.key('Return');s.wait(lambda:pending(first),'request before navigation missing')
    s.key('alt+Up');s.wait(lambda:s.at(base.parent) and pending() is None,'navigation did not cancel the request')
    outlive_delay();assert len(reg.records())==1,reg.records()
    s.cases.append('navigation supersedes a pending open')

    # 4. Closing cancels the pending request: normal exit, nothing launched afterwards.
    s.address(base);s.select_item(first);s.key('Return');s.wait(lambda:pending(first),'request before close missing')
    (s.out/'state-final.json').write_text(json.dumps(s.state(),indent=2)+'\n')
    s.focus(s.window);s.key('ctrl+w');s.app.wait(timeout=5);assert s.app.returncode==0
    outlive_delay();assert len(reg.records())==1,reg.records()
    s.cases.append('closing during a pending open exits normally and launches nothing')
    reg.unchanged()
    s.result({'features':['fixture-delay'],'scope':'Private X11 actual GTK controls; fixture-delay binary only (do not distribute); fake registered opener; no Wayland or Orca acceptance'})
finally:
    s.close()
