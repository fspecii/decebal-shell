#!/usr/bin/env python3
"""Pure-Python checks of the helpers in widget-creator-native.py. No X server, no Settings, no cargo: it only exercises argument validation, the config/TOML
verifiers, the token scan, pidfd shutdown (real throwaway python children) and the signal-to-cleanup path.

  python3 -I tests/e2e/widget-creator-native-test.py
"""
import importlib.util
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("widget_creator_native", HERE / "widget-creator-native.py")
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)

CLOCK = '''# Created with Decebal Settings > Widgets > Create widget (clock).
format = 1
id = "desk-clock"
name = "Desk Clock"
description = "Digital clock created in Decebal Settings."

[root]
type = "col"
width = 236

[[root.children]]
type = "text"
text = "{clock:%I:%M %p}"
'''
NEW = {"use": "desk-clock", "anchor": "top-right", "x": -40, "y": 60}


def item(use="desk-clock", **extra):
    return {"use": use, "place": "desktop", "anchor": "top-right", "x": -40, "y": 60, "monitor": "primary", **extra}


class Arguments(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="wcn-")
        self.root = Path(self.tmp.name)
        self.bins = self.root / "bin"
        self.bins.mkdir(mode=0o755)
        for name in gate.REQUIRED_BINS:
            (self.bins / name).write_text("#!/bin/sh\n")
            (self.bins / name).chmod(0o755)

    def tearDown(self):
        self.tmp.cleanup()

    def test_valid_directory(self):
        found = gate.validate_bin_dir(str(self.bins), self.root / "out")
        self.assertEqual(sorted(found), ["decsettings", "decwidgets"])

    def test_rejections(self):
        with self.assertRaises(AssertionError):
            gate.validate_bin_dir("relative/bin")
        link = self.root / "link"
        link.symlink_to(self.bins)
        with self.assertRaises(AssertionError):
            gate.validate_bin_dir(str(link))
        (self.bins / "decwidgets").unlink()
        with self.assertRaises(AssertionError):
            gate.validate_bin_dir(str(self.bins))
        (self.bins / "decwidgets").symlink_to(self.bins / "decsettings")
        with self.assertRaises(AssertionError):
            gate.validate_bin_dir(str(self.bins))
        (self.bins / "decwidgets").unlink()
        (self.bins / "decwidgets").write_text("x")
        with self.assertRaises(AssertionError):      # not executable
            gate.validate_bin_dir(str(self.bins))
        (self.bins / "decwidgets").chmod(0o755)
        self.bins.chmod(0o775)
        with self.assertRaises(AssertionError):      # group writable: not immutable
            gate.validate_bin_dir(str(self.bins))
        self.bins.chmod(0o755)
        with self.assertRaises(AssertionError):      # the evidence directory may not sit in the binaries
            gate.validate_bin_dir(str(self.bins), self.bins / "out")
        with self.assertRaises(AssertionError):      # an extra binary that is not there
            gate.validate_bin_dir(str(self.bins), None, ["nothing"])

    def test_fresh_out(self):
        out = self.root / "o"
        made = gate.fresh_out(str(out))
        self.assertTrue(made.is_dir())
        with self.assertRaises(AssertionError):      # never reused
            gate.fresh_out(str(out))
        with self.assertRaises(AssertionError):
            gate.fresh_out("relative")
        with self.assertRaises(AssertionError):
            gate.fresh_out(str(self.root / "missing-parent" / "o"))
        with self.assertRaises(AssertionError):      # socket path too long
            gate.fresh_out(str(self.root / ("x" * 80)))
        self.assertTrue(str(made).startswith("/"))


class Verifiers(unittest.TestCase):
    def before(self):
        return gate.initial_config()

    def after(self, *added):
        doc = gate.initial_config()
        doc["widgets"]["items"] += list(added)
        doc["widgets"]["enabled"] = True
        return doc

    def test_good_config_passes(self):
        gate.verify_config(self.before(), self.after(item()), [NEW])

    def test_each_regression_is_caught(self):
        def bad(mutate, cards=(NEW,), added=(item(),)):
            doc = self.after(*added)
            mutate(doc)
            with self.assertRaises(AssertionError):
                gate.verify_config(self.before(), doc, list(cards))
        bad(lambda d: d["widgets"]["items"][0].pop("future_item"))
        bad(lambda d: d["widgets"]["items"][0]["options"].update(sentinel="lost"))
        bad(lambda d: d["widgets"]["items"][1].update(enabled=True))
        bad(lambda d: d["widgets"].update(future=None))
        bad(lambda d: d["widgets"].update(enabled=False))
        bad(lambda d: d["widgets"].update(allow_exec=True))
        bad(lambda d: d.pop("zz_unknown_top"))
        bad(lambda d: d["dock"].update(size=1))
        bad(lambda d: d["widgets"]["items"].pop())
        bad(lambda d: None, added=(item("other"),))                         # wrong card id
        bad(lambda d: None, added=(item(x=5),))                              # not where the form said
        bad(lambda d: None, added=(item(enabled=False),))                    # saved disabled
        bad(lambda d: None, added=(item(exec="rm -rf"),))                    # unexpected keys
        bad(lambda d: None, added=(item(), item("again")))                   # an extra card

    def test_toml(self):
        data = gate.check_widget_toml(CLOCK, "desk-clock", "Desk Clock", have=("{clock:%I:%M %p}",), lack=("%A",), widths=(236.0,))
        self.assertEqual(data["id"], "desk-clock")
        for kwargs in ({"stem": "other"}, {"name": "Other"}):
            with self.assertRaises(AssertionError):
                gate.check_widget_toml(CLOCK, kwargs.get("stem", "desk-clock"), kwargs.get("name", "Desk Clock"))
        with self.assertRaises(AssertionError):
            gate.check_widget_toml(CLOCK, "desk-clock", "Desk Clock", have=("%A",))
        with self.assertRaises(AssertionError):
            gate.check_widget_toml(CLOCK, "desk-clock", "Desk Clock", widths=(338.0,))
        with self.assertRaises(AssertionError):      # a widget that runs commands
            gate.check_widget_toml(CLOCK + '\n[[root.children]]\ntype = "button"\nexec = "x"\n', "desk-clock", "Desk Clock")

    def test_initial_config_is_json_stable(self):
        doc = gate.initial_config()
        self.assertEqual(json.loads(json.dumps(doc)), doc)
        self.assertFalse(doc["widgets"]["enabled"])


class Processes(unittest.TestCase):
    def test_scan_token_in_a_fake_proc(self):
        with tempfile.TemporaryDirectory(prefix="wcn-proc-") as tmp:
            proc = Path(tmp)
            for pid, env in ((101, b"A=1\0DECEBAL_NATIVE_TOKEN=abc\0"), (102, b"DECEBAL_NATIVE_TOKEN=abcd\0"), (103, b"DECEBAL_NATIVE_TOKEN=abc\0"), (104, b"")):
                (proc / str(pid)).mkdir()
                (proc / str(pid) / "environ").write_bytes(env)
            (proc / "self").mkdir()
            self.assertEqual(gate.scan_token("abc", proc=str(proc)), [101, 103])
            self.assertEqual(gate.scan_token("abc", proc=str(proc), exclude=(101,)), [103])
            self.assertEqual(gate.scan_token("abc", proc=str(proc), uid=os.getuid() + 1), [])

    def child(self, ignore_term):
        code = "import signal,time\n" + ("signal.signal(signal.SIGTERM, signal.SIG_IGN)\n" if ignore_term else "") + "print('up', flush=True)\ntime.sleep(60)\n"
        proc = subprocess.Popen([sys.executable, "-I", "-c", code], stdout=subprocess.PIPE, text=True, start_new_session=True)
        self.assertEqual(proc.stdout.readline().strip(), "up")
        proc.stdout.close()
        self.addCleanup(lambda: proc.poll() is None and (proc.kill(), proc.wait()))
        return proc

    def test_pidfd_stop_graceful_forced_and_gone(self):
        proc = self.child(False)
        self.assertEqual(gate.stop_pid(proc.pid, grace=3), "term")
        self.assertEqual(proc.wait(timeout=3), -signal.SIGTERM)
        self.assertEqual(gate.stop_pid(proc.pid, grace=1), "gone")   # reaped: the pid is gone
        stubborn = self.child(True)
        self.assertEqual(gate.stop_pid(stubborn.pid, grace=0.5), "forced")   # a SIGKILL is reported, so the run fails
        self.assertEqual(stubborn.wait(timeout=3), -signal.SIGKILL)

    def test_verify_refuses_the_wrong_process(self):
        proc = self.child(False)
        try:
            with self.assertRaises(AssertionError):
                gate.stop_pid(proc.pid, grace=1, verify=lambda pid: False)
            self.assertIsNone(proc.poll(), "an unverified process must not be signalled")
        finally:
            gate.stop_pid(proc.pid, grace=3)
            proc.wait(timeout=3)

    def test_ledger(self):
        ledger = gate.Ledger()
        self.assertEqual(ledger.problems(), [])
        ledger.forced.append("x")
        ledger.leftovers.append(3)
        self.assertEqual(len(ledger.problems()), 2)

    def test_sigterm_unwinds_through_finally(self):
        """The handler turns SIGTERM/SIGINT into an exception so the single cleanup path runs."""
        ran = []

        def handler(signum, frame):
            raise gate.Terminated(signum)
        old = {s: signal.signal(s, handler) for s in (signal.SIGTERM, signal.SIGINT)}
        try:
            for sig in (signal.SIGTERM, signal.SIGINT):
                try:
                    try:
                        os.kill(os.getpid(), sig)
                        time.sleep(1)
                    finally:
                        ran.append(sig)
                except gate.Terminated as stop:
                    self.assertEqual(stop.signum, sig)
        finally:
            for s, h in old.items():
                signal.signal(s, h)
        self.assertEqual(ran, [signal.SIGTERM, signal.SIGINT])


class Permissions(unittest.TestCase):
    def test_unwritable_directory_denies_new_files_but_keeps_reads(self):
        if os.getuid() == 0:
            self.skipTest("root ignores permission bits; the native gate refuses to run as root")
        with tempfile.TemporaryDirectory(prefix="wcn-perm-") as tmp:
            cfg, widgets = Path(tmp) / "cfg", Path(tmp) / "widgets"
            cfg.mkdir()
            widgets.mkdir()
            (cfg / "decebal.json").write_text("{}")
            cfg.chmod(0o500)
            try:
                with self.assertRaises(PermissionError):
                    (cfg / "new").touch(exist_ok=False)
                self.assertEqual((cfg / "decebal.json").read_text(), "{}")
                self.assertTrue(os.access(widgets, os.W_OK))
                (widgets / "ok.toml").write_text("x")
            finally:
                cfg.chmod(0o700)


class Isolation(unittest.TestCase):
    def test_environment_is_built_from_nothing(self):
        private = Path("/var/tmp/wcn-env")
        env = gate.base_env(private, "tok")
        for key in ("DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY", "RDWM_SOCK", "SESSION_MANAGER", "XDG_SESSION_ID"):
            self.assertNotIn(key, env)
        self.assertEqual(env["PATH"], str(private / "bin"))
        self.assertEqual(env["DECEBAL_NATIVE_TOKEN"], "tok")
        self.assertTrue(env["DBUS_SESSION_BUS_ADDRESS"].startswith(f"unix:path={private}/"))
        self.assertTrue(env["DBUS_SYSTEM_BUS_ADDRESS"].startswith(f"unix:path={private}/"))
        for key, value in env.items():
            if key not in ("LANG", "DECSETTINGS_SCALE", "DECEBAL_BACKEND", "DECEBAL_SYSTEM_AUTOMATIC", TOKEN):
                self.assertIn(str(private), value, key)


class ReviewedRegressions(unittest.TestCase):
    def test_preview_comparison_handles_different_template_heights(self):
        from PIL import Image
        a = Image.new("RGB", (100, 120), "black")
        b = Image.new("RGB", (100, 60), "black")
        self.assertEqual(gate.changed_area(a, b), 0)
        b.paste("white", (10, 10, 30, 30))
        self.assertEqual(gate.changed_area(a, b), 400)

    def test_preview_crop_excludes_page_and_creator_headings(self):
        from PIL import Image
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'shot.png'
            image = Image.new('RGB', (940, 680), 'black')
            for y in range(275, 286):
                image.paste((y - 250, y - 200, y - 150), (270, y, 420, y + 1))
            image.save(path)
            g = object.__new__(gate.Gate)
            g.layout = lambda: {'panes': [{'rect': [12, 64, 212, 32]}]}
            g.ctl = lambda cid: {'rect': [850, 207, 40, 22] if cid == 'wg:enabled' else [694, 498, 200, 26]}
            g.window_width, g.window_height = lambda: 940, lambda: 680
            g.shot = lambda name: path
            self.assertFalse(gate.region_has_content(g.preview_shot('header-only')))
            for y in range(345, 365):
                image.paste((y - 300, y - 250, y - 200), (520, y, 650, y + 1))
            image.save(path)
            self.assertTrue(gate.region_has_content(g.preview_shot('real-preview-ink')))

    def test_cleanup_skips_server_and_continues_after_host_verification_error(self):
        from unittest.mock import Mock, patch
        g = object.__new__(gate.Gate)
        g.settings, g.server = Mock(pid=10), Mock(pid=20)
        g.private, g.token = Path("/private"), "private-token"
        g.x11, g.logs, g.ledger = Mock(), [], gate.Ledger()
        g.stop_popen = Mock()
        g.host_pids = Mock(return_value=[20, 30, 40])
        with patch.object(gate, "scan_token", return_value=[]), \
             patch.object(gate, "exe_of", return_value="/private/bin/decwidgets"), \
             patch.object(gate, "stop_pid", side_effect=[AssertionError("identity changed"), "stopped"]) as stop:
            g.close()
        self.assertEqual([c.args[0] for c in stop.call_args_list], [30, 40])
        self.assertEqual([c.args[0] for c in g.stop_popen.call_args_list], ["decsettings", "Xvfb"])
        g.x11.close.assert_called_once_with(g.server)
        self.assertTrue(any("identity changed" in e for e in g.ledger.unexpected))
        self.assertEqual(g.ledger.leftovers, [])


TOKEN = gate.TOKEN_VAR

if __name__ == "__main__":
    unittest.main()
