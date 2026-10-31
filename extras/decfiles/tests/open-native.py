#!/usr/bin/env python3
"""DRAFT (not yet run): file opening and Open With through a private registered fake opener.
Private Xvfb/session bus/HOME/XDG and MIME registry only (tests/openfix.py); no live
display, bus, applications or default handlers. Proves exact spaced / metacharacter /
non-UTF8 arguments, no shell, Open With choice and cancel, unchanged defaults, executable and
.desktop refusal, launch failure, missing handler and search Enter = reveal."""
import os,sys,time
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from harness import Session
from openfix import Registry
binary=os.environ.get('DECFILES_BIN',str(Path.home()/'.cache/decebal-files-target/debug/decfiles'))
s=Session(binary,os.environ.get('DECFILES_OPEN_EVIDENCE'),'decfiles-open-native-')
opened=lambda:s.state().get('open',{})
last=lambda:opened().get('last') or {}
def requests():return opened().get('requests',0)
def touch(path,data=b'x',mode=0o644):
    fd=os.open(path,os.O_CREAT|os.O_WRONLY,mode);os.write(fd,data);os.close(fd);os.chmod(path,mode)
def row(path_text):return s.state()['view_order'].index(path_text)
def select(path):s.select_item(path)
def double_click(path):
    s.focus(s.window);index=row(str(path));time.sleep(.3)
    s.run(['xdotool','mousemove','--window',s.window,'310',str(s.row_top+50*index),'click','--repeat','2','--delay','100','1'])
def settle(previous,status,seconds=15):
    """A new request has ended with `status` and nothing is pending."""
    return s.wait(lambda:(opened().get('pending') is None and last().get('id',0)>previous and last().get('status')==status and last()),f'open did not end as {status}: {opened()}',seconds)
def wait_records(count):
    s.wait(lambda:len(reg.records())>=count,f'expected {count} opener records, have {len(reg.records())}')
def only_new(count,opener,path):
    wait_records(count);records=reg.records();assert len(records)==count,records
    assert records[-1]['opener']==opener,records[-1];assert reg.argv(records[-1])==[os.fsencode(path)],(reg.argv(records[-1]),path)
def pwned():
    return [p for base in (Path.cwd(),s.root,s.home,s.home/'Documents',s.home/'Documents'/'args') for p in base.glob('pwned-*')]
try:
    s.start_x();reg=Registry(s)
    docs=s.home/'Documents';args=docs/'args';chooser=docs/'chooser';errors=docs/'errors';execs=docs/'exec'
    for d in (args,chooser,errors,execs):d.mkdir(parents=True)
    spaced=args/'spaced name.dfopen';touch(spaced)
    meta=args/('meta $(touch pwned-sub) ; touch pwned-semi `touch pwned-tick` \'q\' "d" & | * ? %f %U ~ #.dfopen');touch(meta)
    raw=os.fsencode(args)+b'/raw-\xff-\xfe.dfopen';touch(raw)
    raw_row=os.fsencode(args).decode('utf8','replace')+'/raw-�-�.dfopen'
    target,other=chooser/'target.dfopen',chooser/'other.dfopen';touch(target);touch(other)
    fails,nohandler=errors/'fails.dffail',errors/'nohandler.dfnone';touch(fails);touch(nohandler)
    marker=s.root/'EXECUTED';tool=execs/'tool.dfopen';touch(tool,('#!/bin/sh\ntouch %s\n'%marker).encode(),0o755)
    launcher=execs/'launcher.desktop';touch(launcher,('[Desktop Entry]\nType=Application\nName=Run\nExec=touch %s-desktop\n'%marker).encode(),0o644)
    link=execs/'link-to-tool.dfopen';link.symlink_to(tool)
    tool_stat=(tool.stat().st_mode,tool.read_bytes())
    s.launch(folder=docs)

    # Folders keep navigating; a file never navigates.
    select(args);s.key('Return');s.wait(lambda:s.at(args),'Enter on a folder did not navigate');assert not reg.records()
    s.cases.append('Enter on a folder still navigates and launches nothing')

    # Enter on a file: exact selected item, registered default (A), one argument.
    before=requests();select(spaced);s.key('Return');settle(before,'launched');only_new(1,'A',spaced)
    assert 'Opened' in s.state()['status'] and 'Decfiles Fake Opener A' in s.state()['status'],s.state()['status']
    s.cases.append('Enter opens the exact spaced file with the registered default application')

    # Double click on shell metacharacters: one literal argument, no shell, nothing executed.
    before=requests();double_click(meta);settle(before,'launched');only_new(2,'A',meta);assert not pwned(),pwned()
    s.cases.append('double click passes a metacharacter name as one exact argument (no shell, no substitution)')

    # Non-UTF8 name: the exact Unix bytes reach the opener.
    before=requests();s.focus(s.window);s.click(310,s.row_top+50*row(raw_row))
    s.wait(lambda:s.op().get('selected_bytes')==list(raw),'raw selection did not match');s.key('Return');settle(before,'launched')
    wait_records(3);records=reg.records();assert len(records)==3 and reg.argv(records[-1])==[raw],records[-1]
    assert last()['path_bytes']==list(raw);s.cases.append('non-UTF8 file name reaches the opener as exact bytes')

    # Footer state and Ctrl+O on the selected file.
    state=opened();assert state['open_enabled'] and state['open_with_enabled'],state
    before=requests();select(spaced);s.key('ctrl+o');settle(before,'launched');only_new(4,'A',spaced)
    s.cases.append('Ctrl+O opens the validated selection; Open and Open With are enabled for a file')

    # Open With: the native chooser picks B; nothing about defaults changes.
    s.address(chooser);select(target);before=requests();s.key('ctrl+shift+o')
    s.wait(lambda:s.op().get('dialog')=='Open With','Open With chooser did not appear');win=s.wait(lambda:s.find('^Open With$'),'Open With window missing')[0];s.focus(win);s.snap('open-with-chooser',win)
    s.run(['xdotool','type','--clearmodifiers','--delay','50','Decfiles Fake Opener B']);s.key('Return')
    settle(before,'launched');only_new(5,'B',target);reg.unchanged()
    s.cases.append('Open With launches the chosen application (B, not the default A) with the exact file; defaults and associations unchanged')

    # Open With cancel (Escape) is inert.
    select(other);before=requests();s.key('ctrl+shift+o');s.wait(lambda:s.op().get('dialog')=='Open With','chooser missing')
    s.focus(s.wait(lambda:s.find('^Open With$'),'Open With window missing')[0]);s.key('Escape');s.wait(lambda:s.op().get('dialog') is None,'chooser did not close')
    settle(before,'cancelled');assert len(reg.records())==5;reg.unchanged();assert not s.state()['model']['error']
    s.cases.append('cancelling Open With launches nothing, shows no error and changes no default')

    # Executable and .desktop files are never run: default activation explains, Open With may view them.
    s.address(execs)
    for item,status in ((tool,'refused-executable'),(link,'refused-executable'),(launcher,'refused-desktop-entry')):
        before=requests();select(item);s.key('Return');settle(before,status)
        assert len(reg.records())==5 and not marker.exists() and not Path(str(marker)+'-desktop').exists()
        assert 'Open With' in last()['message'] and (tool.stat().st_mode,tool.read_bytes())==tool_stat
    s.snap('executable-refused');s.cases.append('Enter on an executable, a link to it and a .desktop file explains instead of running; mode and bytes untouched')
    select(tool);before=requests();s.key('ctrl+shift+o');s.wait(lambda:s.op().get('dialog')=='Open With','Open With must be offered for an executable')
    s.focus(s.wait(lambda:s.find('^Open With$'),'Open With window missing')[0]);s.key('Escape');settle(before,'cancelled');assert not marker.exists()
    s.cases.append('Open With is still offered for an executable (explicit viewing choice), and cancelling runs nothing')

    # Launch failure and missing handler are shown, never silent.
    s.address(errors)
    before=requests();select(fails);s.key('Return');result=settle(before,'failed')
    assert 'nonexistent-decfiles-opener' in result['message'],result;assert s.state()['model']['error'] and 'nonexistent-decfiles-opener' in s.state()['model']['error']
    before=requests();select(nohandler);s.key('Return');result=settle(before,'failed');assert 'No default application' in result['message'],result
    assert len(reg.records())==5;s.snap('open-failure');s.cases.append('a registered opener that cannot start and a type without handler both show clear errors and launch nothing')

    # Search: Enter still reveals + selects; explicit Open opens the selected result.
    s.address(args);s.focus(s.window);s.key('ctrl+f');s.key('ctrl+a');s.run(['xdotool','type','--clearmodifiers','--delay','1','spaced'])
    s.wait(lambda:s.state()['search'].get('active') and s.state()['search'].get('done') and s.state()['search'].get('count')==1,'search did not finish')
    s.click(310,s.row_top);s.wait(lambda:s.state()['open'].get('search_selected') is not None,'search result not selected')
    before=requests();s.key('ctrl+o');settle(before,'launched');only_new(6,'A',spaced);assert s.state()['search'].get('active'),'explicit Open must stay in search'
    s.key('Return');s.wait(lambda:not s.state()['search'].get('active') and s.op().get('selected')==str(spaced),'Enter on a search result must reveal it')
    assert len(reg.records())==6;s.cases.append('search: Enter reveals and selects, explicit Open (Ctrl+O) opens the selected result')
    s.finish({'features':[],'scope':'Private X11 actual GTK controls and a private MIME registry; fake registered opener only; no default-handler change, Wayland or Orca acceptance'})
finally:
    s.close()
