#!/usr/bin/env python3
"""Fixture-delay binary only: a capped per-folder search delay (DECFILES_FIXTURE_SEARCH_DELAY_MS)
makes a newer query and navigation cancel a slow search observably, with no stale results.
Private reserved-display Xvfb/bus/HOME/XDG only."""
import os,sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from harness import Session
fixture=os.environ.get('DECFILES_FIXTURE_BIN','/var/tmp/decfiles-transfer-artifacts/fixture-delay/decfiles')
out=os.environ.get('DECFILES_SEARCH_RACES_EVIDENCE')
t=Session(fixture,out,'decfiles-search-races-',{'DECFILES_FIXTURE_SEARCH_DELAY_MS':'250'})
search=lambda:t.state().get('search',{})
try:
    base=t.home/'Documents'
    for index in range(12):
        folder=base/f'd{index:02}';folder.mkdir(parents=True);(folder/f'aa-{index}').write_bytes(b'');(folder/f'bb-{index}').write_bytes(b'')
    t.start_x();t.launch(folder=base)
    t.focus(t.window);t.key('ctrl+f');t.run(['xdotool','type','--clearmodifiers','--delay','1','aa'])
    t.wait(lambda:search().get('active') and search().get('query')=='aa' and search().get('count',0)>0 and not search().get('done'),'slow search did not stream partial results')
    first=search()['id'];t.key('ctrl+a');t.run(['xdotool','type','--clearmodifiers','--delay','1','bb'])
    t.wait(lambda:search().get('query')=='bb' and search().get('done'),'newer query did not finish',20)
    assert search()['id']>first and search()['count']==12 and all('/bb-' in path for path in search()['results']),search()
    t.cases.append('a newer query cancels the slow search; only its own results are shown')
    t.key('ctrl+a');t.run(['xdotool','type','--clearmodifiers','--delay','1','aa'])
    t.wait(lambda:search().get('query')=='aa' and search().get('count',0)>0 and not search().get('done'),'second slow search not running')
    t.key('alt+Up');t.wait(lambda:t.at(base.parent) and not search().get('active'),'navigation did not cancel the slow search')
    t.cases.append('navigation cancels a slow search mid-walk')
    t.finish({'features':['fixture-delay'],'fixture_search_delay_ms':250,'scope':'Controlled fixture delay only'})
finally:
    t.close()
