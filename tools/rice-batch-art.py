#!/usr/bin/env python3
"""Rebuild original Kiln Court / Tidal Loom PNG art. Requires Pillow only."""
from pathlib import Path
import math
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
W, H = 3840, 2160


def wallpaper(theme, mode):
    light = mode == 'light'
    if theme == 'kiln-court':
        bg, stone, shade, edge, sun = (('#ede1d1', '#d4b79d', '#ad7e65', '#f7eee2', '#a75338') if light else
                                      ('#241e1c', '#47322c', '#342523', '#79513f', '#df986f'))
        im = Image.new('RGB', (W, H), bg)
        d = ImageDraw.Draw(im)
        # An asymmetric colonnade: nested vaults and a cut of daylight.
        # The left third is deliberately unoccupied for the widget column.
        for i in range(5):
            x, y = 1450 + i * 225, 260 + i * 150
            right = 3590 - i * 125
            d.rounded_rectangle((x, y, right, H + 800), radius=(right-x)//2, fill=stone if i % 2 == 0 else shade)
            d.arc((x+12, y+12, right-12, y+right-x-12), 180, 355, fill=edge, width=5)
        d.ellipse((2750, 730, 2990, 970), fill=sun)
        d.polygon([(1450, 1850), (3590, 1850), (W, H), (1110, H)], fill=shade)
        for y in (1940, 2040, 2140):
            d.line((1260, y, W, y), fill=edge, width=2)
    else:
        bg, colors = (('#e7eff0', ['#b7d0d3', '#83b5bd', '#537e9a', '#b7a6c8', '#e3d5e7']) if light else
                      ('#121e2c', ['#1c3448', '#24526a', '#307b89', '#726e9c', '#aba2c4']))
        im = Image.new('RGB', (W, H), bg)
        d = ImageDraw.Draw(im)
        # Broad continuous fabric bands, with fine warp lines following the same flow.
        # Upper right is reserved for native widget cards.
        for band, color in enumerate(colors):
            def curve(x, offset=0):
                return 1050 + band*145 + 270*math.sin(x/W*math.pi*2-0.6) + 190*math.cos(x/W*math.pi) + offset
            top = [(x, int(curve(x))) for x in range(-10, W+11, 10)]
            bottom = [(x, int(curve(x, 250))) for x in range(W+10, -11, -10)]
            d.polygon(top+bottom, fill=color)
            for offset in (15, 35, 55):
                pts = [(x, int(curve(x, offset))) for x in range(0, W+1, 8)]
                d.line(pts, fill=bg, width=2)
    path = ROOT / 'themes' / theme / 'wallpapers' / f'{mode}.png'
    path.parent.mkdir(parents=True, exist_ok=True)
    im.save(path, optimize=True)


if __name__ == '__main__':
    for theme in ('kiln-court', 'tidal-loom'):
        for mode in ('dark', 'light'):
            wallpaper(theme, mode)
