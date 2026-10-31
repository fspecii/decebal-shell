#!/usr/bin/env python3
"""Staged-install regressions for the Makefile/PKGBUILD install contract (packaging/stage.py).

Everything runs in a private temporary tree with a fake cargo that writes fake executables, a stale ./target/release,
a CARGO_TARGET_DIR and DESTDIR containing spaces, theme assets with spaces in their names, private HOME/XDG and fake
systemctl / update-desktop-database that record any call. No real Rust build, no system or live-session changes."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]
# STAGE_UNDER_TEST points the same tests at another copy of stage.py (fail-before evidence).
STAGE = Path(os.environ.get('STAGE_UNDER_TEST') or REPO / 'packaging/stage.py')
sys.path.insert(0, str(STAGE.parent))
import stage as S  # noqa: E402

FAKE_CARGO = r'''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
log = os.environ.get('FAKE_CARGO_LOG')
if log:
    with open(log, 'a') as f:
        f.write(json.dumps(sys.argv[1:]) + '\n')
if sys.argv[1:2] != ['build'] or '--message-format=json-render-diagnostics' not in sys.argv:
    sys.exit('fake cargo: unexpected ' + ' '.join(sys.argv[1:]))
target = Path(os.environ.get('CARGO_TARGET_DIR') or 'target').resolve() / 'release'
target.mkdir(parents=True, exist_ok=True)
manifest = str(Path('Cargo.toml').resolve())
skip = os.environ.get('FAKE_CARGO_SKIP', '')
nonce = os.environ.get('FAKE_CARGO_NONCE', 'n0')
for name in os.environ['FAKE_CARGO_BINS'].split():
    if name == skip:
        continue
    exe = target / name
    exe.write_text(f'#!/bin/sh\necho {name} build-{nonce}\n')
    exe.chmod(0o755)
    print(json.dumps({'reason': 'compiler-artifact', 'manifest_path': manifest, 'target': {'kind': ['bin'], 'name': name},
                      'executable': str(exe), 'fresh': False}))
# a dependency's binary with a colliding name and an example must be ignored
print(json.dumps({'reason': 'compiler-artifact', 'manifest_path': '/elsewhere/Cargo.toml', 'target': {'kind': ['bin'], 'name': 'decbar'},
                  'executable': '/elsewhere/decbar', 'fresh': True}))
print(json.dumps({'reason': 'compiler-artifact', 'manifest_path': manifest, 'target': {'kind': ['example'], 'name': 'launchapp'},
                  'executable': str(target / 'examples/launchapp'), 'fresh': False}))
print(json.dumps({'reason': 'build-finished', 'success': True}))
'''

RECORDER = '#!/bin/sh\necho "$0 $*" >> "$TOOL_CALL_LOG"\nexit 0\n'


def sha(b):
    return hashlib.sha256(b).hexdigest()


class StageFixture(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='decebal pkg stage ')
        self.addCleanup(self.tmp.cleanup)
        base = Path(self.tmp.name)
        self.src = base / 'src'
        (self.src / 'packaging').mkdir(parents=True)
        shutil.copy2(REPO / 'Makefile', self.src / 'Makefile')
        shutil.copy2(STAGE, self.src / 'packaging/stage.py')
        shutil.copytree(REPO / 'contrib', self.src / 'contrib', symlinks=True)
        for f in ('README.md', 'LICENSE', 'Cargo.toml'):
            shutil.copy2(REPO / f, self.src / f)
        # small themes: one with a font + licence, one with spaces in asset names
        t = self.src / 'themes'
        (t / 'font-theme/fonts').mkdir(parents=True)
        (t / 'font-theme/theme.toml').write_text('name = "Font"\n[font]\nfiles = ["fonts/Sans.ttf"]\n')
        (t / 'font-theme/fonts/Sans.ttf').write_bytes(b'\x00\x01fake-font')
        (t / 'font-theme/OFL.txt').write_text('SIL Open Font License\n')
        (t / 'spaced theme/wallpapers').mkdir(parents=True)
        (t / 'spaced theme/theme.toml').write_text('name = "Spaced"\n')
        (t / 'spaced theme/wallpapers/wall paper (1).png').write_bytes(b'\x89PNG spaced')
        (t / 'spaced theme/widgets').mkdir()
        (t / 'spaced theme/widgets/clock card.toml').write_text('[widget]\n')
        # stale default target that a hard-coded Makefile used to install
        stale = self.src / 'target/release'
        stale.mkdir(parents=True)
        for name in S.BINS:
            (stale / name).write_text('#!/bin/sh\necho STALE\n')
        self.tools = base / 'tools'
        self.tools.mkdir()
        (self.tools / 'fake-cargo').write_text(FAKE_CARGO)
        (self.tools / 'fake-cargo').chmod(0o755)
        for tool in ('systemctl', 'update-desktop-database', 'gtk-update-icon-cache', 'xdg-desktop-menu', 'dbus-send', 'pkill', 'killall'):
            (self.tools / tool).write_text(RECORDER)
            (self.tools / tool).chmod(0o755)
        self.target = base / 'cargo target with spaces'
        self.dest = base / 'dest dir with spaces'
        self.home = base / 'home'
        for d in (self.home, self.home / '.config', self.home / '.local/share', base / 'runtime'):
            d.mkdir(parents=True, exist_ok=True)
        self.calls = base / 'tool-calls.log'
        self.env = {'PATH': f'{self.tools}:/usr/bin:/bin', 'HOME': str(self.home), 'XDG_CONFIG_HOME': str(self.home / '.config'),
                    'XDG_DATA_HOME': str(self.home / '.local/share'), 'XDG_RUNTIME_DIR': str(base / 'runtime'),
                    'TMPDIR': os.environ.get('TMPDIR', '/var/tmp'), 'LANG': 'C.UTF-8',
                    'CARGO_TARGET_DIR': str(self.target), 'FAKE_CARGO_BINS': ' '.join(S.BINS), 'FAKE_CARGO_NONCE': 'n1',
                    'FAKE_CARGO_LOG': str(base / 'cargo.log'), 'TOOL_CALL_LOG': str(self.calls)}

    def make(self, *args, env=None):
        e = dict(self.env, **(env or {}))
        return subprocess.run(['make', '--no-print-directory', '-C', str(self.src), f'CARGO={self.tools}/fake-cargo',
                               'PYTHON=python3 -I', *args], env=e, capture_output=True, text=True, timeout=120)

    def files_under(self, root):
        return sorted(str(p.relative_to(root)) for p in Path(root).rglob('*') if p.is_file() or p.is_symlink())

    def assert_no_side_effects(self):
        self.assertFalse(self.calls.exists(), self.calls.read_text() if self.calls.exists() else '')
        self.assertEqual(self.files_under(self.home), [], 'nothing written into HOME / user config')


class MakeInstallTests(StageFixture):
    def test_install_uses_cargo_reported_target_not_stale_default(self):
        r = self.make('install', f'DESTDIR={self.dest}', 'PREFIX=/opt/decebal-test')
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        bin_dir = self.dest / 'opt/decebal-test/bin'
        manifest = json.loads((self.src / 'target/decebal-build-manifest.json').read_text())
        self.assertEqual(manifest['target_release_dirs'], [str(self.target / 'release')])
        for name in S.BINS:
            data = (bin_dir / name).read_bytes()
            self.assertIn(b'build-n1', data)
            self.assertNotIn(b'STALE', data)
            self.assertEqual(sha(data), manifest['binaries'][name]['sha256'])
            self.assertEqual(stat.S_IMODE((bin_dir / name).stat().st_mode), 0o755)
        inv = json.loads((self.dest / 'opt/decebal-test/share/decebal/install-inventory.json').read_text())
        self.assertEqual(inv['binaries'], {n: b['sha256'] for n, b in manifest['binaries'].items()})
        self.assert_no_side_effects()

    def test_inventory_is_complete_and_exact(self):
        r = self.make('install', f'DESTDIR={self.dest}', 'PREFIX=/opt/decebal-test')
        self.assertEqual(r.returncode, 0, r.stderr)
        p = 'opt/decebal-test'
        compat = [f'{p}/bin/mac{n[3:]}' for n in S.BINS if n not in S.NO_COMPAT]
        expected = sorted(
            [f'{p}/bin/{n}' for n in S.BINS] + compat
            + [f'{p}/lib/systemd/user/decebal-{u}.service' for u in S.UNITS]
            + [f'etc/xdg/autostart/decebal-{u}.desktop' for u in S.UNITS]
            + [f'{p}/share/applications/{a}.desktop' for a in S.APPLICATIONS]
            + [f'{p}/share/decebal/dbus/org.freedesktop.Notifications.service', f'{p}/share/decebal/install-inventory.json',
               f'{p}/share/doc/decebal/README.md', f'{p}/share/licenses/decebal/LICENSE',
               f'{p}/share/decebal/themes/font-theme/theme.toml', f'{p}/share/decebal/themes/font-theme/fonts/Sans.ttf',
               f'{p}/share/decebal/themes/font-theme/OFL.txt', f'{p}/share/decebal/themes/spaced theme/theme.toml',
               f'{p}/share/decebal/themes/spaced theme/wallpapers/wall paper (1).png',
               f'{p}/share/decebal/themes/spaced theme/widgets/clock card.toml'])
        self.assertEqual(self.files_under(self.dest), expected)
        self.assertEqual(len(compat), 18)
        self.assertEqual((self.dest / p / 'share/decebal/themes/spaced theme/wallpapers/wall paper (1).png').read_bytes(), b'\x89PNG spaced')
        for u in S.UNITS:
            text = (self.dest / f'{p}/lib/systemd/user/decebal-{u}.service').read_text()
            self.assertNotIn('/usr/bin/', text)
            self.assertRegex(text, r'ExecStart=/opt/decebal-test/bin/dec')
            self.assertEqual(text.replace('/opt/decebal-test/bin/', '/usr/bin/'), (self.src / f'contrib/decebal-{u}.service').read_text())
        dbus = (self.dest / f'{p}/share/decebal/dbus/org.freedesktop.Notifications.service').read_text()
        self.assertIn('Exec=/opt/decebal-test/bin/decnotify', dbus)
        for u in S.UNITS:   # autostart and desktop entries are copied byte-for-byte
            self.assertEqual((self.dest / f'etc/xdg/autostart/decebal-{u}.desktop').read_bytes(),
                             (self.src / f'contrib/decebal-{u}.desktop').read_bytes())
        self.assertFalse([f for f in self.files_under(self.dest) if '.stage-' in f], 'no temp files left')
        self.assert_no_side_effects()

    def test_install_without_build_manifest_installs_nothing(self):
        r = self.make('-o', 'all', 'install', f'DESTDIR={self.dest}')     # what packaging/deb/build.sh runs
        self.assertNotEqual(r.returncode, 0)
        self.assertIn('run `make`', r.stderr)
        self.assertFalse(self.dest.exists())

    def test_binary_changed_after_build_is_refused(self):
        self.assertEqual(self.make('all').returncode, 0)
        (self.target / 'release/decwm').write_text('#!/bin/sh\necho tampered\n')
        r = self.make('-o', 'all', 'install', f'DESTDIR={self.dest}')
        self.assertNotEqual(r.returncode, 0)
        self.assertIn('decwm', r.stderr)
        self.assertFalse(self.dest.exists())

    def test_cargo_missing_a_binary_fails_the_build(self):
        r = self.make('all', env={'FAKE_CARGO_SKIP': 'decsettings'})
        self.assertNotEqual(r.returncode, 0)
        self.assertIn('decsettings', r.stderr)
        self.assertFalse((self.src / 'target/decebal-build-manifest.json').exists())

    def test_cargo_invocation_is_locked_release(self):
        self.assertEqual(self.make('all').returncode, 0)
        argv = json.loads((Path(self.env['FAKE_CARGO_LOG'])).read_text().splitlines()[0])
        self.assertEqual(argv[:4], ['build', '--release', '--locked', '--message-format=json-render-diagnostics'])

    def test_non_portable_prefix_or_relative_destdir_refused(self):
        self.assertEqual(self.make('all').returncode, 0)
        for args in (['PREFIX=/opt/with space'], ['PREFIX=opt/rel'], ['PREFIX=/opt/../etc'], ['PREFIX=/opt/a;b'],
                     ['DESTDIR=relative/dest']):
            r = self.make('-o', 'all', 'install', *([f'DESTDIR={self.dest}'] if not args[0].startswith('DESTDIR') else []), *args)
            self.assertNotEqual(r.returncode, 0, args)
            self.assertFalse(self.dest.exists(), args)

    def test_unwritable_location_refused_before_any_write(self):
        self.assertEqual(self.make('all').returncode, 0)
        (self.dest / 'etc').mkdir(parents=True)
        (self.dest / 'etc').chmod(0o555)
        self.addCleanup((self.dest / 'etc').chmod, 0o755)
        r = self.make('-o', 'all', 'install', f'DESTDIR={self.dest}')
        self.assertNotEqual(r.returncode, 0)
        self.assertIn('nothing installed', r.stderr)
        self.assertIn('not writable', r.stderr)
        self.assertEqual(self.files_under(self.dest), [])

    def test_undecodable_unit_fails_before_any_write(self):
        self.assertEqual(self.make('all').returncode, 0)
        (self.src / 'contrib/decebal-hot.service').write_bytes(b'ExecStart=/usr/bin/dechot \xff\n')
        r = self.make('-o', 'all', 'install', f'DESTDIR={self.dest}')
        self.assertNotEqual(r.returncode, 0)
        self.assertFalse(self.dest.exists(), 'pass 1 must fail before the first write')

    def test_package_mode_writes_no_inventory_and_uninstall_refuses(self):
        self.assertEqual(self.make('all').returncode, 0)
        r = subprocess.run([sys.executable, '-I', str(self.src / 'packaging/stage.py'), 'install', '--prefix', '/usr', '--destdir',
                            str(self.dest), '--manifest', str(self.src / 'target/decebal-build-manifest.json'), '--no-inventory'],
                           env=self.env, capture_output=True, text=True, timeout=60)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse((self.dest / 'usr/share/decebal/install-inventory.json').exists())
        self.assertTrue((self.dest / 'usr/bin/decwm').exists())
        r = self.make('uninstall', f'DESTDIR={self.dest}', 'PREFIX=/usr')
        self.assertNotEqual(r.returncode, 0, 'make uninstall never removes package-manager-owned files')
        self.assertTrue((self.dest / 'usr/bin/decwm').exists())

    def test_symlink_at_destination_is_replaced_not_followed(self):
        self.assertEqual(self.make('all').returncode, 0)
        outside = Path(self.tmp.name) / 'outside-sentinel'
        outside.write_text('keep')
        (self.dest / 'usr/local/bin').mkdir(parents=True)
        os.symlink(outside, self.dest / 'usr/local/bin/decbar')
        r = self.make('-o', 'all', 'install', f'DESTDIR={self.dest}')
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(outside.read_text(), 'keep')
        self.assertFalse((self.dest / 'usr/local/bin/decbar').is_symlink())

    def test_upgrade_and_uninstall_only_remove_unchanged_owned_files(self):
        r = self.make('install', f'DESTDIR={self.dest}')
        self.assertEqual(r.returncode, 0, r.stderr)
        share = self.dest / 'usr/local/share/decebal/themes'
        # next version drops the font theme; the admin edited one installed file meanwhile
        shutil.rmtree(self.src / 'themes/font-theme')
        (share / 'spaced theme/theme.toml').write_text('name = "edited by admin"\n')
        foreign = share / 'other-vendor/theme.toml'
        foreign.parent.mkdir()
        foreign.write_text('not ours')
        r = self.make('install', f'DESTDIR={self.dest}', env={'FAKE_CARGO_NONCE': 'n2'})
        self.assertEqual(r.returncode, 0, r.stderr)
        report = json.loads(r.stdout.strip().splitlines()[-1])
        self.assertEqual(sorted(report['removed']), sorted(f'/usr/local/share/decebal/themes/font-theme/{f}' for f in ('theme.toml', 'fonts/Sans.ttf', 'OFL.txt')))
        self.assertIn(b'build-n2', (self.dest / 'usr/local/bin/decbar').read_bytes())
        self.assertEqual((share / 'spaced theme/theme.toml').read_text(), 'name = "Spaced"\n', 'shipped file restored on reinstall')
        (share / 'spaced theme/theme.toml').write_text('name = "edited again"\n')
        r = self.make('uninstall', f'DESTDIR={self.dest}')
        self.assertEqual(r.returncode, 0, r.stderr)
        report = json.loads(r.stdout.strip().splitlines()[-1])
        self.assertEqual(report['kept_changed'], ['/usr/local/share/decebal/themes/spaced theme/theme.toml'])
        self.assertEqual(sorted(self.files_under(self.dest)), sorted([
            'usr/local/share/decebal/themes/other-vendor/theme.toml', 'usr/local/share/decebal/themes/spaced theme/theme.toml']))
        self.assert_no_side_effects()

    def test_uninstall_without_inventory_removes_nothing(self):
        (self.dest / 'usr/local/bin').mkdir(parents=True)
        (self.dest / 'usr/local/bin/decbar').write_text('legacy')
        r = self.make('uninstall', f'DESTDIR={self.dest}')
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual((self.dest / 'usr/local/bin/decbar').read_text(), 'legacy')


class ConfinementTests(StageFixture):
    """Attacks on the destination tree. OUTSIDE is a sibling of DESTDIR inside the private fixture."""

    def setUp(self):
        super().setUp()
        self.outside = Path(self.tmp.name) / 'outside'
        self.outside.mkdir()
        if not hasattr(S, 'FAULTS'):       # older stage.py under STAGE_UNDER_TEST: hooks simply never fire
            S.FAULTS = {}
        S.FAULTS.clear()
        self.addCleanup(S.FAULTS.clear)
        self.manifest = self.src / 'target/decebal-build-manifest.json'

    def built(self):
        r = self.make('all')
        self.assertEqual(r.returncode, 0, r.stderr)

    def installed(self, prefix='/usr/local'):
        r = self.make('install', f'DESTDIR={self.dest}', f'PREFIX={prefix}')
        self.assertEqual(r.returncode, 0, r.stderr)

    def snapshot(self, root):
        return {str(p.relative_to(root)): (p.read_bytes() if p.is_file() and not p.is_symlink() else os.readlink(p) if p.is_symlink() else None)
                for p in sorted(Path(root).rglob('*'))}

    def inventory_path(self, prefix='/usr/local'):
        return self.dest / prefix.lstrip('/') / 'share/decebal/install-inventory.json'

    def forge(self, prefix, files, real_prefix='/usr/local'):
        p = self.inventory_path(real_prefix)
        inv = json.loads(p.read_text())
        inv.update(prefix=prefix, files=files)
        p.write_text(json.dumps(inv))

    def test_forged_inventory_prefix_or_foreign_paths_delete_nothing(self):
        self.installed()
        foreign = {'home/user/notes.txt': b'my notes', 'usr/local/lib/libfoo.so': b'lib', 'usr/local/bin/othertool': b'tool',
                   'etc/xdg/autostart/other-app.desktop': b'[Desktop Entry]\n', 'usr/local/share/applications/firefox.desktop': b'x'}
        foreign.update({
            'usr/local/bin/machinectl': b'foreign machine manager',
            'usr/local/bin/macchanger': b'foreign MAC tool',
            'usr/local/bin/decforeign': b'unrelated program',
            'usr/local/share/applications/decforeign.desktop': b'unrelated desktop entry',
            'usr/local/lib/systemd/user/decebal-foreign.service': b'unrelated service',
            'etc/xdg/autostart/decebal-foreign.desktop': b'unrelated autostart',
        })
        for rel, data in foreign.items():
            (self.dest / rel).parent.mkdir(parents=True, exist_ok=True)
            (self.dest / rel).write_bytes(data)
        before = self.snapshot(self.dest)
        cases = [('/', {'/home/user/notes.txt': sha(b'my notes')})]
        cases += [('/usr/local', {'/' + rel: sha(data)}) for rel, data in foreign.items() if not rel.startswith('home/')]
        cases += [('/usr/local', {'/usr/local/share/decebal/../../lib/libfoo.so': sha(b'lib')})]
        for prefix, files in cases:
            self.forge(prefix, files)
            for args in (['uninstall'], ['-o', 'all', 'install']):
                r = self.make(*args, f'DESTDIR={self.dest}')
                self.assertNotEqual(r.returncode, 0, (prefix, files, args))
                self.assertIn('refusing', r.stderr)
            for rel, data in foreign.items():
                self.assertEqual((self.dest / rel).read_bytes(), data, (prefix, files))
        r = self.make('uninstall', f'DESTDIR={self.dest}', 'PREFIX=/')
        self.assertNotEqual(r.returncode, 0)
        self.assertIn('PREFIX=/ is not supported', r.stderr)
        after = self.snapshot(self.dest)
        after.pop('usr/local/share/decebal/install-inventory.json'), before.pop('usr/local/share/decebal/install-inventory.json')
        self.assertEqual(after, before)

    def test_symlinked_ancestor_refused_before_any_install_write(self):
        self.built()
        (self.outside / 'local/bin').mkdir(parents=True)
        self.dest.mkdir()
        os.symlink(self.outside, self.dest / 'usr')
        r = self.make('-o', 'all', 'install', f'DESTDIR={self.dest}')
        self.assertNotEqual(r.returncode, 0)
        self.assertIn('symlink or non-directory parent', r.stderr)
        self.assertEqual(self.files_under(self.outside), [])
        self.assertEqual(sorted(os.listdir(self.outside)), ['local'])
        self.assertEqual(os.listdir(self.dest), ['usr'])
        self.assertFalse((self.dest / 'etc').exists(), 'no other folder created before the refusal')

    def test_symlinked_ancestor_at_uninstall_removes_nothing(self):
        self.installed()
        # bin replaced by a symlink to OUTSIDE holding byte-identical copies (same digests as the inventory)
        shutil.move(self.dest / 'usr/local/bin', self.outside / 'bin')
        os.symlink(self.outside / 'bin', self.dest / 'usr/local/bin')
        before_out, before_dest = self.snapshot(self.outside), self.snapshot(self.dest)
        r = self.make('uninstall', f'DESTDIR={self.dest}')
        self.assertNotEqual(r.returncode, 0)
        self.assertIn('nothing removed', r.stderr)
        self.assertEqual(self.snapshot(self.outside), before_out)
        self.assertEqual(self.snapshot(self.dest), before_dest)

    def test_inventory_behind_symlinked_parent_or_symlink_is_refused(self):
        self.installed()
        share = self.dest / 'usr/local/share'
        shutil.move(share, self.outside / 'share')
        os.symlink(self.outside / 'share', share)
        before = self.snapshot(self.outside)
        r = self.make('uninstall', f'DESTDIR={self.dest}')
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(self.snapshot(self.outside), before)
        os.unlink(share)
        shutil.move(self.outside / 'share', share)
        inv = self.inventory_path()
        shutil.move(inv, self.outside / 'inv.json')
        os.symlink(self.outside / 'inv.json', inv)
        before = self.snapshot(self.dest)
        r = self.make('uninstall', f'DESTDIR={self.dest}')
        self.assertNotEqual(r.returncode, 0)
        self.assertIn('not a regular file', r.stderr)
        self.assertEqual(self.snapshot(self.dest), before)

    def test_parent_replaced_mid_install_is_not_followed(self):
        self.built()
        fired = []

        def swap(path):
            if '/share/decebal/themes/' in path and not fired:
                fired.append(path)
                share = self.dest / 'usr/local/share'
                os.rename(share, self.dest / 'moved-share')
                os.symlink(self.outside, share)
        S.FAULTS['before_write'] = swap
        with self.assertRaisesRegex(S.StageError, 'symlink or non-directory parent'):
            S.install('/usr/local', str(self.dest), str(self.manifest), source=self.src)
        self.assertTrue(fired)
        self.assertEqual(self.files_under(self.outside), [], 'nothing written through the swapped-in symlink')

    def test_parent_replaced_mid_uninstall_is_not_followed(self):
        self.installed()
        mirror = self.outside / 'bin'
        shutil.copytree(self.dest / 'usr/local/bin', mirror)
        fired = []

        def swap(path):
            if not fired:
                fired.append(path)
                b = self.dest / 'usr/local/bin'
                os.rename(b, self.dest / 'moved-bin')
                os.symlink(mirror, b)
        S.FAULTS['before_unlink'] = swap
        before = self.snapshot(self.outside)
        with self.assertRaisesRegex(S.StageError, 'symlink or non-directory parent'):
            S.uninstall('/usr/local', str(self.dest))
        self.assertTrue(fired)
        self.assertEqual(self.snapshot(self.outside), before, 'no OUTSIDE file unlinked')

    def _limit_with_oversized_theme_file(self):
        sizes = [Path(src).stat().st_size for _, src, *_ in S.inventory('/usr/local', json.loads(self.manifest.read_text()), self.src)]
        limit = max(sizes) + 16
        (self.src / 'themes/spaced theme/wallpapers/huge wall.png').write_bytes(b'x' * (limit + 1))
        return limit

    def test_oversized_source_refused_before_any_write(self):
        self.built()
        limit = self._limit_with_oversized_theme_file()
        old = S.MAX_FILE
        S.MAX_FILE = limit
        self.addCleanup(setattr, S, 'MAX_FILE', old)
        with self.assertRaisesRegex(S.StageError, f'over the {limit}-byte limit'):
            S.install('/usr/local', str(self.dest), str(self.manifest), source=self.src)
        self.assertFalse(self.dest.exists())

    def test_source_growing_past_limit_while_read_refused(self):
        self.built()
        target = self.src / 'themes/spaced theme/theme.toml'
        sizes = [Path(src).stat().st_size for _, src, *_ in S.inventory('/usr/local', json.loads(self.manifest.read_text()), self.src)]
        old = S.MAX_FILE
        S.MAX_FILE = max(sizes) + 16
        self.addCleanup(setattr, S, 'MAX_FILE', old)

        def grow(fd, label):
            if label == str(target):
                with open(target, 'ab') as f:
                    f.write(b'#' * (S.MAX_FILE + 1))
        S.FAULTS['source_opened'] = grow
        with self.assertRaisesRegex(S.StageError, 'grew past'):
            S.install('/usr/local', str(self.dest), str(self.manifest), source=self.src)
        self.assertFalse(self.dest.exists())

    def test_final_name_that_is_a_directory_is_refused_up_front(self):
        self.built()
        (self.dest / 'usr/local/bin/decbar').mkdir(parents=True)
        r = self.make('-o', 'all', 'install', f'DESTDIR={self.dest}')
        self.assertNotEqual(r.returncode, 0)
        self.assertIn('is a directory', r.stderr)
        self.assertEqual(self.files_under(self.dest), [])


class RepositoryContractTests(unittest.TestCase):
    """Read-only checks of the real tree."""

    def test_binary_list_matches_sources_and_cargo(self):
        self.assertEqual(len(S.BINS), 23)
        self.assertEqual(sorted(S.BINS), sorted(p.stem for p in (REPO / 'src/bin').glob('*.rs')))
        cargo = (REPO / 'Cargo.toml').read_text()
        names = re.findall(r'\[\[bin\]\]\s*\nname\s*=\s*"([^"]+)"', cargo)
        self.assertEqual(sorted(names), sorted(S.BINS))

    def test_units_autostart_and_applications_match_contrib(self):
        c = REPO / 'contrib'
        self.assertEqual(sorted(f'decebal-{u}.service' for u in S.UNITS), sorted(p.name for p in c.glob('*.service')))
        self.assertEqual(sorted(f'decebal-{u}.desktop' for u in S.UNITS), sorted(p.name for p in c.glob('decebal-*.desktop')))
        self.assertEqual(sorted(f'{a}.desktop' for a in S.APPLICATIONS), sorted(p.name for p in c.glob('dec[!e]*.desktop')))
        for p in c.glob('*.service'):
            self.assertNotRegex(p.read_text(), r'(?m)^ExecStart=(?!/usr/bin/)', 'units must use /usr/bin/ so PREFIX rewriting works')

    def test_every_theme_file_is_delivered_and_fonts_carry_licences(self):
        items = S.inventory('/usr')
        themes = sorted(str(p.relative_to(REPO / 'themes')) for p in (REPO / 'themes').rglob('*') if p.is_file())
        delivered = sorted(d.split('/usr/share/decebal/themes/', 1)[1] for d, *_ in items if '/share/decebal/themes/' in d)
        self.assertEqual(delivered, themes)
        for theme in (REPO / 'themes').iterdir():
            if any(theme.glob('fonts/*.[ot]tf')):
                self.assertTrue([p for p in theme.iterdir() if re.search(r'licen|ofl|copying', p.name, re.I)], theme.name)
        self.assertEqual(len(items), 23 + 18 + 2 * len(S.UNITS) + len(S.APPLICATIONS) + 1 + len(themes) + 2)

    def test_top_level_widgets_and_data_are_compiled_in(self):
        samples = (REPO / 'src/widgets/samples.rs').read_text()
        for w in (REPO / 'widgets').glob('*.toml'):
            self.assertIn(f'include_str!("../../widgets/{w.name}")', samples, f'{w.name} is neither compiled in nor installed')
        self.assertIn('include_str!("../../../data/wallhaven-curated.json")', (REPO / 'src/settings/wallhaven/curated.rs').read_text())

    def test_python_build_dependency_is_declared(self):
        self.assertRegex((REPO / 'PKGBUILD').read_text(), r"(?m)^makedepends=\(.*'python'.*\)")
        install = (REPO / 'README.md').read_text().split('## Install', 1)[1].split('\n## ', 1)[0]
        self.assertIn('python3', install)
        self.assertIn('python3', (REPO / 'packaging/README.md').read_text().split('| Debian', 1)[1].split('\n', 1)[0])

    def test_makefile_and_pkgbuild_share_the_staging_contract(self):
        mk = (REPO / 'Makefile').read_text()
        install = mk[mk.index('\ninstall:'):mk.index('\nuninstall:')]
        self.assertNotIn('target/release', install)
        self.assertIn('packaging/stage.py install', install)
        self.assertNotIn('systemctl', mk.split('# Extra login session')[0])
        pkg = (REPO / 'PKGBUILD').read_text()
        self.assertIn('packaging/stage.py install', pkg)
        self.assertIn('packaging/stage.py build', pkg)
        self.assertNotIn('systemctl', pkg)
        ver = re.search(r'(?m)^version = "([^"]+)"', (REPO / 'Cargo.toml').read_text()).group(1)
        self.assertEqual(re.search(r'(?m)^pkgver=(.+)$', pkg).group(1), ver)


if __name__ == '__main__':
    unittest.main(verbosity=2)
