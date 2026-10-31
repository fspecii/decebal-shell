#!/usr/bin/env python3
"""Palette and profile following through the shell's generated gtk-4.0/gtk.css, rewritten
live in a private XDG_CONFIG_HOME: the profile marker selects Explorer/Finder/Native, the
default layout follows it until the user chooses one, rendered sidebar pixels follow the
palette, and --profile overrides the file. Private reserved-display Xvfb/bus/HOME/XDG only."""
import os,subprocess,sys,time
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from harness import Session
binary=os.environ.get('DECFILES_BIN',str(Path.home()/'.cache/decebal-files-target/debug/decfiles'))
out=os.environ.get('DECFILES_PROFILE_EVIDENCE')
def generated(profile,sidebar,window='#30343c',sidebar_fg='#f5f5f5'):
    """The shell's forms (theme/system/gtk.rs and file_manager.rs): named colours, :root
    variables, then the Files profile marker with its Nautilus-scoped rules."""
    text=f'/* Fixture palette: dark */\n@define-color window_bg_color {window};\n@define-color sidebar_bg_color {sidebar};\n@define-color sidebar_fg_color {sidebar_fg};\n:root {{\n  --window-bg-color: {window};\n  --sidebar-bg-color: {sidebar};\n  --sidebar-fg-color: {sidebar_fg};\n}}\n'
    if profile:text+=f'/* Files visual profile: {profile} (native file operations) */\n.nautilus-window headerbar {{ padding-top: 4px; padding-bottom: 4px; }}\n'
    return text
def write(session,text):
    path=Path(session.env['XDG_CONFIG_HOME'])/'gtk-4.0'/'gtk.css';path.parent.mkdir(exist_ok=True)
    staging=path.with_suffix('.tmp');staging.write_text(text);staging.rename(path)
def pixel(session,name,x=90,y=500):
    session.snap(name);value=subprocess.run(['magick',str(session.out/(name+'.png')),'-format',f'%[hex:p{{{x},{y}}}]','info:'],capture_output=True,text=True,check=True).stdout.strip()
    return '#'+value[:6].lower()
s=Session(binary,out,'decfiles-profile-native-')
profile=lambda:s.state().get('profile',{})
try:
    base=s.home/'Documents';(base/'Folder').mkdir(parents=True);(base/'file.txt').write_bytes(b'x')
    write(s,generated('finder','#2a5a8a'))
    s.start_x();s.launch(folder=base)
    s.wait(lambda:profile().get('id')=='finder' and s.state().get('layout')=='columns','Finder marker did not choose the Finder profile and columns')
    assert 'decfiles-finder' in profile()['window_classes'] and not profile()['layout_explicit']
    s.wait(lambda:pixel(s,'finder-start')=='#2a5a8a','sidebar did not use the palette sidebar colour')
    s.cases.append('generated Finder marker selects Finder surfaces and default columns; sidebar uses palette colour')
    reloads=profile()['palette_reloads'];write(s,generated('explorer','#c86432'))
    s.wait(lambda:profile().get('id')=='explorer' and profile()['palette_reloads']>reloads and s.state().get('layout')=='details','live rewrite did not switch to Explorer and details')
    s.wait(lambda:pixel(s,'explorer-live')=='#c86432','live palette change not rendered')
    s.cases.append('live gtk.css rewrite switches profile, default layout and rendered palette without restart')
    s.layout(True);assert profile()['layout_explicit']
    write(s,generated('explorer','#3c8c50'));s.wait(lambda:pixel(s,'explicit-columns-new-palette')=='#3c8c50','palette change not rendered with explicit layout')
    assert s.state()['layout']=='columns';write(s,generated('finder','#2a5a8a'));s.wait(lambda:profile().get('id')=='finder','profile not followed');time.sleep(.3)
    write(s,generated('explorer','#3c8c50'));s.wait(lambda:profile().get('id')=='explorer','profile not followed back');time.sleep(.3)
    assert s.state()['layout']=='columns' and profile()['layout_explicit']
    s.cases.append('an explicitly chosen layout survives palette and profile changes')
    too_large=profile().get('too_large',0)
    write(s,generated('finder','#aa00aa')+'/*'+'x'*(3*1024*1024)+'*/\n')
    s.wait(lambda:profile().get('too_large',0)>too_large,'oversized gtk.css was not refused')
    assert profile()['id']=='explorer' and pixel(s,'oversized-ignored')=='#3c8c50' and s.state()['layout']=='columns'
    s.cases.append('a gtk.css over the 2 MiB cap is not read past the cap and changes nothing')
    write(s,generated(None,'#3c8c50'));s.wait(lambda:profile().get('id')=='native' and 'decfiles-native' in profile()['window_classes'],'missing marker did not select Native')
    (Path(s.env['XDG_CONFIG_HOME'])/'gtk-4.0'/'gtk.css').unlink();time.sleep(.6);assert profile()['id']=='native' and s.state()['layout']=='columns'
    s.cases.append('no marker or no file means Native without touching the explicit layout')
    s.finish({'features':[],'scope':'Private X11 actual GTK rendering of a fixture gtk.css in the shell format; no live user config'})
finally:
    s.close()
t=Session(binary,(out+'-cli') if out else None,'decfiles-profile-cli-')
profile=lambda:t.state().get('profile',{})
try:
    base=t.home/'Documents';base.mkdir(parents=True)
    write(t,generated('explorer','#c86432'))
    t.start_x();t.launch('--profile=finder',folder=base)
    t.wait(lambda:profile().get('id')=='finder' and profile().get('from_cli') and profile().get('palette_reloads',0)>0,'--profile did not win')
    assert t.state()['layout']=='columns';write(t,generated('native','#2a5a8a'));time.sleep(.8);assert profile()['id']=='finder'
    t.cases.append('--profile overrides the generated marker; the palette still follows the file')
    t.finish({'features':[],'scope':'Private X11; explicit --profile'})
finally:
    t.close()
