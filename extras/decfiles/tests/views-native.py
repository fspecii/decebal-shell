#!/usr/bin/env python3
"""Real header sorting, sort persistence across views/navigation and selection during sorting.
Private Xvfb/bus/HOME/XDG only. DECFILES_BIN and DECFILES_VIEWS_EVIDENCE choose binary/evidence."""
import os,sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from harness import Session
s=Session(os.environ.get('DECFILES_BIN',str(Path.home()/'.cache/decebal-files-target/debug/decfiles')),os.environ.get('DECFILES_VIEWS_EVIDENCE'),'decfiles-views-native-')
try:
    base=s.home/'Documents';base.mkdir()
    (base/'zeta dir').mkdir();(base/'Alpha dir').mkdir()
    files={'b notes.txt':(300,1_600_000_000),'a picture.png':(50,1_700_000_000),'c readme.md':(10,1_500_000_000)}
    for name,(size,mtime) in files.items():
        path=base/name;path.write_bytes(b'x'*size);os.utime(path,(mtime,mtime))
    folders=[str(base/'Alpha dir'),str(base/'zeta dir')]
    order=lambda names:folders+[str(base/name) for name in names]
    s.start_x();s.launch(folder=base)
    view=lambda:s.state().get('view_order')
    sort=lambda:s.state().get('sort')
    assert view()==order(['a picture.png','b notes.txt','c readme.md']) and sort()=={'column':'name','descending':False},s.state().get('view_order')
    types={e['name']:e.get('content_type') for e in s.state()['model']['columns'][-1]['entries']}
    assert types['b notes.txt']=='text/plain' and types['a picture.png']=='image/png',types
    s.snap('details-name');s.cases.append('details default Name order keeps folders first with GIO content types')
    s.select_item(base/'b notes.txt')
    s.click(865,66);s.wait(lambda:sort()=={'column':'size','descending':False},'Size header did not sort')
    assert view()==order(['c readme.md','a picture.png','b notes.txt']),view();assert s.op()['selected']==str(base/'b notes.txt')
    s.cases.append('Size header sorts ascending and keeps the selected item')
    s.click(865,66);s.wait(lambda:sort()=={'column':'size','descending':True},'second Size click did not reverse')
    assert set(view()[:2])==set(folders) and view()[2:]==[str(base/n) for n in ['b notes.txt','a picture.png','c readme.md']],view()
    s.snap('details-size-descending');s.cases.append('second Size click sorts descending with folders still first')
    s.click(977,66);s.wait(lambda:sort()=={'column':'modified','descending':False},'Date modified header did not sort')
    assert view()[2:]==[str(base/n) for n in ['c readme.md','b notes.txt','a picture.png']],view()
    s.cases.append('Date modified header sorts by modification time')
    s.layout(True);s.layout(False);assert sort()=={'column':'modified','descending':False} and view()[2:]==[str(base/n) for n in ['c readme.md','b notes.txt','a picture.png']]
    s.cases.append('sort choice survives switching to columns and back')
    s.address(base/'Alpha dir');s.key('alt+Left');s.wait(lambda:s.at(base),'Back failed');assert sort()=={'column':'modified','descending':False}
    s.cases.append('sort choice survives navigation and history')
    # Folder monitoring: external changes refresh the current folder without user action.
    import time
    mon=lambda:s.state().get('monitor',{})
    s.wait(lambda:mon().get('watching')==str(base),'current folder not watched')
    s.select_item(base/'b notes.txt');before=mon()['refreshes']
    (base/'d external.txt').write_bytes(b'external')
    s.wait(lambda:str(base/'d external.txt') in (s.state().get('view_order') or []),'external create not shown')
    assert s.op()['selected']==str(base/'b notes.txt') and mon()['refreshes']>before
    s.cases.append('external create appears without user action and keeps selection')
    s.key('Down');s.wait(lambda:s.op().get('selected') not in (None,str(base/'b notes.txt')),'keyboard focus not kept in the list after refresh')
    s.cases.append('keyboard focus stays in the file list across a background refresh')
    unchanged=mon()['unchanged'];os.utime(base/'c readme.md',(1_500_000_000,1_500_000_000))
    s.wait(lambda:mon()['unchanged']>unchanged,'attribute-only change was not recognised as unchanged')
    s.cases.append('attribute-only change refreshes without re-rendering unchanged entries')
    s.select_item(base/'d external.txt');(base/'d external.txt').rename(base/'e renamed.txt')
    s.wait(lambda:str(base/'e renamed.txt') in s.state()['view_order'] and str(base/'d external.txt') not in s.state()['view_order'],'external rename not shown')
    s.wait(lambda:s.op().get('selected') is None,'vanished selection kept')
    (base/'e renamed.txt').unlink();s.wait(lambda:str(base/'e renamed.txt') not in s.state()['view_order'],'external delete not shown')
    s.cases.append('external rename and delete update the view and clear a vanished selection')
    s.focus(s.window);s.key('ctrl+shift+n');s.wait(lambda:s.op().get('dialog')=='Create folder','create dialog missing')
    count=mon()['refreshes'];(base/'f during dialog.txt').write_bytes(b'x');time.sleep(1.2)
    assert mon()['refreshes']==count and str(base/'f during dialog.txt') not in s.state()['view_order']
    s.key('Escape');s.wait(lambda:str(base/'f during dialog.txt') in (s.state().get('view_order') or []),'deferred refresh did not run after dialog')
    s.cases.append('refresh waits while a dialog is open, then runs')
    s.address(base/'Alpha dir');s.wait(lambda:mon().get('watching')==str(base/'Alpha dir'),'watch did not follow navigation')
    count=mon()['refreshes'];(base/'g old folder.txt').write_bytes(b'x');time.sleep(1.2);assert mon()['refreshes']==count
    (base/'Alpha dir'/'inside.txt').write_bytes(b'x');s.wait(lambda:str(base/'Alpha dir'/'inside.txt') in (s.state().get('view_order') or []),'new watched folder change not shown')
    s.cases.append('watch follows navigation; the previous folder no longer refreshes')
    s.finish({'features':[],'scope':'Private X11 actual GTK header controls and inotify on private fixtures; no Orca/Wayland acceptance'})
finally:
    s.close()
