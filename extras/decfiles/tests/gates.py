#!/usr/bin/env python3
"""Run every private native suite against pinned binaries and cross-check hashes.
gates.py ARTIFACTS EVIDENCE_PREFIX  (ARTIFACTS holds default/ and fixture-delay/ from pin.py).
A suite is retried only when no reserved high private display could be proven ours (tests/xserver.py)."""
from pathlib import Path
import hashlib,json,os,subprocess,sys
artifacts,prefix=Path(sys.argv[1]),sys.argv[2];here=Path(__file__).resolve().parent;source_root=here.parent
suites=[('default','native.py','DECFILES_BIN','DECFILES_EVIDENCE','navigation'),
 ('default','operations-native.py','DECFILES_BIN','DECFILES_OPERATIONS_EVIDENCE','operations'),
 ('default','transfers-native.py','DECFILES_BIN','DECFILES_TRANSFERS_EVIDENCE','transfers'),
 ('default','views-native.py','DECFILES_BIN','DECFILES_VIEWS_EVIDENCE','views'),
 ('default','tabs-native.py','DECFILES_BIN','DECFILES_TABS_EVIDENCE','tabs'),
 ('default','search-native.py','DECFILES_BIN','DECFILES_SEARCH_EVIDENCE','search'),
 ('default','profile-native.py','DECFILES_BIN','DECFILES_PROFILE_EVIDENCE','profile'),
 ('default','open-native.py','DECFILES_BIN','DECFILES_OPEN_EVIDENCE','open'),
 ('fixture-delay','close-native.py','DECFILES_FIXTURE_BIN','DECFILES_CLOSE_EVIDENCE','close'),
 ('fixture-delay','transfers-cancel-native.py','DECFILES_FIXTURE_BIN','DECFILES_TRANSFERS_CANCEL_EVIDENCE','transfers-cancel'),
 ('fixture-delay','paste-races-native.py','DECFILES_FIXTURE_BIN','DECFILES_PASTE_RACES_EVIDENCE','paste-races'),
 ('fixture-delay','search-races-native.py','DECFILES_FIXTURE_BIN','DECFILES_SEARCH_RACES_EVIDENCE','search-races'),
 ('fixture-delay','open-races-native.py','DECFILES_FIXTURE_BIN','DECFILES_OPEN_RACES_EVIDENCE','open-races'),
 ('fixture-delay','open-dispatch-native.py','DECFILES_FIXTURE_BIN','DECFILES_OPEN_DISPATCH_EVIDENCE','open-dispatch'),
 ('fixture-delay','theme-races-native.py','DECFILES_FIXTURE_BIN','DECFILES_THEME_RACES_EVIDENCE','theme-races')]
sources={str(p.relative_to(source_root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source_root.rglob('*')) if p.is_file() and 'target' not in p.parts and (p.suffix=='.rs' or p.name in ('Cargo.toml','Cargo.lock'))}
# Pure helper precondition: mocked registry isolation must pass before any native suite starts.
subprocess.run([sys.executable,'-I',str(here/'openfix-test.py')],check=True)
summary={}
for kind,script,bin_var,out_var,name in suites:
    if len(sys.argv)>3 and name not in sys.argv[3:]:continue
    manifest=json.loads((artifacts/kind/'manifest.json').read_text());assert manifest['source_hashes']==sources,f'{kind} binary was built from different sources'
    evidence=f'{prefix}-{name}-evidence'
    for attempt in range(3):
        run=subprocess.run([sys.executable,str(here/script)],env=dict(os.environ,**{bin_var:manifest['binary'],out_var:evidence}),capture_output=True,text=True,timeout=600)
        result=Path(evidence,'result.json')
        if result.exists():break
        logs=' '.join(p.read_text(errors='replace') for p in Path(evidence).glob('*.log'))
        if 'no reserved high private display' not in run.stderr:break
    assert result.exists(),f'{name} failed:\n{run.stdout[-1500:]}\n{run.stderr[-3000:]}'
    data=json.loads(result.read_text());assert data['passed'] and data['sha256']==manifest['sha256'] and data['source_hashes']==sources,name
    summary[name]={'binary':kind,'cases':len(data['cases']),'result':str(result)}
    manifest.setdefault('checks',[]);manifest['checks']=sorted(set(manifest['checks'])|{str(result)});(artifacts/kind/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps(summary,indent=1))
