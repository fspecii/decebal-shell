#!/usr/bin/env python3
"""Real search entry: nested case-insensitive results without following links, revealing
file and folder results, Escape and navigation leaving search, and monitoring during
search. Private reserved-display only; search-races-native.py covers cancellation."""
import os,sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from harness import Session
binary=os.environ.get('DECFILES_BIN',str(Path.home()/'.cache/decebal-files-target/debug/decfiles'))
out=os.environ.get('DECFILES_SEARCH_EVIDENCE')
s=Session(binary,out,'decfiles-search-native-')
search=lambda:s.state().get('search',{})
def query(text):
    s.focus(s.window);s.key('ctrl+f');s.key('ctrl+a');s.run(['xdotool','type','--clearmodifiers','--delay','1',text])
    s.wait(lambda:search().get('active') and search().get('query')==text and search().get('done'),'search did not finish for '+text)
    return search()
def activate(path):
    index=search()['results'].index(str(path));s.click(310,s.row_top+50*index);s.key('Return')
try:
    base=s.home/'Documents';(base/'Alpha'/'Beta').mkdir(parents=True);(base/'Reports dir').mkdir()
    (base/'Alpha'/'Beta'/'Report.TXT').write_bytes(b'x');(base/'report-top.md').write_bytes(b'x');(base/'unrelated').write_bytes(b'x')
    raw=os.fsencode(base)+b'/raw-\xff-report';fd=os.open(raw,os.O_CREAT|os.O_WRONLY,0o600);os.close(fd)
    outside=s.root/'outside';outside.mkdir();(outside/'report-outside').write_bytes(b'x');(base/'link-to-outside').symlink_to(outside)
    s.start_x();s.launch(folder=base)
    result=query('REPORT');found=set(result['results'])
    assert found=={str(base/'Alpha'/'Beta'/'Report.TXT'),str(base/'report-top.md'),raw.decode('utf8','replace'),str(base/'Reports dir')},found
    assert 'results for “REPORT”' in s.state()['status'];s.snap('search-results')
    s.cases.append('nested case-insensitive results, non-UTF8 name, symlinked folder not entered')
    activate(base/'Alpha'/'Beta'/'Report.TXT')
    s.wait(lambda:s.at(base/'Alpha'/'Beta') and s.op().get('selected')==str(base/'Alpha'/'Beta'/'Report.TXT'),'file result not revealed and selected')
    assert not search()['active'] and search()['query']==''
    s.cases.append('activating a file result opens its folder with the file selected and leaves search')
    s.address(base);query('reports');activate(base/'Reports dir');s.wait(lambda:s.at(base/'Reports dir'),'folder result not opened')
    s.cases.append('activating a folder result opens it')
    s.address(base);query('report');s.key('Escape')
    s.wait(lambda:not search().get('active') and s.state().get('view_order'),'Escape did not return to the folder view');assert s.at(base)
    s.cases.append('Escape leaves search and restores the folder view')
    query('report');(base/'report-new').write_bytes(b'x');s.wait(lambda:s.state().get('monitor',{}).get('refreshes',0)>0,'monitor refresh did not happen')
    assert search().get('active');s.cases.append('a background refresh does not leave or break the results view')
    s.key('alt+Up');s.wait(lambda:s.at(base.parent) and not search().get('active'),'navigation did not leave search');assert search()['query']==''
    s.cases.append('navigation cancels and leaves search')
    s.address(base);result=query('zzzz-no-match');assert result['count']==0 and '0 results' in s.state()['status']
    s.cases.append('a query without matches reports zero results')
    s.finish({'features':[],'scope':'Private X11 actual GTK search entry and results; no Orca/Wayland acceptance'})
finally:
    s.close()
