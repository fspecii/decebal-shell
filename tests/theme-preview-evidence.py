#!/usr/bin/env python3
"""Regenerate automatic previews from actual packages; no per-theme layout code.
Usage: python tests/theme-preview-evidence.py ID [ID ...]
Requires decthemectl; optional DECEBAL_DECO_RENDERER enables native captions.
"""
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
import os
import subprocess
import sys
import tempfile
root = Path(__file__).resolve().parents[1]
out = root / 'docs/theme-ports/automatic'
out.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='decebal-preview-evidence-') as tmp:
    env = dict(os.environ, HOME=tmp, XDG_CONFIG_HOME=tmp, XDG_CACHE_HOME=str(root/'target/theme-preview-cache'),
               XDG_DATA_HOME=tmp, XDG_STATE_HOME=tmp, XDG_RUNTIME_DIR=tmp,
               DECEBAL_CONFIG=tmp, DECEBAL_THEMES=str(root/'themes'), DECEBAL_SYSTEM_THEMES=tmp,
               DECEBAL_PALETTES=str(root/'palettes'), DECEBAL_SYSTEM_AUTOMATIC='0',
               DISPLAY='', WAYLAND_DISPLAY='', DBUS_SESSION_BUS_ADDRESS='unix:path=/nonexistent')
    def render(pair):
        ident, mode = pair
        result = subprocess.run([str(root/'target/debug/decthemectl'), 'theme', 'render', ident,
                                 '--mode', mode, '--png', str(out/f'{ident}-{mode}.png')], env=env,
                                text=True, capture_output=True, check=True)
        print(result.stdout.strip())
    with ThreadPoolExecutor(max_workers=2) as pool:
        list(pool.map(render, [(ident, mode) for ident in sys.argv[1:] for mode in ('dark','light')]))
