#!/usr/bin/env python3
"""Native headless widget evidence; sample data, no desktop/session access.
Run after cargo build --bin decwidgets --bin decthemectl.
Pillow places native PNGs on a labelled solid UI test canvas; never creates wallpaper assets.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'docs' / 'rice-batch'
OUT.mkdir(parents=True, exist_ok=True)

with tempfile.TemporaryDirectory(prefix='decebal-rice-render-') as tmp:
    p = Path(tmp)
    env = dict(os.environ, HOME=tmp, XDG_CONFIG_HOME=tmp, XDG_DATA_HOME=tmp,
               XDG_STATE_HOME=tmp, XDG_CACHE_HOME=tmp, XDG_RUNTIME_DIR=tmp,
               DECEBAL_CONFIG=tmp, DECEBAL_THEMES=str(ROOT / 'themes'),
               DECEBAL_SYSTEM_THEMES=str(p / 'no-system'), DECEBAL_PALETTES=str(p / 'palettes'),
               DECEBAL_SYSTEM_AUTOMATIC='0', DISPLAY='', WAYLAND_DISPLAY='',
               DBUS_SESSION_BUS_ADDRESS='unix:path=/nonexistent')
    for theme in ('kiln-court', 'tidal-loom'):
        package = ROOT / 'themes' / theme
        manifest = tomllib.loads((package / 'theme.toml').read_text())
        for mode in ('dark', 'light'):
            (p / 'decebal.json').write_text(json.dumps({'theme': {'name': theme, 'mode': mode}}))
            canvas = Image.new('RGBA', (1920, 1080), manifest['colors'][mode]['background'])
            boxes = []
            for spec in manifest['widgets']:
                widget = spec['use'].removeprefix('builtin:')
                assert (ROOT / 'widgets' / f'{widget}.toml').exists()
                target = OUT / f'{theme}-{widget}-{mode}.png'
                subprocess.run([str(ROOT / 'target/debug/decwidgets'), 'render', spec['use'],
                                '--png', str(target), '--margin', '0'], env=env, check=True)
                card = Image.open(target).convert('RGBA')
                x = spec['x'] if spec['anchor'].endswith('left') else 1920 + spec['x'] - card.width
                y = spec['y']
                assert 0 <= x and 0 <= y and x+card.width <= 1920 and y+card.height <= 1080
                box = (x, y, x+card.width, y+card.height)
                assert all(box[2] <= b[0] or b[2] <= box[0] or box[3] <= b[1] or b[3] <= box[1] for b in boxes)
                boxes.append(box)
                canvas.alpha_composite(card, (x, y))
            ImageDraw.Draw(canvas).text((64, 1028), f'{manifest["name"]} / {mode.upper()} / NATIVE WIDGET RENDER / SAMPLE DATA / NO PACKAGED WALLPAPER',
                                       fill=manifest['colors'][mode]['text_secondary'])
            canvas.convert('RGB').save(OUT / f'{theme}-{mode}.png', optimize=True)
        subprocess.run([str(ROOT / 'target/debug/decthemectl'), 'theme', 'validate', str(package)], env=env, check=True)
print(f'Evidence: {OUT}')
