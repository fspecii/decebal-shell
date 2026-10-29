#!/usr/bin/env python3
"""Render real widget pixels in a private config; Pillow composes gallery images.
Usage: python tests/glyph-rice-render.py [output-dir]
No installation, daemon, desktop display or live configuration is used.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from PIL import Image, ImageDraw

repo = Path(__file__).resolve().parents[1]
out = Path(sys.argv[1] if len(sys.argv) > 1 else '/tmp/glyph-validation').resolve()
out.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='glyph-render-') as tmp:
    private = Path(tmp)
    env = dict(os.environ, DECEBAL_CONFIG=tmp, DECEBAL_THEMES=str(repo / 'themes'),
               DECEBAL_SYSTEM_THEMES=str(private / 'no-system'), XDG_DATA_HOME=tmp,
               XDG_RUNTIME_DIR=tmp, DISPLAY='', WAYLAND_DISPLAY='')
    for mode in ('dark', 'light'):
        (private / 'decebal.json').write_text(json.dumps({'theme': {'name': 'glyph-rice', 'mode': mode}}))
        for card in ('home', 'system', 'player'):
            subprocess.run([str(repo / 'target/debug/decwidgets'), 'render',
                            str(repo / f'themes/glyph-rice/widgets/glyph-{card}.toml'),
                            '--png', str(out / f'{card}-{mode}.png'), '--margin', '0'],
                           env=env, check=True)
        canvas = Image.open(repo / f'themes/glyph-rice/wallpapers/{mode}.png').convert('RGBA')
        for card, pos in [('home', (150, 130)), ('system', (1400, 170)), ('player', (1400, 390))]:
            canvas.alpha_composite(Image.open(out / f'{card}-{mode}.png').convert('RGBA'), pos)
        ImageDraw.Draw(canvas).text((150, 1040), 'GLYPH RICE / ISOLATED WIDGET COMPOSITION / NOT A DESKTOP SCREENSHOT',
                                   fill='#8c8c8c' if mode == 'dark' else '#6e6e6e')
        canvas.convert('RGB').save(out / f'composition-{mode}.png')
print(out)
