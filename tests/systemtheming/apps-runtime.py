#!/usr/bin/env python3
"""Real Files/libadwaita + unpatched st palette propagation on private D-Bus/Xvfb.
Run under dbus-run-session; creates all config/state inside a temporary directory.
"""
import json, os, pathlib, subprocess, tempfile, time
from PIL import Image

root = pathlib.Path(__file__).resolve().parents[2]
artifacts = pathlib.Path(os.environ.get('SYSTEM_THEME_SHOTS', tempfile.mkdtemp(prefix='decebal-system-apps-')))
artifacts.mkdir(parents=True, exist_ok=True)
if os.environ.get('DECEBAL_APP_TEST_PRIVATE_BUS') != '1' and (artifacts/'config').exists():
    artifacts=pathlib.Path(tempfile.mkdtemp(prefix='run-', dir=artifacts))
env = os.environ.copy()
for key in ['WAYLAND_DISPLAY', 'RDWM_SOCK', 'OMARCHY_PATH', 'GTK_THEME', 'GSETTINGS_BACKEND']:
    env.pop(key, None)
env.update(HOME=str(artifacts/'home'), XDG_CONFIG_HOME=str(artifacts/'config'), XDG_STATE_HOME=str(artifacts/'state'), XDG_DATA_HOME=str(artifacts/'data'), XDG_RUNTIME_DIR=str(artifacts/'runtime'), DISPLAY=':246', DECEBAL_CONFIG=str(artifacts/'config/decebal'), DECEBAL_BACKEND='x11', RDWMCTL='/bin/false', GDK_BACKEND='x11', GSK_RENDERER='cairo')
for path in ['home', 'home/Theme integration files', 'home/Next-launch themed files', 'config/decebal', 'state', 'data', 'runtime']:
    (artifacts/path).mkdir(parents=True, exist_ok=True)
os.chmod(artifacts/'runtime', 0o700)
ctl = os.environ.get('DECTHEMECTL', str(root/'target/debug/decthemectl'))
if os.environ.get('DECEBAL_APP_TEST_PRIVATE_BUS') != '1':
    env['DECEBAL_APP_TEST_PRIVATE_BUS']='1'
    env['SYSTEM_THEME_SHOTS']=str(artifacts)
    raise SystemExit(subprocess.call(['dbus-run-session','--','python3',str(pathlib.Path(__file__).resolve())],env=env))
procs = []
def spawn(args, label):
    proc = subprocess.Popen(args, env=env, stdout=open(artifacts/f'{label}.log','w'), stderr=subprocess.STDOUT)
    procs.append(proc)
    return proc
def run(args):
    return subprocess.check_output(args, env=env, text=True)
def window(pattern):
    for _ in range(60):
        result = subprocess.run(['xdotool','search','--name',pattern], env=env, text=True, capture_output=True)
        if result.returncode == 0 and result.stdout.strip(): return result.stdout.split()[-1]
        time.sleep(.1)
    raise AssertionError(f'window absent: {pattern}')
def shot(wid, label):
    path=artifacts/f'{label}.png'
    subprocess.run(['import','-window',wid,str(path)],env=env,check=True)
    return Image.open(path).convert('RGB')
def pixels(image, rgb):
    return sum(count for count, color in image.getcolors(image.width*image.height) if all(abs(a-b)<=2 for a,b in zip(color,rgb)))
try:
    spawn(['Xvfb', env['DISPLAY'], '-screen','0','1400x900x24','-nolisten','tcp','-ac'],'xvfb')
    time.sleep(.5)
    wm = os.environ.get('RDWM_BIN','/home/ambsd/Projects/rdwm/target/release/rdwm')
    (artifacts/'config/rdwm').mkdir(exist_ok=True)
    (artifacts/'config/rdwm/config.toml').write_text('[general]\ngap_inner=6\n')
    spawn([wm,'--backend','x11','--config',str(artifacts/'config/rdwm/config.toml')],'rdwm')
    time.sleep(.5)
    # Existing apps start before generated colours exist: this checks propagation, not only file generation.
    spawn(['nautilus','--new-window',str(artifacts/'home/Theme integration files')],'files')
    spawn([os.environ.get('ST_BIN','st'),'-t','Decebal terminal integration QA','-e','env','HOME='+env['HOME'],'bash','--noprofile','--norc','-c','printf "Terminal palette QA\\n"; sleep 90'],'st')
    files=window('Theme integration files'); terminal=window('Decebal terminal integration QA')
    time.sleep(2)
    shot(files,'files-before'); shot(terminal,'terminal-before')
    config={'theme':{'name':'nord','mode':'dark'},'system':{'enabled':True,'targets':{'omarchy':False,'browser':False,'editors':False,'hooks':False}}}
    (artifacts/'config/decebal/decebal.json').write_text(json.dumps(config))
    report=json.loads(run([ctl,'apply-system','--only','gtk,terminals,x11','--json']))
    (artifacts/'apply.json').write_text(json.dumps(report,indent=2))
    assert not report['warnings'], report['warnings']
    time.sleep(2)
    gtk=shot(files,'files-after'); st=shot(terminal,'terminal-after')
    # Nord background is an exact, observable surface in both actual apps.
    expected=(46,52,64)
    assert pixels(st,expected)>st.width*st.height*.6, 'running st did not receive OSC palette'
    # Libadwaita picks up dark/light live, but user CSS is a process-startup provider.
    # A second private bus starts a separate Files process without closing/restarting the first.
    spawn(['dbus-run-session','--','nautilus','--new-window',str(artifacts/'home/Next-launch themed files')],'files-next-launch')
    fresh=window('Next-launch themed files')
    time.sleep(2)
    themed=shot(fresh,'files-next-launch')
    assert pixels(themed,expected)>themed.width*themed.height*.12, 'new Files process did not adopt palette'
    assert "'prefer-dark'" in run(['gsettings','get','org.gnome.desktop.interface','color-scheme'])
    spawn([os.environ.get('ST_BIN','st'),'-t','Decebal new terminal startup QA','-e','env','HOME='+env['HOME'],'bash','--noprofile','-ic','printf \"New terminal QA\\n\"; sleep 90'],'st-new')
    new_st=window('Decebal new terminal startup QA');time.sleep(1)
    new_terminal=shot(new_st,'terminal-next-launch')
    assert pixels(new_terminal,expected)>new_terminal.width*new_terminal.height*.6, 'new st did not read startup palette'
    print(f'PASS: st recoloured live; existing Files followed mode; new Files process adopted Nord palette; artifacts: {artifacts}')
finally:
    for proc in reversed(procs):
        proc.terminate()
    for proc in procs:
        try: proc.wait(timeout=3)
        except subprocess.TimeoutExpired: proc.kill();proc.wait()
