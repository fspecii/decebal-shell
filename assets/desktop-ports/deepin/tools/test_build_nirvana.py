#!/usr/bin/env python3
"""Self-test of the DCI reader in build_nirvana.py (stdlib only, no ImageMagick, no network, no upstream checkout).

    python3 -I test_build_nirvana.py
"""
import importlib.util
import os
import struct
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True  # never leave a __pycache__ in the asset folder

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("build_nirvana", os.path.join(HERE, "build_nirvana.py"))
B = importlib.util.module_from_spec(spec)
spec.loader.exec_module(B)


def entry(kind, name, content):
    return bytes([kind]) + name.encode().ljust(63, b"\0") + struct.pack("<Q", len(content)) + content


def webp(extra=16):
    body = b"WEBP" + b"\0" * extra
    return b"RIFF" + struct.pack("<I", len(body)) + body


def dci(*root, count=None, version=1, magic=b"DCI\0"):
    return magic[:3] + magic[3:4] + bytes([version]) + (len(root) if count is None else count).to_bytes(3, "little") + b"".join(root)


def tree(*parts, leaf):
    """entry chain a/b/c/leaf"""
    inner = leaf
    for name in reversed(parts):
        inner = entry(2, name, inner)
    return inner


GOOD_LAYER = entry(1, "1.webp", webp())


class Reader(unittest.TestCase):
    def good(self):
        return dci(tree("256", "normal.light", "3", leaf=GOOD_LAYER))

    def test_reads_the_one_plain_layer(self):
        layer, why = B.best_layer(B.load_dci(self.good()))
        self.assertIsNone(why)
        self.assertEqual(layer[:3], (256, 3, "1.webp"))

    def test_a_link_to_the_other_tone_is_ignored(self):
        data = dci(entry(2, "256", entry(2, "normal.dark", entry(2, "3", entry(3, "1.webp", b"/256/normal.light/3/1.webp"))) + entry(2, "normal.light", entry(2, "3", GOOD_LAYER))))
        layer, _ = B.best_layer(B.load_dci(data))
        self.assertEqual(layer[2], "1.webp")

    def test_truncated_file_is_refused_with_the_numbers(self):
        data = self.good()[:-20]
        with self.assertRaises(B.Malformed) as cm:
            B.load_dci(data)
        self.assertIn("declares", str(cm.exception))
        self.assertIn("remain", str(cm.exception))

    def test_bad_magic_version_and_count(self):
        for data, word in [(b"XXX\0" + self.good()[4:], "magic"), (self.good()[:4] + bytes([2]) + self.good()[5:], "version"), (dci(GOOD_LAYER, count=2), "header says")]:
            with self.assertRaises(B.Malformed) as cm:
                B.load_dci(data)
            self.assertIn(word, str(cm.exception))

    def test_names_must_be_terminated_utf8_without_slash_or_controls(self):
        for bad in [bytes([1]) + b"a" * 63 + struct.pack("<Q", 0), bytes([1]) + b"\xff\xfe".ljust(63, b"\0") + struct.pack("<Q", 0), entry(1, "a/b", b""), entry(1, "a\x01b", b"")]:
            with self.assertRaises(B.Malformed):
                B.load_dci(dci(bad))

    def test_depth_and_entry_count_limits(self):
        deep = GOOD_LAYER
        for _ in range(B.MAX_DEPTH + 2):
            deep = entry(2, "d", deep)
        with self.assertRaises(B.Malformed):
            B.load_dci(dci(deep))
        many = [entry(1, f"{i}.webp", b"") for i in range(B.MAX_ENTRIES_PER_LEVEL + 1)]
        with self.assertRaises(B.Malformed):
            B.load_dci(dci(entry(2, "256", b"".join(many))))

    def test_size_limit_before_parsing(self):
        with self.assertRaises(B.Malformed):
            B.load_dci(b"DCI\0\1\1\0\0" + b"\0" * B.MAX_DCI_BYTES)

    def test_layers_that_cannot_be_flattened_are_listed_not_guessed(self):
        two = dci(tree("256", "normal.light", "3", leaf=entry(1, "1.webp", webp()) + entry(1, "2.webp", webp())))
        self.assertIn("2 layers", B.best_layer(B.load_dci(two))[1])
        alpha = dci(tree("256", "normal.light", "3", leaf=entry(1, "1.7p.3.png.alpha8", b"x")))
        self.assertIn("not a plain", B.best_layer(B.load_dci(alpha))[1])
        png = dci(tree("256", "normal.light", "3", leaf=entry(1, "1.webp", b"\x89PNG" + b"\0" * 20)))
        self.assertIn("not a WebP", B.best_layer(B.load_dci(png))[1])
        bad_riff = dci(tree("256", "normal.light", "3", leaf=entry(1, "1.webp", b"RIFF" + struct.pack("<I", 999) + b"WEBP" + b"\0" * 8)))
        self.assertIn("RIFF size", B.best_layer(B.load_dci(bad_riff))[1])
        dark_only = dci(tree("256", "normal.dark", "3", leaf=GOOD_LAYER))
        self.assertIn("no normal.light", B.best_layer(B.load_dci(dark_only))[1])

    def test_a_plain_layer_is_exactly_prior_dot_webp(self):
        for name in ["1.0.webp", "1.7p.3.webp", "2.3.webp.alpha8", "1.png", "0.webp", "01.webp", "a.webp", "1.webp.png", "1..webp", ".webp", "1.WEBP", "1000.webp"]:
            data = dci(tree("256", "normal.light", "3", leaf=entry(1, name, webp())))
            layer, why = B.best_layer(B.load_dci(data))
            self.assertIsNone(layer, name)
            self.assertIn("plain", why, name)
        for name in ["1.webp", "9.webp", "12.webp", "999.webp"]:
            data = dci(tree("256", "normal.light", "3", leaf=entry(1, name, webp())))
            self.assertIsNotNone(B.best_layer(B.load_dci(data))[0], name)

    def test_malformed_or_out_of_range_size_and_scale_directories_are_refusals_not_crashes(self):
        for size, scale in [("abc", "3"), ("256", "x"), ("-1", "3"), ("0", "3"), ("256", "0"), ("256", "9"), ("99999", "3"), ("1e2", "3"), (" 256", "3"), ("256", "٣"), ("256", "03"), ("1025", "3")]:
            data = dci(tree(size, "normal.light", scale, leaf=GOOD_LAYER))
            layer, why = B.best_layer(B.load_dci(data))
            self.assertIsNone(layer, (size, scale))
            self.assertTrue(why, (size, scale))
        # one bad directory beside a good one refuses the icon rather than guessing
        data = dci(entry(2, "256", entry(2, "normal.light", entry(2, "3", GOOD_LAYER))), entry(2, "big", entry(2, "normal.light", entry(2, "3", GOOD_LAYER))))
        self.assertIsNone(B.best_layer(B.load_dci(data))[0])

    def test_icon_names(self):
        for ok in ["dde-file-manager", "org.deepin.browser", "FBReader.png", "a+b"]:
            self.assertTrue(B.ICON_NAME.match(ok), ok)
        for bad in ["", ".hidden", "-x", "a b", "a/b", "é", "x" * 129]:
            self.assertFalse(B.ICON_NAME.match(bad), bad)


GIT = ["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false"]
ENV = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1", "LC_ALL": "C"}


def write(path, data, mode="wb"):
    with open(path, mode) as fh:
        fh.write(data)


def run_git(repo, *args):
    return subprocess.run([*GIT, "-C", repo, *args], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=ENV).stdout.decode()


class Provenance(unittest.TestCase):
    """The input is hashed against the pinned revision's Git objects, in a throwaway repository (no network, no upstream checkout)."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="nirvana-prov-", dir=os.environ.get("TMPDIR") or "/var/tmp")
        self.addCleanup(self.tmp.cleanup)
        self.repo = os.path.join(self.tmp.name, "repo")
        self.src = os.path.join(self.repo, "nirvana", "dsg-icons", "nirvana")
        os.makedirs(self.src)
        os.makedirs(os.path.join(self.repo, "LICENSES"))
        os.makedirs(os.path.join(self.repo, ".reuse"))
        self.a = dci(tree("256", "normal.light", "3", leaf=GOOD_LAYER))
        write(os.path.join(self.src, "a.dci"), self.a)
        os.symlink("a.dci", os.path.join(self.src, "b.dci"))
        write(os.path.join(self.repo, "LICENSES", "CC-BY-4.0.txt"), b"licence text\n")
        write(os.path.join(self.repo, ".reuse", "dep5"), b"Files: nirvana/*\nLicense: CC-BY-4.0\n")
        run_git(self.repo, "init", "-q")
        run_git(self.repo, "add", "-A")
        run_git(self.repo, "commit", "-q", "-m", "x")
        self.rev = run_git(self.repo, "rev-parse", "HEAD").strip()
        B.git_env = lambda: ENV  # the module's own helper, pinned to a config-free environment
        self.pinned = B.Pinned(self.repo, self.rev)

    def check(self, name):
        return B.verify_dci(self.pinned, name, os.path.join(self.src, name), self.src)

    def test_a_clean_tree_verifies_files_and_links_with_their_git_blob(self):
        raw, blob, problems = self.check("a.dci")
        self.assertEqual((problems, blob), ([], B.blob_oid(self.a)))
        raw, blob, problems = self.check("b.dci")
        self.assertEqual((problems, raw, blob), ([], self.a, B.blob_oid(self.a)))
        self.assertEqual(self.pinned.tree, run_git(self.repo, "rev-parse", f"{self.rev}:nirvana/dsg-icons/nirvana").strip())
        self.assertEqual(set(self.pinned.entries), {"a.dci", "b.dci"})
        for rel in B.Pinned.LICENCE_FILES:
            data, problems = B.read_licence(self.repo, self.pinned, rel)
            self.assertEqual(problems, [], rel)
            self.assertEqual(B.blob_oid(data), self.pinned.licences[rel])

    def test_a_modified_tracked_file_is_a_problem_and_has_no_blob(self):
        write(os.path.join(self.src, "a.dci"), b"x", "ab")
        _, blob, problems = self.check("a.dci")
        self.assertIsNone(blob)
        self.assertTrue(any("modified" in p for p in problems), problems)
        _, blob, problems = self.check("b.dci")  # the alias resolves to the modified sibling
        self.assertIsNone(blob)
        self.assertTrue(any("target a.dci" in p for p in problems), problems)

    def test_an_untracked_file_is_reported_and_never_gets_a_blob(self):
        write(os.path.join(self.src, "evil.dci"), self.a)
        _, blob, problems = self.check("evil.dci")
        self.assertIsNone(blob)
        self.assertTrue(any("untracked" in p for p in problems), problems)
        self.assertIn("evil.dci", set(os.listdir(self.src)) - set(self.pinned.entries))

    def test_a_retargeted_or_replaced_link_is_a_problem(self):
        write(os.path.join(self.src, "c.dci"), self.a)
        run_git(self.repo, "add", "-A")
        run_git(self.repo, "commit", "-q", "-m", "c")
        pinned = B.Pinned(self.repo, run_git(self.repo, "rev-parse", "HEAD").strip())
        os.unlink(os.path.join(self.src, "b.dci"))
        os.symlink("c.dci", os.path.join(self.src, "b.dci"))
        _, blob, problems = B.verify_dci(pinned, "b.dci", os.path.join(self.src, "b.dci"), self.src)
        self.assertIsNone(blob)
        self.assertTrue(any("link text" in p for p in problems), problems)
        os.unlink(os.path.join(self.src, "b.dci"))
        write(os.path.join(self.src, "b.dci"), self.a)  # a file where the revision has a link
        _, blob, problems = B.verify_dci(pinned, "b.dci", os.path.join(self.src, "b.dci"), self.src)
        self.assertIsNone(blob)
        self.assertTrue(any("link in the revision" in p or "mode 120000" in p for p in problems), problems)

    def test_a_link_leaving_the_directory_is_refused(self):
        outside = os.path.join(self.tmp.name, "outside.dci")
        write(outside, self.a)
        os.unlink(os.path.join(self.src, "b.dci"))
        os.symlink(outside, os.path.join(self.src, "b.dci"))
        with self.assertRaises(B.Malformed):
            self.check("b.dci")

    def test_a_modified_licence_file_is_a_problem(self):
        write(os.path.join(self.repo, "LICENSES", "CC-BY-4.0.txt"), b"tampered\n")
        _, problems = B.read_licence(self.repo, self.pinned, "LICENSES/CC-BY-4.0.txt")
        self.assertTrue(any("modified" in p for p in problems), problems)
        os.unlink(os.path.join(self.repo, ".reuse", "dep5"))
        _, problems = B.read_licence(self.repo, self.pinned, ".reuse/dep5")
        self.assertTrue(problems)

    def test_the_command_line_refuses_dirty_input_unless_told_and_then_labels_it(self):
        """The whole tool against a throwaway repository whose revision is allowed explicitly (it needs `magick` only to convert, so the
        refusal paths run without it)."""
        tool = os.path.join(HERE, "build_nirvana.py")
        write(os.path.join(self.src, "a.dci"), b"x", "ab")
        r = subprocess.run([sys.executable, "-I", tool, "--upstream", self.repo, "--out", os.path.join(self.tmp.name, "out"), "--allow-other-revision"],
                           env=ENV, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.assertNotEqual(r.returncode, 0)
        self.assertIn(b"differ from", r.stderr)
        self.assertFalse(os.path.exists(os.path.join(self.tmp.name, "out")), "nothing is written when the input is not the revision")


if __name__ == "__main__":
    unittest.main()
