#!/usr/bin/env python3
"""Private-fixture regressions for tools/release/decebal_release.py: bundle verification, read-only plans, handoff,
and (below) per-file publish/rollback with fault injection. Nothing here touches live folders, processes or config."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import sys
import tempfile
import unittest

# DECEBAL_RELEASE_TOOL lets the same tests run against an older copy of the tool (fail-before evidence).
SPEC = importlib.util.spec_from_file_location('decebal_release', os.environ.get('DECEBAL_RELEASE_TOOL')
                                              or Path(__file__).resolve().parents[1] / 'tools/release/decebal_release.py')
R = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(R)

COMMIT = 'a' * 40


def sha(b):
    return hashlib.sha256(b).hexdigest()


def make_bundle(root: Path, bins=None, tamper=None):
    """A minimal sealed decebal-frozen-release-manifest/1 bundle, laid out like manifest.py writes it."""
    bins = bins or {'decbar': b'#!/bin/sh\necho bar-new\n', 'decwm': b'#!/bin/sh\necho wm-new\n', 'decthemectl': b'#!/bin/sh\necho t\n',
                    'examples/launchapp': b'#!/bin/sh\necho l\n'}
    art = root / 'art'
    (art / 'bin/examples').mkdir(parents=True)
    for n, b in bins.items():
        (art / 'bin' / n).write_bytes(b)
        (art / 'bin' / n).chmod(0o555)
    digests = {n: sha(b) for n, b in bins.items()}
    src = {'head': COMMIT, 'dirty': False, 'tree': 'b' * 40, 'production_fingerprint': 'c' * 64, 'tracked_fingerprint': 'd' * 64,
           'production_inputs': {'Cargo.toml': 'e' * 64, 'Cargo.lock': 'f' * 64}}
    records = {
        'source-before.json': json.dumps(src).encode(), 'source-after.json': json.dumps(src).encode(),
        'result.json': json.dumps({'passed': True, 'sha': COMMIT, 'dirty': False, 'binaries': digests}).encode(),
        'build.log': b'Finished\n', 'build-messages.jsonl': b'{}\n',
        'SHA256SUMS': ''.join(f'{d}  {n}\n' for n, d in sorted(digests.items())).encode(),
    }
    for n, b in records.items():
        (art / n).write_bytes(b)
    m = {'schema': R.SCHEMA, 'sealed_utc': 'now',
         'source': {'commit': COMMIT, 'tree': 'b' * 40, 'dirty': False, 'branch': 'qa/x', 'production_fingerprint': 'c' * 64,
                    'tracked_fingerprint': 'd' * 64, 'key_inputs': {'Cargo.toml': 'e' * 64}, 'production_inputs_unchanged_during_build': True,
                    'pin_record_sha256': '0' * 64},
         'build': {'exit': 0, 'timed_out': False, 'profile': 'release', 'locked': True, 'offline': True, 'env_keys': ['HOME', 'PATH']},
         'binaries': {n: {'sha256': d, 'size': len(bins[n]), 'cargo_executable': '/x/' + n, 'cargo_fresh': False, 'opt_level': '3',
                          'debug_assertions': False} for n, d in digests.items()},
         'records': {n: sha(b) for n, b in records.items()}}
    if tamper:
        tamper(m, art)
    blob = (json.dumps(m, indent=2, sort_keys=True) + '\n').encode()
    (art / 'MANIFEST.json').write_bytes(blob)
    (art / 'MANIFEST.json.sha256').write_text(sha(blob) + '  MANIFEST.json\n')
    for n in list(records) + ['MANIFEST.json', 'MANIFEST.json.sha256']:
        (art / n).chmod(0o444)
    return art


class Base(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='decebal-release-test-')
        self.addCleanup(self._cleanup)
        self.root = Path(os.path.realpath(self.tmp.name))
        R.FAULTS.clear()
        self.addCleanup(R.FAULTS.clear)
        saved = R.CONFIG_WATCH
        R.CONFIG_WATCH = ()                      # never hash the real user config from tests
        self.addCleanup(setattr, R, 'CONFIG_WATCH', saved)

    def _cleanup(self):
        for d, dirs, files in os.walk(self.root):
            os.chmod(d, 0o700)
        self.tmp.cleanup()


class VerifyTests(Base):
    def test_clean_bundle_verifies(self):
        rep = R.verify(make_bundle(self.root), expect_commit=COMMIT)
        self.assertTrue(rep['ok'], rep['problems'])
        self.assertEqual(len(rep['binaries']), 4)
        self.assertEqual(rep['build']['env_keys'], ['HOME', 'PATH'])

    def test_wrong_expected_commit(self):
        rep = R.verify(make_bundle(self.root), expect_commit='b' * 40)
        self.assertFalse(rep['ok'])

    def test_binary_content_mismatch(self):
        art = make_bundle(self.root)
        p = art / 'bin/decbar'
        p.chmod(0o755)
        p.write_bytes(b'#!/bin/sh\necho evil\n')
        p.chmod(0o555)
        rep = R.verify(art)
        self.assertTrue(any('decbar' in x for x in rep['problems']), rep['problems'])

    def test_manifest_seal_mismatch(self):
        art = make_bundle(self.root)
        (art / 'MANIFEST.json.sha256').chmod(0o644)
        (art / 'MANIFEST.json.sha256').write_text('0' * 64 + '  MANIFEST.json\n')
        self.assertIn('MANIFEST.json.sha256 does not seal MANIFEST.json', R.verify(art)['problems'])

    def test_manifest_digest_disagrees_with_file(self):
        def t(m, art):
            m['binaries']['decwm']['sha256'] = '1' * 64
        rep = R.verify(make_bundle(self.root, tamper=t))
        self.assertFalse(rep['ok'])
        self.assertTrue(any('decwm' in x for x in rep['problems']))

    def test_unknown_manifest_key_refused(self):
        def t(m, art):
            m['post_install'] = ['systemctl --user restart decbar']
        rep = R.verify(make_bundle(self.root, tamper=t))
        self.assertFalse(rep['ok'])
        self.assertIn('manifest keys', rep['problems'][0])

    def test_path_traversal_name_refused(self):
        def t(m, art):
            m['binaries']['../../etc/x'] = dict(m['binaries']['decbar'])
        self.assertFalse(R.verify(make_bundle(self.root, tamper=t))['ok'])

    def test_symlinked_binary_refused(self):
        art = make_bundle(self.root)
        target = self.root / 'outside'
        target.write_bytes(b'#!/bin/sh\necho bar-new\n')
        (art / 'bin').chmod(0o755)
        os.unlink(art / 'bin/decbar')
        os.symlink(target, art / 'bin/decbar')
        rep = R.verify(art)
        self.assertTrue(any('symlink' in x for x in rep['problems']), rep['problems'])

    def test_extra_file_in_bin_refused(self):
        art = make_bundle(self.root)
        (art / 'bin/rogue').write_bytes(b'x')
        self.assertIn('unexpected file in bin/: rogue', R.verify(art)['problems'])

    def test_writable_artifact_refused(self):
        art = make_bundle(self.root)
        (art / 'bin/decwm').chmod(0o755)
        self.assertIn('binary decwm: writable artifact', R.verify(art)['problems'])

    def test_record_tamper_and_dirty_provenance(self):
        def t(m, art):
            m['source']['dirty'] = True
        art = make_bundle(self.root, tamper=t)
        self.assertIn('source was dirty', R.verify(art)['problems'])
        art2 = make_bundle(self.root / 'two')
        (art2 / 'build.log').chmod(0o644)
        (art2 / 'build.log').write_text('edited\n')
        self.assertTrue(any(x.startswith('record build.log') for x in R.verify(art2)['problems']))

    def test_source_before_after_drift_refused(self):
        def t(m, art):
            p = art / 'source-after.json'
            s = json.loads(p.read_text())
            s['production_fingerprint'] = '9' * 64
            p.chmod(0o644)
            p.write_text(json.dumps(s))
            m['records']['source-after.json'] = sha(p.read_bytes())
        rep = R.verify(make_bundle(self.root, tamper=t))
        self.assertIn('source-after: production fingerprint differs', rep['problems'])

    def test_wrongly_typed_manifest_field_is_a_problem_not_a_crash(self):
        def t(m, art):
            m['source']['key_inputs'] = ['Cargo.toml']
        rep = R.verify(make_bundle(self.root, tamper=t))
        self.assertFalse(rep['ok'])
        self.assertIn('malformed', rep['problems'][-1])

    def test_fifo_in_place_of_binary_does_not_hang(self):
        art = make_bundle(self.root)
        (art / 'bin').chmod(0o755)
        os.unlink(art / 'bin/decwm')
        os.mkfifo(art / 'bin/decwm')
        rep = R.verify(art)
        self.assertTrue(any('decwm' in x for x in rep['problems']), rep['problems'])

    def test_size_bound(self):
        def t(m, art):
            m['binaries']['decbar']['size'] = R.MAX_BINARY + 1
        self.assertFalse(R.verify(make_bundle(self.root, tamper=t))['ok'])

    def test_symlinked_artifact_dir_refused(self):
        art = make_bundle(self.root)
        os.symlink(art, self.root / 'link')
        self.assertFalse(R.verify(self.root / 'link')['ok'])


class PlanTests(Base):
    def setUp(self):
        super().setUp()
        self.art = make_bundle(self.root)
        self.dest = self.root / 'private/bin'
        self.dest.mkdir(parents=True, mode=0o700)
        (self.dest / 'decbar').write_bytes(b'#!/bin/sh\necho bar-old\n')
        (self.dest / 'decbar').chmod(0o755)
        (self.dest / 'decthemectl').write_bytes(b'#!/bin/sh\necho t\n')
        (self.dest / 'rdwm').write_bytes(b'not ours')

    def test_plan_actions_and_untouched(self):
        p = R.plan(self.art, [str(self.dest)], proc=self.root / 'noproc', runtime_dir=self.root / 'run')
        acts = {f['name']: f['action'] for f in p['files']}
        self.assertEqual(acts, {'decbar': 'replace', 'decthemectl': 'unchanged', 'decwm': 'create'})
        self.assertEqual(p['untouched_entries'][str(self.dest)], ['rdwm'])
        self.assertEqual((p['config_changes'], p['runtime_actions'], p['live_destinations']), ([], [], []))
        self.assertEqual(p['plan_sha256'], R._canon_sha(p))
        self.assertIn('decwm', p['handoff']['never_touch'])
        self.assertEqual(sorted(os.listdir(self.dest)), ['decbar', 'decthemectl', 'rdwm'], 'plan must not write')

    def test_plan_refuses_symlink_destination_entry(self):
        os.unlink(self.dest / 'decbar')
        os.symlink('/bin/true', self.dest / 'decbar')
        with self.assertRaisesRegex(R.ReleaseError, 'not a regular file'):
            R.plan(self.art, [str(self.dest)])

    def test_plan_refuses_symlinked_dest_dir_and_writable_dir(self):
        os.symlink(self.dest, self.root / 'dlink')
        with self.assertRaisesRegex(R.ReleaseError, 'symlink'):
            R.plan(self.art, [str(self.root / 'dlink')])
        self.dest.chmod(0o777)
        with self.assertRaisesRegex(R.ReleaseError, 'writable'):
            R.plan(self.art, [str(self.dest)])

    def test_plan_refuses_unknown_name_and_bad_manifest(self):
        with self.assertRaisesRegex(R.ReleaseError, 'not in the manifest'):
            R.plan(self.art, [str(self.dest)], only=['decsomething'])
        (self.art / 'bin').chmod(0o755)
        (self.art / 'bin/rogue').write_bytes(b'x')
        with self.assertRaisesRegex(R.ReleaseError, 'verification failed'):
            R.plan(self.art, [str(self.dest)])

    def test_handoff_exact_owned_pids_only(self):
        proc, run = self.root / 'proc', self.root / 'run'
        run.mkdir()
        me = os.getuid()

        def fake(pid, exe, comm):
            d = proc / str(pid)
            d.mkdir(parents=True)
            os.symlink(exe, d / 'exe')
            (d / 'stat').write_text(f'{pid} ({comm}) S ' + ' '.join(['1'] * 18) + ' 4242 0\n')
            (d / 'comm').write_text(comm + '\n')
        fake(100, str(self.dest / 'decbar'), 'decbar')            # owned, ours
        fake(200, '/usr/bin/python3', 'decbar')                   # reused pid / same name, other exe
        (run / 'decbar.lock').write_text('100')
        (run / 'decbar-bottom.lock').write_text('200')
        p = R.plan(self.art, [str(self.dest)], only=['decbar'], proc=proc, runtime_dir=run)
        holders = p['handoff']['components'][0]['holders']
        self.assertEqual(holders[0]['state'], 'owned')
        self.assertEqual(holders[0]['pid'], 100)
        self.assertEqual(holders[0]['starttime_ticks'], 4242)
        self.assertTrue(holders[1]['state'].startswith('not-owned'))
        self.assertEqual(me, os.getuid())

    def test_live_destination_is_classified(self):
        self.assertTrue(R.is_live(R.LIVE_DIRS[0]))
        self.assertTrue(R.is_live(Path.home() / '.local/share'))
        self.assertFalse(R.is_live(self.dest))


OLD_BAR = b'#!/bin/sh\necho bar-old\n'
OLD_WM = b'#!/bin/sh\necho wm-old\n'


class PublishFixture(Base):
    def setUp(self):
        super().setUp()
        self.art = make_bundle(self.root)
        self.priv = self.root / 'private'
        self.priv.mkdir()
        self.priv.chmod(0o700)
        self.dest = self.priv / 'bin'
        self.dest.mkdir()
        self.dest.chmod(0o755)
        (self.dest / 'decbar').write_bytes(OLD_BAR)
        (self.dest / 'decbar').chmod(0o750)
        os.utime(self.dest / 'decbar', ns=(1_000_000_000, 1_000_000_000))
        (self.dest / 'decwm').write_bytes(OLD_WM)
        (self.dest / 'decwm').chmod(0o755)
        (self.dest / 'decthemectl').write_bytes(b'#!/bin/sh\necho t\n')      # unchanged
        (self.dest / 'rdwm').write_bytes(b'foreign')                          # untouched
        self.journal = self.root / 'journal'
        self.lock = self.root / 'release.lock'

    def make_plan(self, only=None):
        self.cfg = self.root / 'decebal.json'
        if not self.cfg.exists():
            self.cfg.write_text('{"theme": "x"}')
        p = R.plan(self.art, [str(self.dest)], only=only, proc=self.root / 'noproc', runtime_dir=self.root / 'run',
                   config_watch=[self.cfg, self.root / 'absent.toml'])
        path = self.root / f'plan-{len(os.listdir(self.root))}.json'
        path.write_text(json.dumps(p))
        return path, p

    def snapshot(self):
        return {n: (self.dest / n).read_bytes() for n in sorted(os.listdir(self.dest))}

    def publish(self, plan_path):
        return R.publish(plan_path, self.priv, self.journal, self.lock)

    def journal_file(self, name):
        data = json.loads((self.journal / 'journal.json').read_text())
        return data, next(f for f in data['files'] if f['name'] == name)


class PublishTests(PublishFixture):
    def test_publish_commits_per_file_and_preserves_metadata(self):
        path, p = self.make_plan()
        self.assertEqual(p['summary'], {'create': 0, 'replace': 2, 'unchanged': 1})
        res = self.publish(path)
        self.assertEqual(res['state'], 'committed', res.get('error'))
        self.assertEqual((self.dest / 'decbar').read_bytes(), b'#!/bin/sh\necho bar-new\n')
        self.assertEqual(stat.S_IMODE(os.stat(self.dest / 'decbar').st_mode), 0o750)
        self.assertEqual((self.dest / 'rdwm').read_bytes(), b'foreign')
        self.assertEqual(sorted(os.listdir(self.dest)), ['decbar', 'decthemectl', 'decwm', 'rdwm'], 'no staged leftovers')
        data, bar = self.journal_file('decbar')
        self.assertEqual((self.journal / 'backups' / bar['backup']).read_bytes(), OLD_BAR)
        self.assertEqual({f['state'] for f in data['files']}, {'committed', 'unchanged'})
        self.assertFalse(data['restarts_sent'])
        self.assertTrue(data['configs_match_plan'])

    def test_create_action_and_rollback_of_committed_publish_via_cli(self):
        os.unlink(self.dest / 'decwm')
        before_bar = os.stat(self.dest / 'decbar')
        plan_out = self.root / 'cli-plan.json'
        tool = Path(__file__).resolve().parents[1] / 'tools/release/decebal_release.py'
        import subprocess
        run = lambda *a: subprocess.run([sys.executable, '-I', str(tool), *a], capture_output=True, text=True, timeout=60)
        r = run('plan', str(self.art), '--dest', str(self.dest), '--out', str(plan_out))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(json.loads(plan_out.read_text())['summary']['create'], 1)
        r = run('publish', '--plan', str(plan_out), '--private-root', str(self.priv), '--journal', str(self.journal), '--lock', str(self.lock))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertEqual(json.loads(r.stdout)['state'], 'committed')
        self.assertTrue((self.dest / 'decwm').exists())
        r = run('rollback', '--journal', str(self.journal), '--lock', str(self.lock))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertFalse((self.dest / 'decwm').exists(), 'created file removed on rollback')
        self.assertEqual((self.dest / 'decbar').read_bytes(), OLD_BAR)
        after = os.stat(self.dest / 'decbar')
        self.assertEqual((stat.S_IMODE(after.st_mode), after.st_mtime_ns), (stat.S_IMODE(before_bar.st_mode), before_bar.st_mtime_ns))
        self.assertNotEqual(after.st_ino, before_bar.st_ino, 'rollback restores content/mode/mtime, not the original inode')

    def test_refuses_live_or_loose_private_root_and_outside_destinations(self):
        path, _ = self.make_plan()
        with self.assertRaisesRegex(R.ReleaseError, 'live'):
            R._private_root_check(Path.home() / '.local/share')
        self.priv.chmod(0o750)
        with self.assertRaisesRegex(R.ReleaseError, '0700'):
            self.publish(path)
        self.priv.chmod(0o700)
        other = self.root / 'other'
        other.mkdir(mode=0o700)
        with self.assertRaisesRegex(R.ReleaseError, 'not inside the private root'):
            R.publish(path, other, self.journal, self.lock)
        self.assertFalse(self.journal.exists())

    def test_refuses_edited_plan_and_runtime_changes(self):
        path, p = self.make_plan()
        p['files'][0]['new_sha256'] = '0' * 64
        path.write_text(json.dumps(p))
        with self.assertRaisesRegex(R.ReleaseError, 'edited plan'):
            self.publish(path)
        path, p = self.make_plan()
        p['runtime_actions'] = [{'kill': 'decwm'}]
        p['plan_sha256'] = R._canon_sha(p)
        path.write_text(json.dumps(p))
        with self.assertRaisesRegex(R.ReleaseError, 'config or runtime'):
            self.publish(path)
        path, p = self.make_plan()
        p['files'][0]['action'] = 'create'          # replace entry relabelled create, sha recomputed
        p['plan_sha256'] = R._canon_sha(p)
        path.write_text(json.dumps(p))
        with self.assertRaisesRegex(R.ReleaseError, 'inconsistent'):
            self.publish(path)
        path, p = self.make_plan()
        p['files'].append(dict(p['files'][0]))
        p['plan_sha256'] = R._canon_sha(p)
        path.write_text(json.dumps(p))
        with self.assertRaisesRegex(R.ReleaseError, 'distinct'):
            self.publish(path)
        path, p = self.make_plan()
        p['files'][0]['action'] = 'chmod'
        p['plan_sha256'] = R._canon_sha(p)
        path.write_text(json.dumps(p))
        with self.assertRaisesRegex(R.ReleaseError, 'unknown plan entry'):
            self.publish(path)

    def test_refuses_when_manifest_changes_after_planning(self):
        path, _ = self.make_plan()
        (self.art / 'bin').chmod(0o755)
        (self.art / 'bin/rogue').write_bytes(b'x')
        snap = self.snapshot()
        with self.assertRaisesRegex(R.ReleaseError, 'no longer verifies'):
            self.publish(path)
        self.assertEqual(self.snapshot(), snap)

    def test_config_drift_since_planning_aborts_and_is_never_written(self):
        path, p = self.make_plan()
        self.assertEqual(p['config_watch'][str(self.root / 'absent.toml')], None)
        self.cfg.write_text('{"theme": "y"}')
        snap = self.snapshot()
        res = self.publish(path)
        self.assertEqual(res['state'], 'aborted_before_changes')
        self.assertIn('config changed since planning', res['error'])
        self.assertEqual(self.snapshot(), snap)
        self.assertEqual(self.cfg.read_text(), '{"theme": "y"}')
        self.assertNotIn('theme', json.dumps(p['config_watch']), 'only digests, never config content')

    def test_stale_destination_before_any_change_aborts(self):
        path, _ = self.make_plan()
        (self.dest / 'decwm').write_bytes(b'edited after plan\n')
        snap = self.snapshot()
        res = self.publish(path)
        self.assertEqual(res['state'], 'aborted_before_changes')
        self.assertIn('decwm', res['error'])
        self.assertEqual(self.snapshot(), snap)

    def test_partial_staged_write_rolls_back(self):
        path, _ = self.make_plan()
        snap = self.snapshot()

        def truncate(fd, index, **_):
            if index == 2:                       # decwm, after decbar committed
                os.ftruncate(fd, 5)
        R.FAULTS['stage_write'] = truncate
        res = self.publish(path)
        self.assertEqual(res['state'], 'rolled_back')
        self.assertIn('partial or altered write', res['error'])
        self.assertEqual(self.snapshot(), snap)
        self.assertEqual(sorted(os.listdir(self.dest)), ['decbar', 'decthemectl', 'decwm', 'rdwm'])

    def test_external_replacement_before_commit_is_left_in_place(self):
        path, _ = self.make_plan()

        def swap_in(index, path):
            if index == 2:
                tmp = self.dest / 'ext.tmp'
                tmp.write_bytes(b'external\n')
                os.replace(tmp, path)
        R.FAULTS['before_commit'] = swap_in
        res = self.publish(path)
        self.assertEqual(res['state'], 'rolled_back', res)
        self.assertIn('stale destination', res['error'])
        self.assertEqual((self.dest / 'decwm').read_bytes(), b'external\n', 'external file never clobbered')
        self.assertEqual((self.dest / 'decbar').read_bytes(), OLD_BAR, 'earlier committed file rolled back')
        self.assertEqual(sorted(os.listdir(self.dest)), ['decbar', 'decthemectl', 'decwm', 'rdwm'])

    def test_external_replacement_after_commit_is_a_rollback_conflict(self):
        path, _ = self.make_plan()

        def after(index, path):
            if index == 2:
                tmp = self.dest / 'ext.tmp'
                tmp.write_bytes(b'external\n')
                os.replace(tmp, self.dest / 'decbar')
                raise R.ReleaseError('injected failure after last commit')
        R.FAULTS['after_commit'] = after
        res = self.publish(path)
        self.assertEqual(res['state'], 'rollback_incomplete')
        self.assertEqual(res['unresolved'], [str(self.dest / 'decbar')])
        self.assertEqual((self.dest / 'decbar').read_bytes(), b'external\n')
        self.assertEqual((self.dest / 'decwm').read_bytes(), OLD_WM)
        _, bar = self.journal_file('decbar')
        self.assertEqual(bar['state'], 'rollback_conflict')

    def test_failed_rollback_is_reported_not_hidden(self):
        path, _ = self.make_plan()

        def fail(index, path):
            if index == 2:
                raise R.ReleaseError('injected failure after last commit')

        def corrupt(index, backup):
            os.chmod(backup, 0o600)
            Path(backup).write_bytes(b'corrupt')
        R.FAULTS['after_commit'] = fail
        R.FAULTS['rollback_restore'] = corrupt
        res = self.publish(path)
        self.assertEqual(res['state'], 'rollback_incomplete')
        self.assertEqual(sorted(res['unresolved']), [str(self.dest / 'decbar'), str(self.dest / 'decwm')])
        self.assertEqual((self.dest / 'decwm').read_bytes(), b'#!/bin/sh\necho wm-new\n', 'left at the verified new content')
        _, wm = self.journal_file('decwm')
        self.assertEqual(wm['state'], 'rollback_failed')
        self.assertIn('digest', wm['error'])
        self.assertEqual(sorted(os.listdir(self.dest)), ['decbar', 'decthemectl', 'decwm', 'rdwm'])

    def test_crash_between_swap_and_journal_recovers_from_identities(self):
        path, _ = self.make_plan()

        def crash(index, path):
            if index == 2:
                raise KeyboardInterrupt('simulated process death right after the swap')
        R.FAULTS['after_swap'] = crash
        with self.assertRaises(KeyboardInterrupt):
            self.publish(path)
        R.FAULTS.clear()
        data, wm = self.journal_file('decwm')
        self.assertEqual((data['state'], wm['state']), ('publishing', 'staged'))
        self.assertEqual((self.dest / 'decwm').read_bytes(), b'#!/bin/sh\necho wm-new\n')
        res = R.rollback(self.journal, self.lock)
        self.assertEqual(res['state'], 'rolled_back')
        self.assertEqual((self.dest / 'decwm').read_bytes(), OLD_WM)
        self.assertEqual((self.dest / 'decbar').read_bytes(), OLD_BAR)
        _, wm = self.journal_file('decwm')
        self.assertEqual(wm['restored_from'], wm['staged'], 'displaced original swapped back, not copied')
        self.assertEqual(sorted(os.listdir(self.dest)), ['decbar', 'decthemectl', 'decwm', 'rdwm'])

    def test_disk_full_mid_publish_then_recovery(self):
        """The sequence seen on a full tmpfs: a staged write fails with ENOSPC, then journal writes fail too. Rollback
        still restores from on-disk identities; the final journal write failure surfaces; a later rollback finishes."""
        path, _ = self.make_plan()
        snap = self.snapshot()
        full = {'on': False}

        def enospc_stage(fd, index, **_):
            if index == 2:
                full['on'] = True
                raise OSError(28, 'No space left on device')

        def enospc_journal(journal):
            if full['on']:
                raise OSError(28, 'No space left on device')
        R.FAULTS['stage_write'] = enospc_stage
        R.FAULTS['journal_save'] = enospc_journal
        with self.assertRaises(OSError):
            self.publish(path)
        self.assertEqual(self.snapshot(), snap, 'files restored even though the journal could not record it')
        data, wm = self.journal_file('decwm')
        self.assertEqual(data['state'], 'publishing', 'last durable journal state is honest about being unfinished')
        self.assertFalse([n for n in os.listdir(self.journal) if n.startswith('.journal')], 'no torn journal temp files')
        R.FAULTS.clear()
        res = R.rollback(self.journal, self.lock)
        self.assertEqual(res['state'], 'rolled_back', res)
        self.assertEqual(self.snapshot(), snap)
        self.assertEqual(R.rollback(self.journal, self.lock)['state'], 'rolled_back', 'idempotent')

    def test_staged_file_with_unjournaled_identity_is_preserved(self):
        path, _ = self.make_plan()
        fired = {'n': 0}

        def die_on_identity_save(journal):
            if journal.data['files'][2].get('staged_identity') and not fired['n']:
                fired['n'] = 1
                raise KeyboardInterrupt('simulated death before the staged identity reached disk')
        R.FAULTS['journal_save'] = die_on_identity_save
        with self.assertRaises(KeyboardInterrupt):
            self.publish(path)
        R.FAULTS.clear()
        _, wm = self.journal_file('decwm')
        self.assertEqual((wm['state'], wm['staged_identity']), ('staging', None))
        res = R.rollback(self.journal, self.lock)
        self.assertEqual(res['state'], 'rolled_back')
        _, wm = self.journal_file('decwm')
        self.assertEqual(wm['preserved'], wm['staged'])
        self.assertTrue((self.dest / wm['staged']).exists(), 'unverifiable staged file must not be deleted')
        self.assertEqual((self.dest / 'decbar').read_bytes(), OLD_BAR)

    def test_lock_held_refuses(self):
        path, _ = self.make_plan()
        fd = R._lock(self.lock)
        try:
            with self.assertRaisesRegex(R.ReleaseError, 'held by another publish'):
                self.publish(path)
        finally:
            os.close(fd)

    def test_existing_journal_dir_refused(self):
        path, _ = self.make_plan()
        self.journal.mkdir()
        with self.assertRaises(FileExistsError):
            self.publish(path)


class ContainmentTests(PublishFixture):
    """Escapes out of the private root through journal paths or substituted parents. The OUTSIDE sentinel folder is a
    sibling of the private root inside the test root; its files are hard links to installed inodes so that a tool which
    trusts identities reached through an escaped path would act on it."""

    def setUp(self):
        super().setUp()
        self.outside = self.root / 'outside'
        self.outside.mkdir(mode=0o700)

    def committed(self):
        path, _ = self.make_plan()
        res = self.publish(path)
        self.assertEqual(res['state'], 'committed', res.get('error'))
        return path

    def edit_journal(self, fn):
        p = self.journal / 'journal.json'
        data = json.loads(p.read_text())
        fn(data)
        p.chmod(0o600)
        p.write_text(json.dumps(data))

    def rollback_outcome(self, journal=None):
        """Rollback, returning the refusal (or None); callers check OUTSIDE first, then that it was refused."""
        try:
            R.rollback(journal or self.journal, self.lock)
        except R.ReleaseError as e:
            return e
        return None

    def sentinel_intact(self, name, ino, content):
        st = os.lstat(self.outside / name)
        self.assertEqual((st.st_ino, (self.outside / name).read_bytes()), (ino, content), f'OUTSIDE/{name} was modified')

    def test_rollback_refuses_dotdot_journal_path(self):
        self.committed()
        os.link(self.dest / 'decbar', self.outside / 'decbar')
        ino = os.lstat(self.outside / 'decbar').st_ino
        escaped = f'{self.dest}/../../outside/decbar'

        def edit(d):
            next(f for f in d['files'] if f['name'] == 'decbar')['path'] = escaped
        self.edit_journal(edit)
        refused = self.rollback_outcome()
        self.sentinel_intact('decbar', ino, b'#!/bin/sh\necho bar-new\n')
        self.assertIsInstance(refused, R.ReleaseError)

    def test_rollback_refuses_staged_name_pointing_outside(self):
        path, _ = self.make_plan()
        sentinel = self.outside / 'sentinel'
        sentinel.write_bytes(b'keep me')

        def die(index, **_):
            if index == 2:
                raise KeyboardInterrupt('simulated death while staging decwm')
        R.FAULTS['stage_write'] = die
        with self.assertRaises(KeyboardInterrupt):
            self.publish(path)
        R.FAULTS.clear()

        def edit(d):
            next(f for f in d['files'] if f['name'] == 'decwm')['staged'] = str(sentinel)
        self.edit_journal(edit)
        refused = self.rollback_outcome()
        self.assertTrue(sentinel.exists(), 'OUTSIDE/sentinel deleted through the journal staged path')
        self.assertIsInstance(refused, R.ReleaseError)

    def test_rollback_refuses_unsafe_journal_names(self):
        self.committed()
        sentinel = self.outside / 'decbar'
        sentinel.write_bytes(b'keep me')
        for key in ('staged', 'restore', 'backup'):
            for bad in ('../../outside/decbar', str(sentinel), '.', 'sub/name'):
                def edit(d, key=key, bad=bad):
                    next(f for f in d['files'] if f['name'] == 'decbar')[key] = bad
                self.edit_journal(edit)
                with self.assertRaisesRegex(R.ReleaseError, 'not a single safe file name', msg=f'{key}={bad}'):
                    R.rollback(self.journal, self.lock)
        self.assertEqual(sentinel.read_bytes(), b'keep me')

    def test_rollback_refuses_dotdot_journal_dir(self):
        self.committed()
        with self.assertRaises(R.ReleaseError):
            R.rollback(f'{self.root}/outside/../journal', self.lock)

    def test_rollback_refuses_parent_replaced_by_symlink_to_outside(self):
        self.committed()
        os.rename(self.dest, self.priv / 'bin.real')
        os.link(self.priv / 'bin.real/decbar', self.outside / 'decbar')
        os.link(self.priv / 'bin.real/decwm', self.outside / 'decwm')
        inos = {n: os.lstat(self.outside / n).st_ino for n in ('decbar', 'decwm')}
        os.symlink(self.outside, self.dest)
        refused = self.rollback_outcome()
        self.assertIsInstance(refused if (self.outside / 'decbar').read_bytes() != OLD_BAR else None, R.ReleaseError,
                              'OUTSIDE/decbar was restored to the old binary through the symlinked parent')
        self.sentinel_intact('decbar', inos['decbar'], b'#!/bin/sh\necho bar-new\n')
        self.sentinel_intact('decwm', inos['decwm'], b'#!/bin/sh\necho wm-new\n')

    def test_parent_swapped_to_outside_symlink_during_publish(self):
        path, _ = self.make_plan()
        moved = self.priv / 'bin.moved'

        def swap(index, path):
            if index == 2:
                os.rename(self.dest, moved)
                for n in ('decbar', 'decwm'):
                    os.link(moved / n, self.outside / n)
                os.symlink(self.outside, self.dest)
        R.FAULTS['before_commit'] = swap
        try:
            res = self.publish(path)
        except (R.ReleaseError, OSError):
            res = None
        inos = {n: os.lstat(self.outside / n).st_ino for n in ('decbar', 'decwm')}
        self.assertEqual(inos, {n: os.lstat(moved / n).st_ino for n in ('decbar', 'decwm')} if res is None else inos)
        self.assertEqual((self.outside / 'decwm').read_bytes(), OLD_WM, 'OUTSIDE/decwm written through the symlink')
        self.assertEqual(sorted(os.listdir(self.outside)), ['decbar', 'decwm'], 'nothing staged or left in OUTSIDE')
        self.assertIsNotNone(res)
        self.assertEqual(res['state'], 'rolled_back', res)
        self.assertIn('replaced during publish', res['error'])
        self.assertEqual((moved / 'decbar').read_bytes(), OLD_BAR, 'the pinned (moved) folder is what gets rolled back')

    def test_create_rollback_race_after_precheck_keeps_external_file(self):
        os.unlink(self.dest / 'decwm')
        self.committed()
        ext = self.root / 'external'
        target = self.dest / 'decwm'
        armed = {'on': True}

        def replace_now():
            if armed['on']:
                armed['on'] = False
                ext.write_bytes(b'external racer\n')
                os.replace(ext, target)
        real_hash_path = R.hash_path

        def hash_then_race(p, limit):          # the old tool's last check before unlink was hash_path(dest)
            out = real_hash_path(p, limit)
            if str(p) == str(target):
                replace_now()
            return out
        R.hash_path = hash_then_race
        self.addCleanup(setattr, R, 'hash_path', real_hash_path)
        R.FAULTS['rollback_prechecked'] = lambda index, name, **_: replace_now() if name == 'decwm' else None
        res = R.rollback(self.journal, self.lock)
        self.assertFalse(armed['on'], 'race was injected')
        self.assertEqual(target.read_bytes(), b'external racer\n', 'external replacement deleted by rollback')
        _, wm = self.journal_file('decwm')
        self.assertEqual(wm['state'], 'rollback_conflict')
        self.assertEqual(res['state'], 'rollback_incomplete')
        self.assertEqual((self.dest / 'decbar').read_bytes(), OLD_BAR)

    def test_replace_rollback_race_after_precheck_keeps_external_file(self):
        self.committed()
        target = self.dest / 'decbar'

        def race(index, name, **_):
            if name == 'decbar':
                ext = self.root / 'external'
                ext.write_bytes(b'external racer\n')
                os.replace(ext, target)
        R.FAULTS['rollback_prechecked'] = race
        res = R.rollback(self.journal, self.lock)
        self.assertEqual(target.read_bytes(), b'external racer\n')
        self.assertEqual(res['state'], 'rollback_incomplete')
        self.assertEqual(sorted(n for n in os.listdir(self.dest) if n.startswith('.')), [], 'no claim/restore leftovers')

    def test_restore_name_collision_preserves_unowned_entry(self):
        self.committed()
        planted = []
        def plant(index, backup):
            _, f = self.journal_file('decbar')
            if f.get('restore') and not planted:
                p = self.dest / f['restore']
                p.write_bytes(b'foreign restore collision\n')
                planted.append(p)
        R.FAULTS['rollback_restore'] = plant
        res = R.rollback(self.journal, self.lock)
        self.assertEqual(len(planted), 1)
        self.assertEqual(planted[0].read_bytes(), b'foreign restore collision\n')
        self.assertEqual(res['state'], 'rollback_incomplete')

    def test_restore_replacement_keeps_foreign_content_and_timestamp(self):
        self.committed()
        real_copy = R._copy_verified
        replaced = []
        def copying(sdfd, sname, ddfd, dname, *args, **kwargs):
            callback = kwargs.get('on_create')
            if 'restore' in dname and not replaced:
                def race(st):
                    if callback:
                        callback(st)
                    p = self.dest / dname
                    temp = self.root / 'foreign-restore'
                    temp.write_bytes(b'foreign restore replacement\n')
                    os.utime(temp, ns=(123456789000, 123456789000))
                    os.replace(temp, p)
                    replaced.append(p)
                kwargs['on_create'] = race
            return real_copy(sdfd, sname, ddfd, dname, *args, **kwargs)
        R._copy_verified = copying
        self.addCleanup(setattr, R, '_copy_verified', real_copy)
        res = R.rollback(self.journal, self.lock)
        self.assertEqual(len(replaced), 1)
        self.assertEqual(replaced[0].read_bytes(), b'foreign restore replacement\n')
        self.assertEqual(replaced[0].stat().st_mtime_ns, 123456789000)
        self.assertEqual(res['state'], 'rollback_incomplete')

    def test_rollback_refuses_live_private_root(self):
        self.committed()
        self.edit_journal(lambda d: d.update(private_root=str(R.LIVE_DIRS[0].parent)))
        with self.assertRaisesRegex(R.ReleaseError, 'live'):
            R.rollback(self.journal, self.lock)


class ManagedRuntimeTests(Base):
    def setUp(self):
        super().setUp()
        self.home = self.root / 'home'
        self.home.mkdir(mode=0o700)
        for key, val in (
            ('HOME', self.home),
            ('LIVE_DIRS', tuple(self.home / f'.local/share/{n}/bin' for n in ('rdwm-decebal', 'rdwm-macshell'))),
            ('LIVE_FORBIDDEN_ROOTS', tuple(self.home / p for p in ('.local/share/rdwm-decebal/bin', '.local/share/rdwm-macshell/bin', '.local/bin', '.config')) + (Path('/usr'), Path('/etc'), Path('/opt'))),
            ('CONFIG_WATCH', (self.home / '.config/rdwm/config.toml',)),
        ):
            self.addCleanup(setattr, R, key, getattr(R, key))
            setattr(R, key, val)
        R.CONFIG_WATCH[0].parent.mkdir(parents=True)
        R.CONFIG_WATCH[0].write_text('config')
        self.dests = list(R.LIVE_DIRS)
        for d in self.dests:
            d.mkdir(parents=True)
            (d / 'decbar').write_bytes(b'old')
            (d / 'decbar').chmod(0o755)
            (d / 'rdwm').write_bytes(b'foreign')
        self.art = make_bundle(self.root, {n: b'new-' + n.encode() for n in R.SHIPPED_BINARIES | {'examples/launchapp'}})
        self.priv = self.root / 'private'
        self.priv.mkdir(mode=0o700)
        self.journal = self.priv / 'journal'
        self.lock = self.priv / 'lock'

    def make_plan(self, **kw):
        opts = dict(only=['decbar'], proc=self.root / 'noproc', runtime_dir=self.root / 'run', managed_runtime=True)
        opts.update(kw)
        p = R.plan(self.art, list(map(str, self.dests)), **opts)
        self.plan_path = self.root / 'plan.json'
        self.plan_path.write_text(json.dumps(p))
        return p

    def publish(self, **kw):
        return R.publish(self.plan_path, self.priv, self.journal, self.lock, managed_runtime=True, **kw)

    def edit_plan(self, p):
        p['plan_sha256'] = R._canon_sha(p)
        self.plan_path.write_text(json.dumps(p))

    def test_publish_and_explicit_rollback_preserve_foreign_entries(self):
        self.make_plan()
        busy = R._busy
        R._busy = lambda *a: self.fail('managed publish must not require idle processes')
        self.addCleanup(setattr, R, '_busy', busy)
        self.assertEqual(self.publish()['state'], 'committed')
        for d in self.dests:
            self.assertEqual((d / 'decbar').read_bytes(), b'new-decbar')
            self.assertEqual((d / 'rdwm').read_bytes(), b'foreign')
        with self.assertRaisesRegex(R.ReleaseError, 'explicit'):
            R.rollback(self.journal, self.lock)
        self.assertEqual(R.rollback(self.journal, self.lock, managed_runtime=True)['state'], 'rolled_back')
        for d in self.dests:
            self.assertEqual((d / 'decbar').read_bytes(), b'old')

    def test_open_old_inode_keeps_content_and_folder_during_atomic_publish(self):
        self.make_plan()
        fd = os.open(self.dests[0] / 'decbar', os.O_RDONLY)
        try:
            old = os.fstat(fd)
            self.assertEqual(self.publish()['state'], 'committed')
            self.assertEqual(os.read(fd, 100), b'old')
            self.assertEqual(os.fstat(fd).st_ino, old.st_ino)
            self.assertEqual(Path(os.readlink(f'/proc/self/fd/{fd}')).parent, self.dests[0])
        finally:
            os.close(fd)

    def test_failure_rolls_back_both_managed_folders(self):
        self.make_plan()
        def fail(index, **kw):
            if index == 1:
                raise R.ReleaseError('fixture failure')
        R.FAULTS['after_commit'] = fail
        self.assertEqual(self.publish()['state'], 'rolled_back')
        for d in self.dests:
            self.assertEqual((d / 'decbar').read_bytes(), b'old')

    def test_default_publish_still_refuses_live_and_managed_plan(self):
        self.make_plan()
        with self.assertRaisesRegex(R.ReleaseError, 'explicit'):
            R.publish(self.plan_path, self.priv, self.journal, self.lock)
        with self.assertRaises(R.ReleaseError):
            R._private_root_check(self.home)

    def test_allowlist_exact_and_symlink_free(self):
        self.make_plan()
        for d in (self.home, self.home / '.config', self.dests[0].parent, self.dests[0] / 'nested', self.home / '.local/bin'):
            with self.assertRaises(R.ReleaseError):
                R._managed_dest(d)
        self.dests[0].rename(self.dests[0].with_name('old-bin'))
        self.dests[0].symlink_to(self.dests[0].with_name('old-bin'))
        with self.assertRaises(R.ReleaseError):
            self.publish()

    def test_nonstandard_manifest_and_qa_name_refused(self):
        with self.assertRaises(R.ReleaseError):
            self.make_plan(only=['examples/launchapp'])
        other = self.root / 'other'
        other.mkdir()
        self.art = make_bundle(other)
        with self.assertRaisesRegex(R.ReleaseError, 'standard'):
            self.make_plan()

    def test_stale_plan_refused(self):
        p = self.make_plan()
        p['created_utc'] = '2000-01-01T00:00:00Z'
        self.edit_plan(p)
        with self.assertRaisesRegex(R.ReleaseError, 'stale'):
            self.publish()
        self.assertFalse(self.journal.exists())

    def test_config_same_content_replaced_inode_refused(self):
        self.make_plan()
        cfg = R.CONFIG_WATCH[0]
        replacement = cfg.with_suffix('.new')
        replacement.write_bytes(cfg.read_bytes())
        replacement.replace(cfg)
        with self.assertRaisesRegex(R.ReleaseError, 'config identity'):
            self.publish()

    def test_destination_replaced_inode_refused(self):
        self.make_plan()
        self.dests[0].rename(self.dests[0].with_name('old-bin'))
        self.dests[0].mkdir()
        with self.assertRaisesRegex(R.ReleaseError, 'identity changed'):
            self.publish()

    def test_stale_file_refused_before_changes(self):
        self.make_plan()
        (self.dests[1] / 'decbar').write_bytes(b'foreign-change')
        self.assertEqual(self.publish()['state'], 'aborted_before_changes')
        self.assertEqual((self.dests[0] / 'decbar').read_bytes(), b'old')
        self.assertEqual((self.dests[1] / 'decbar').read_bytes(), b'foreign-change')

    def test_modified_foreign_file_survives_rollback(self):
        self.make_plan()
        self.publish()
        (self.dests[0] / 'decbar').write_bytes(b'foreign-change')
        res = R.rollback(self.journal, self.lock, managed_runtime=True)
        self.assertEqual(res['state'], 'rollback_incomplete')
        self.assertEqual((self.dests[0] / 'decbar').read_bytes(), b'foreign-change')

    def test_forged_journal_destination_flag_cannot_authorize_arbitrary_path(self):
        self.make_plan()
        self.publish()
        path = self.journal / 'journal.json'
        data = json.loads(path.read_text())
        data['dest_dirs'][str(self.home / '.config')] = next(iter(data['dest_dirs'].values()))
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(R.ReleaseError, 'exact managed'):
            R.rollback(self.journal, self.lock, managed_runtime=True)

    def test_journal_and_lock_must_be_inside_private_root(self):
        self.make_plan()
        self.lock = self.home / 'forbidden-lock'
        with self.assertRaisesRegex(R.ReleaseError, 'private root'):
            self.publish()
        self.assertFalse(self.lock.exists())


class RunExactTests(Base):
    def script(self, body):
        p = self.root / 'tool'
        p.write_text('#!/bin/sh\n' + body)
        p.chmod(0o555)
        return p, sha(p.read_bytes())

    def test_runs_verified_inode_with_explicit_env(self):
        p, d = self.script('echo "n=$(env | grep -vc -e ^PWD= -e ^SHLVL= -e ^_=) a=$A"\n')
        res = R.run_exact(p, [], d, {'A': 'secret-value', 'PATH': '/usr/bin:/bin'}, timeout=10)
        self.assertEqual(res['returncode'], 0, res)
        self.assertEqual(res['stdout'].strip(), 'n=2 a=secret-value')
        self.assertEqual(res['env_keys'], ['A', 'PATH'])
        self.assertNotIn('secret-value', json.dumps({k: v for k, v in res.items() if k not in ('stdout',)}))

    def test_digest_mismatch_not_executed(self):
        p, _ = self.script(f'touch {self.root}/ran\n')
        with self.assertRaisesRegex(R.ReleaseError, 'not executed'):
            R.run_exact(p, [], '0' * 64, {'PATH': '/usr/bin:/bin'}, timeout=5)
        self.assertFalse((self.root / 'ran').exists())
        with self.assertRaisesRegex(R.ReleaseError, 'explicit environment'):
            R.run_exact(p, [], '0' * 64, None, timeout=5)

    def test_timeout_kills_only_its_own_group(self):
        p, d = self.script(f'sleep 30 & echo $! > {self.root}/grandchild; wait\n')
        res = R.run_exact(p, [], d, {'PATH': '/usr/bin:/bin'}, timeout=1)
        self.assertTrue(res['timed_out'])
        self.assertLess(res['wall_s'], 10)
        gc = int((self.root / 'grandchild').read_text())
        for _ in range(50):
            try:
                os.kill(gc, 0)
            except ProcessLookupError:
                break
            import time
            time.sleep(0.05)
        else:
            self.fail('grandchild in the helper-created group survived')


if __name__ == '__main__':
    unittest.main(verbosity=2)
