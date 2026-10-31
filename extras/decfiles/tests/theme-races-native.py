#!/usr/bin/env python3
"""Fixture-delay binary only. DECFILES_FIXTURE_THEME_DELAY_MS holds every gtk.css read as if
blocked in a syscall that cancellation cannot retire, so the read lifecycle is observable:
a stale read is never applied, a burst of rewrites keeps one physical read plus one coalesced
request and the latest file wins, and closing stops pending work. Private only."""
import os,subprocess,sys,time
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from harness import Session
binary=os.environ.get('DECFILES_FIXTURE_BIN','/var/tmp/decfiles-transfer-artifacts/fixture-delay/decfiles')
out=os.environ.get('DECFILES_THEME_RACES_EVIDENCE')
def generated(profile,sidebar):
    return f'@define-color sidebar_bg_color {sidebar};\n:root {{\n  --sidebar-bg-color: {sidebar};\n  --sidebar-fg-color: #f5f5f5;\n}}\n/* Files visual profile: {profile} (native file operations) */\n'
def write(session,text):
    path=Path(session.env['XDG_CONFIG_HOME'])/'gtk-4.0'/'gtk.css';path.parent.mkdir(exist_ok=True)
    staging=path.with_suffix('.tmp');staging.write_text(text);staging.rename(path)
def pixel(session,name):
    session.snap(name);value=subprocess.run(['magick',str(session.out/(name+'.png')),'-format','%[hex:p{90,500}]','info:'],capture_output=True,text=True,check=True).stdout.strip()
    return '#'+value[:6].lower()
s=Session(binary,out,'decfiles-theme-races-',{'DECFILES_FIXTURE_THEME_DELAY_MS':'1500'})
profile=lambda:s.state().get('profile',{})
try:
    base=s.home/'Documents';base.mkdir(parents=True)
    write(s,generated('explorer','#c86432'))
    s.start_x();s.launch(folder=base)
    write(s,generated('finder','#2a5a8a'))
    # The held startup read returns with stale (Explorer) text; it must never be applied.
    end=time.monotonic()+12;seen=set()
    while time.monotonic()<end:
        current=profile();seen.add(current.get('id'))
        if current.get('id')=='finder' and current.get('stale_ignored',0)>=1 and not current.get('active_read'):break
        time.sleep(.03)
    assert 'explorer' not in seen and profile().get('id')=='finder',(seen,profile())
    assert s.state()['layout']=='columns' and pixel(s,'latest-after-stale')=='#2a5a8a'
    s.cases.append('a held stale read is discarded, never applied even briefly; the newer file wins after release')
    started,requested=profile()['reads_started'],profile()['requests']
    colours=['#aa3333','#33aa33','#3333aa','#aaaa33','#33aaaa','#aa33aa','#888888','#3c8c50']
    for index,colour in enumerate(colours):
        write(s,generated('finder' if index%2 else 'explorer',colour));time.sleep(.12)
        assert profile().get('reads_started',0)-started<=2,'more than one coalesced read was started'
    s.wait(lambda:profile().get('id')=='finder' and profile().get('palette_reloads',0)>=2 and not profile().get('active_read') and not profile().get('pending_read') and s.state().get('layout')=='columns' and pixel(s,'latest-after-burst')=='#3c8c50','latest burst file not applied',15)
    assert profile()['requests']-requested>=3,profile()
    assert profile()['reads_started']-started<=2,profile()
    assert pixel(s,'latest-after-burst')=='#3c8c50'
    s.cases.append('a burst of rewrites keeps one physical read plus one coalesced request; the latest file wins')
    s.finish({'features':['fixture-delay'],'fixture_theme_delay_ms':1500,'scope':'Controlled fixture hold of every read only'})
finally:
    s.close()
t=Session(binary,(out+'-close') if out else None,'decfiles-theme-close-',{'DECFILES_FIXTURE_THEME_DELAY_MS':'3000'})
profile=lambda:t.state().get('profile',{})
try:
    base=t.home/'Documents';base.mkdir(parents=True)
    write(t,generated('explorer','#c86432'))
    t.start_x();t.launch(folder=base)
    write(t,generated('finder','#2a5a8a'))
    t.wait(lambda:profile().get('active_read') and profile().get('pending_read'),'no pending request behind the held read')
    started=time.monotonic();t.focus(t.window);t.key('ctrl+w');t.app.wait(timeout=5);elapsed=time.monotonic()-started
    final=t.state()['profile'];assert t.app.returncode==0 and elapsed<2.5,(elapsed,t.app.returncode)
    assert final['closed'] and not final['pending_read'] and final['reads_started']==1,final
    t.cases.append('closing during a held read cancels it, drops the pending request and starts nothing new')
    result={'passed':True,'cases':t.cases,'close_elapsed_seconds':elapsed,'display':t.env.get('DISPLAY'),'binary':str(t.binary),
        'sha256':__import__('hashlib').sha256(t.binary.read_bytes()).hexdigest(),'source_hashes':{str(p.relative_to(Path(__file__).resolve().parents[1])):__import__('hashlib').sha256(p.read_bytes()).hexdigest() for p in sorted(Path(__file__).resolve().parents[1].rglob('*')) if p.is_file() and 'target' not in p.parts and (p.suffix=='.rs' or p.name in ('Cargo.toml','Cargo.lock'))},'features':['fixture-delay']}
    (t.out/'result.json').write_text(__import__('json').dumps(result,indent=2)+'\n');print(__import__('json').dumps(result)[:400])
finally:
    t.close()
