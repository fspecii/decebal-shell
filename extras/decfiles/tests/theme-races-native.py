#!/usr/bin/env python3
"""Fixture-delay binary only: the startup gtk.css read is held (DECFILES_FIXTURE_THEME_DELAY_MS)
while the file is rewritten; the newer read applies first and the older, delayed completion
is ignored by generation instead of reverting palette, profile or layout. Private only."""
import os,subprocess,sys,time
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from harness import Session
binary=os.environ.get('DECFILES_FIXTURE_BIN','/var/tmp/decfiles-transfer-artifacts/fixture-delay/decfiles')
s=Session(binary,os.environ.get('DECFILES_THEME_RACES_EVIDENCE'),'decfiles-theme-races-',{'DECFILES_FIXTURE_THEME_DELAY_MS':'4000'})
def generated(profile,sidebar):
    return f'@define-color sidebar_bg_color {sidebar};\n:root {{\n  --sidebar-bg-color: {sidebar};\n  --sidebar-fg-color: #f5f5f5;\n}}\n/* Files visual profile: {profile} (native file operations) */\n'
def write(text):
    path=Path(s.env['XDG_CONFIG_HOME'])/'gtk-4.0'/'gtk.css';path.parent.mkdir(exist_ok=True)
    staging=path.with_suffix('.tmp');staging.write_text(text);staging.rename(path)
def pixel(name):
    s.snap(name);value=subprocess.run(['magick',str(s.out/(name+'.png')),'-format','%[hex:p{90,500}]','info:'],capture_output=True,text=True,check=True).stdout.strip()
    return '#'+value[:6].lower()
profile=lambda:s.state().get('profile',{})
try:
    base=s.home/'Documents';base.mkdir(parents=True)
    write(generated('explorer','#c86432'))
    s.start_x();s.launch(folder=base)
    assert profile().get('generation')==1 and profile().get('palette_reloads',0)==0,'startup read should still be held'
    write(generated('finder','#2a5a8a'))
    s.wait(lambda:profile().get('id')=='finder' and profile().get('generation',0)>=2 and s.state().get('layout')=='columns','newer read did not apply')
    s.wait(lambda:pixel('newer-applied')=='#2a5a8a','newer palette not rendered')
    s.wait(lambda:profile().get('stale_ignored',0)>=1,'held startup read never completed',10)
    time.sleep(.5)
    assert profile()['id']=='finder' and s.state()['layout']=='columns' and pixel('after-stale-completion')=='#2a5a8a',profile()
    s.cases.append('an older delayed gtk.css read completing after a newer one is ignored by generation')
    s.finish({'features':['fixture-delay'],'fixture_theme_delay_ms':4000,'scope':'Controlled fixture delay of the startup read only'})
finally:
    s.close()
