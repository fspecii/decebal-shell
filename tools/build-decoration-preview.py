#!/usr/bin/env python3
"""Compile the optional native preview helper; rdwm sources are READ ONLY.
Usage: python tools/build-decoration-preview.py /path/to/rdwm
Creates build files only below this checkout's ignored target/.
"""
from pathlib import Path
import json
import subprocess
import sys
root = Path(__file__).resolve().parents[1]
rdwm = Path(sys.argv[1]).resolve()
work = root / 'target/decoration-preview-helper'
work.mkdir(parents=True, exist_ok=True)
(work / 'Cargo.toml').write_text(f'''[package]
name = "decebal-deco-preview"
version = "0.1.0"
edition = "2021"

[workspace]

[[bin]]
name = "decebal-deco-preview"
path = {json.dumps(str(root / 'tools/decoration-preview.rs'))}

[dependencies]
rdwm-deco = {{ path = {json.dumps(str(rdwm / 'crates/rdwm-deco'))} }}
toml = "0.8"
png = "0.17"
serde_json = "1"
''')
subprocess.run(['cargo', 'build', '--offline', '--manifest-path', str(work / 'Cargo.toml'), '--target-dir', str(work / 'target')], check=True)
print(work / 'target/debug/decebal-deco-preview')
