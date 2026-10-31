#!/usr/bin/env python3
"""Run every private native suite against pinned binaries and cross-check hashes.
gates.py ARTIFACTS EVIDENCE_PREFIX  (ARTIFACTS holds default/ and fixture-delay/ from pin.py).
A suite is retried only when its private Xvfb could not start (other sessions race -displayfd)."""
from pathlib import Path
import hashlib,json,os,subprocess,sys
artifacts,prefix=Path(sys.argv[1]),sys.argv[2];here=Path(__file__).resolve().parent;source_root=here.parent
suites=[('default','native.py','DECFILES_BIN','DECFILES_EVIDENCE','navigation'),
 ('default','operations-native.py','DECFILES_BIN','DECFILES_OPERATIONS_EVIDENCE','operations'),
 ('default','transfers-native.py','DECFILES_BIN','DECFILES_TRANSFERS_EVIDENCE','transfers'),
 ('default','views-native.py','DECFILES_BIN','DECFILES_VIEWS_EVIDENCE','views'),
 ('fixture-delay','close-native.py','DECFILES_FIXTURE_BIN','DECFILES_CLOSE_EVIDENCE','close'),
 ('fixture-delay','transfers-cancel-native.py','DECFILES_FIXTURE_BIN','DECFILES_TRANSFERS_CANCEL_EVIDENCE','transfers-cancel'),
 ('fixture-delay','paste-races-native.py','DECFILES_FIXTURE_BIN','DECFILES_PASTE_RACES_EVIDENCE','paste-races')]
sources={str(p.relative_to(source_root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source_root.rglob('*')) if p.is_file() and 'target' not in p.parts and (p.suffix=='.rs' or p.name in ('Cargo.toml','Cargo.lock'))}
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
        if 'Cannot write display number' not in logs and 'Failed to open display' not in logs and 'no private Xvfb' not in run.stderr:break
    assert result.exists(),f'{name} failed:\n{run.stdout[-1500:]}\n{run.stderr[-3000:]}'
    data=json.loads(result.read_text());assert data['passed'] and data['sha256']==manifest['sha256'] and data['source_hashes']==sources,name
    summary[name]={'binary':kind,'cases':len(data['cases']),'result':str(result)}
    manifest.setdefault('checks',[]);manifest['checks']=sorted(set(manifest['checks'])|{str(result)});(artifacts/kind/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps(summary,indent=1))
