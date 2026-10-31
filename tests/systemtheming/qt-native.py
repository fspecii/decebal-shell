#!/usr/bin/env python3
"""Real Qt6 palette/icon probe through a privately supplied qt6ct plugin; no display or session bus.
Set DECTHEMECTL, QT_PROBE (compiled qt_probe.cpp), QT6CT_ROOT (unpacked package), then pass a fresh evidence directory.
This proves newly opened Qt widgets through qt6ct, not KDE/DTK apps or live updates.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
from PIL import Image

out = Path(sys.argv[1])
out.mkdir(mode=0o700)  # existing evidence is never overwritten
ctl = Path(os.environ['DECTHEMECTL']).resolve(strict=True)
probe = Path(os.environ['QT_PROBE']).resolve(strict=True)
plugin = Path(os.environ['QT6CT_ROOT']).resolve(strict=True)
for directory in ('home', 'config/qt6ct', 'data/icons/fixture-icons/apps/32', 'state', 'cache', 'run', 'datadirs'):
    (out / directory).mkdir(parents=True, exist_ok=True)
(out / 'run').chmod(0o700)
icons = out / 'data/icons/fixture-icons'
(icons / 'index.theme').write_text('[Icon Theme]\nName=Fixture icons\nDirectories=apps/32\n[apps/32]\nSize=32\nType=Fixed\n')
(icons / 'apps/32/folder.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><path fill="#2379bf" d="M2 6h12l3 4h13v20H2z"/></svg>')
env = {k: v for k, v in os.environ.items() if k in ('PATH', 'LANG', 'LC_ALL')}
env.update(HOME=str(out/'home'), XDG_CONFIG_HOME=str(out/'config'), XDG_DATA_HOME=str(out/'data'),
           XDG_DATA_DIRS=str(out/'datadirs'), XDG_STATE_HOME=str(out/'state'), XDG_CACHE_HOME=str(out/'cache'),
           XDG_RUNTIME_DIR=str(out/'run'), XDG_CURRENT_DESKTOP='Decebal', DECEBAL_CONFIG=str(out/'config/decebal'),
           DECEBAL_SYSTEM_AUTOMATIC='0', QT_QPA_PLATFORM='offscreen', QT_QPA_PLATFORMTHEME='qt6ct',
           QT_PLUGIN_PATH=str(plugin/'usr/lib/qt6/plugins'), LD_LIBRARY_PATH=str(plugin/'usr/lib'),
           DBUS_SESSION_BUS_ADDRESS='unix:path='+str(out/'run/absent-session'),
           DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(out/'run/absent-system'), QT_DEBUG_PLUGINS='1', RDWMCTL='/bin/false')
(out/'config/decebal').mkdir()
results=[]

def run(argv):
    return subprocess.run([str(a) for a in argv], env=env, capture_output=True, text=True, timeout=30)

for theme in ('nord', 'windows11'):
    for mode in ('dark', 'light'):
        case=out/f'{theme}-{mode}'
        case.mkdir()
        config={'theme': {'name':theme, 'mode':mode}, 'asset_packs':{'downloads':'ask'},
                'system':{'enabled':True, 'iconTheme':'fixture-icons', 'targets':{k:k=='qt' for k in
                          ('gtk','qt','kvantum','omarchy','browser','editors','hooks','x11','terminals')}}}
        (out/'config/decebal/decebal.json').write_text(json.dumps(config))
        applied=run([ctl,'apply-system','--only','qt','--json'])
        (case/'apply.log').write_text(applied.stdout+applied.stderr)
        assert applied.returncode==0, applied.stderr
        palette=run([ctl,'system','palette','--json'])
        assert palette.returncode==0, palette.stderr
        expected=json.loads(palette.stdout)
        native=run([probe,case/'qt.json',case/'qt.png'])
        (case/'qt.log').write_text(native.stdout+native.stderr)
        assert native.returncode==0, native.stderr[-4000:]
        actual=json.loads((case/'qt.json').read_text())
        checks={key: actual[key].lower()==expected[role].lower() for key,role in
                [('window','bg'),('base','bg'),('button','surface'),('text','text'),('highlight','accent'),
                 ('highlight_text','accent_text'),('disabled_text','text_faint')]}
        checks['icon_theme']=actual['icon_theme']=='fixture-icons' and actual['folder_icon']
        checks['plugin_loaded']=actual.get('qt6ct_loaded') is True
        image=Image.open(case/'qt.png').convert('RGB')
        checks['painted_window']=image.getpixel((image.width-5,image.height-5))==tuple(bytes.fromhex(expected['bg'][1:]))
        results.append({'case':case.name,'checks':checks,'actual':actual})
(out/'result.json').write_text(json.dumps({'results':results,'ctl_sha256':hashlib.sha256(ctl.read_bytes()).hexdigest(),
    'probe_sha256':hashlib.sha256(probe.read_bytes()).hexdigest(),'scope':'private offscreen Qt6 with qt6ct; newly opened apps only'},indent=2))
failures=[r['case']+': '+k for r in results for k,v in r['checks'].items() if not v]
print(json.dumps({'checks':sum(len(r['checks']) for r in results),'failures':failures}))
sys.exit(bool(failures))
