#!/usr/bin/env python3
"""Offline wallpaper-art retirement audit; native widget rendering/fixture solids are untouched."""
import hashlib
import json
from pathlib import Path
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]

class PackageArtPolicy(unittest.TestCase):
    def test_preserved_palette_layout_widgets_and_declared_asset_removals(self):
        audit = json.loads((ROOT/'docs/generated-wallpaper-removal.json').read_text())
        self.assertEqual(len(audit['themes']), 17)
        for record in audit['themes']:
            with self.subTest(theme=record['theme_id']):
                manifest = tomllib.loads((ROOT/'themes'/record['theme_id']/'theme.toml').read_text())
                self.assertNotIn('wallpaper', manifest)
                for key in ('screenshots', 'description'):
                    manifest.pop(key, None)
                digest = hashlib.sha256(json.dumps(manifest, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()).hexdigest()
                self.assertIn(digest, [record['preserved_manifest_sha256'], record.get('accepted_parent_manifest_sha256')])
                for path, want in record['preserved_widget_files'].items():
                    self.assertEqual(hashlib.sha256((ROOT/path).read_bytes()).hexdigest(), want)
                for asset in record['removed']:
                    self.assertFalse((ROOT/asset['path']).exists(), asset['path'])
        for asset in audit['removed_tools']:
            self.assertFalse((ROOT/asset['path']).exists())

    def test_all_visible_packages_have_independent_wallpaper_choices(self):
        catalogue = json.loads((ROOT/'data/wallhaven-curated.json').read_text())
        self.assertEqual(len(catalogue['themes']), 21)
        for ident in catalogue['themes']:
            with self.subTest(theme=ident):
                package = ROOT/'themes'/ident
                manifest = tomllib.loads((package/'theme.toml').read_text())
                self.assertNotIn('wallpaper', manifest)
                self.assertNotIn('inherit', manifest)
                self.assertIn('Wallpaper → For this theme', (package/'README.md').read_text())
                photos = list((package/'wallpapers').glob('*'))
                self.assertEqual(len(photos), 2 if ident=='sweeteh' else 0)
                self.assertFalse((package/'ARTWORK.md').exists())

    def test_retained_sweeteh_upstream_photos_are_byte_identical(self):
        photos = {'julia-ly-pD0yNIuaLho-unsplash.jpg':'794f829ff0176679f21a0f19f17cc3e524c58222c4f02faf82cf35564822fb2f',
                  'jack-b-JUvlf3OT6kc-unsplash.jpg':'16f9c299f7a3b648106fdfdd70e8af2afbee39b96aa39da48a55fe00215a6c9b'}
        for name, want in photos.items():
            self.assertEqual(hashlib.sha256((ROOT/'themes/sweeteh/wallpapers'/name).read_bytes()).hexdigest(),want)
        attribution = (ROOT/'themes/sweeteh/ATTRIBUTION.md').read_text()
        for token in ['Julia Ly', 'Jack B', 'pD0yNIuaLho', 'JUvlf3OT6kc', 'https://unsplash.com/license']:
            self.assertIn(token, attribution)

if __name__ == '__main__':
    unittest.main()
