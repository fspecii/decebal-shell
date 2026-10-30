#!/usr/bin/env python3
"""Compose actual native headless evidence emitted by tests/theme_ports.rs.
No UI chrome is painted here. Sample/empty data are always labelled.
Usage: python tests/theme-ports-compose.py [mountain-neon-dot|glyph-rice ...]
"""
from pathlib import Path
import sys
import tomllib
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
for ident in sys.argv[1:] or ['mountain-neon-dot', 'glyph-rice', 'sweeteh']:
    package = ROOT / 'themes' / ident
    manifest = tomllib.loads((package / 'theme.toml').read_text())
    width, height = (1920, 1200) if ident == 'glyph-rice' else (1920, 1080)
    out = ROOT / 'docs/theme-ports' / ident
    for mode in ('dark', 'light'):
        for state in ('sample', 'empty'):
            # Neutral UI evidence, not a replacement/generated wallpaper asset.
            canvas = Image.new('RGBA', (width, height), manifest['colors'][mode]['background'])
            boxes = []
            for spec in manifest['widgets']:
                card = Image.open(out / f'{spec["use"]}-{mode}-{state}.png').convert('RGBA')
                x, y = spec['x'], spec['y']
                anchor = spec['anchor']
                if anchor.endswith('right'):
                    x += width-card.width
                elif anchor in ('top', 'bottom', 'center'):
                    x += (width-card.width)//2
                if anchor.startswith('bottom'):
                    y += height-card.height
                assert 0 <= x and 0 <= y and x+card.width <= width and y+card.height <= height, (spec, card.size)
                box = (x, y, x+card.width, y+card.height)
                assert all(box[2] <= b[0] or b[2] <= box[0] or box[3] <= b[1] or b[3] <= box[1] for b in boxes), (spec, boxes)
                boxes.append(box)
                canvas.alpha_composite(card, (x, y))
            label = f'{manifest["name"]} / {mode.upper()} / NATIVE WIDGETS / {state.upper()} DATA / NO PACKAGED WALLPAPER / NOT A DESKTOP CAPTURE'
            ImageDraw.Draw(canvas).text((65, height-24), label, fill=manifest['colors'][mode]['text'])
            canvas.convert('RGB').save(out / f'{mode}-{state}.png', optimize=True)
    print(out)
