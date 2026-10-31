#!/usr/bin/env python3
"""Pin one built binary with its exact source hashes: pin.py BINARY OUT_DIR [feature...]"""
from pathlib import Path
import datetime,hashlib,json,shutil,subprocess,sys
binary,out=Path(sys.argv[1]).resolve(),Path(sys.argv[2]);features=sys.argv[3:]
source_root=Path(__file__).resolve().parents[1];out.mkdir(parents=True,exist_ok=True)
pinned=out/'decfiles';shutil.copy2(binary,pinned)
sources={str(p.relative_to(source_root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source_root.rglob('*')) if p.is_file() and 'target' not in p.parts and (p.suffix=='.rs' or p.name in ('Cargo.toml','Cargo.lock'))}
command=['cargo','build','--offline','-j2','--manifest-path','extras/decfiles/Cargo.toml']+(['--features',','.join(features)] if features else [])
manifest={'features':features,'binary':str(pinned),'sha256':hashlib.sha256(pinned.read_bytes()).hexdigest(),'source_hashes':sources,'build_command':command,
 'rustc':subprocess.run(['rustc','--version'],capture_output=True,text=True).stdout.strip(),'pinned_utc':datetime.datetime.now(datetime.timezone.utc).isoformat()}
(out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n');print(json.dumps({'binary':str(pinned),'sha256':manifest['sha256']}))
