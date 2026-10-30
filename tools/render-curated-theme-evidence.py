#!/usr/bin/env python3
"""Native, isolated package evidence. No network or live apply.

By default render the actual package (no photograph if it has none). Optional
--dark-photo / --light-photo build a TEMPORARY QA package with those already
cached images. The real package remains untouched; photo output stays in /tmp.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('theme')
p.add_argument('--dark-photo', type=Path)
p.add_argument('--light-photo', type=Path)
p.add_argument('--out', type=Path, required=True)
a = p.parse_args()
if not a.theme.replace('-', '').isalnum():
    p.error('theme must be an installed package ID')
if bool(a.dark_photo) != bool(a.light_photo):
    p.error('provide both photos or neither')
source = root / 'themes' / a.theme
photos = bool(a.dark_photo)
if photos and not a.out.resolve().is_relative_to(Path('/tmp')):
    p.error('private photograph evidence must stay under /tmp, outside git')
a.out.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='decebal-curated-qa-') as tmp:
    private = Path(tmp)
    package = private / 'themes' / a.theme
    shutil.copytree(source, package)
    manifest = package / 'theme.toml'
    if photos:
        # Aster deliberately omits wallpaper. Avoid duplicate sections/ambiguous inheritance.
        text = manifest.read_text()
        if '[wallpaper]' in text or '\ninherit' in text:
            p.error('photo QA requires an explicit package without wallpaper/inheritance')
        (package / 'wallpapers').mkdir()
        metadata = {}
        for mode in ('dark', 'light'):
            photo = getattr(a, mode + '_photo').resolve()
            shutil.copyfile(photo, package / 'wallpapers' / (mode + photo.suffix))
            metadata[mode] = {'source': str(photo), 'sha256': hashlib.sha256(photo.read_bytes()).hexdigest()}
        text += '\n[wallpaper]\nmode = "fill"\n' + ''.join(f'{m} = "wallpapers/{m}{getattr(a,m+"_photo").suffix}"\n' for m in ('dark','light'))
        manifest.write_text(text)
        (a.out / 'private-photo-inputs.json').write_text(json.dumps(metadata, indent=2) + '\n')
    env = dict(os.environ, DECEBAL_CONFIG=str(private/'config'), DECEBAL_THEMES=str(private/'themes'),
               DECEBAL_SYSTEM_THEMES=str(private/'no-system'), DECEBAL_PALETTES=str(root/'palettes'),
               XDG_CACHE_HOME=str(private/'cache'), DECEBAL_SYSTEM_AUTOMATIC='0', DISPLAY='',
               WAYLAND_DISPLAY='', DBUS_SESSION_BUS_ADDRESS='unix:path=/nonexistent')
    for mode in ('dark', 'light'):
        subprocess.run([str(root/'target/debug/decthemectl'), 'theme', 'render', a.theme,
                        '--mode', mode, '--png', str(a.out.resolve()/f'{a.theme}-{mode}.png')], env=env, check=True)
