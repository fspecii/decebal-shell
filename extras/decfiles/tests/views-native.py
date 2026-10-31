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
    s.finish({'features':[],'scope':'Private X11 actual GTK header controls; no Orca/Wayland acceptance'})
finally:
    s.close()
