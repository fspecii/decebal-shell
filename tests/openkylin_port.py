#!/usr/bin/env python3
"""Focused checks for the openKylin desktop port (themes/openkylin*, assets/desktop-ports/openkylin, docs).

Usage: python3 tests/openkylin_port.py            (offline; no network, no display)
       DECTHEMECTL=/path/to/decthemectl python3 tests/openkylin_port.py    also validates every package with the real CLI
"""
import hashlib, importlib.util, json, os, pathlib, re, struct, subprocess, sys, tempfile, tomllib, unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
A = ROOT / "assets/desktop-ports/openkylin"
IDS = ["openkylin", "openkylin-islands", "openkylin-light-seeking", "openkylin-heyin", "openkylin-classic"]
# git blob ids of icons/ukui-icon-theme-{default,fashion,classical}/128x128/apps/kylin-startmenu.png at openkylin-theme 3c78d71
_D, _F, _C = "adbe8bd46bcb314dd8eaff1e1ec198409b27b856", "4dfa94880821aa60e3ecd9860eb9a6dbeda431cd", "379a20c9cd81c6e5ff929b6523401e1986c80cb8"
LOGO_BLOBS = {"openkylin": _D, "openkylin-islands": _D, "openkylin-light-seeking": _D, "openkylin-heyin": _F, "openkylin-classic": _C}


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path); m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m); return m


dp = load("derive_palette", A / "tools/derive_palette.py")
ct = load("cursor_table", A / "tools/cursor_table.py")
xr = load("xcursor_read", A / "tools/xcursor_read.py")


def rgb(h): v = int(h[1:], 16); return (v >> 16, v >> 8 & 255, v & 255)


def contrast(a, b): return dp.contrast(rgb(a), rgb(b))


class Packages(unittest.TestCase):
    def theme(self, i): return tomllib.loads((ROOT / "themes" / i / "theme.toml").read_text())

    def test_generated_manifests_do_not_drift(self):
        r = subprocess.run([sys.executable, "-I", str(A / "tools/build_packages.py"), str(ROOT), "--check"], capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)

    def test_generated_docs_do_not_drift(self):
        r = subprocess.run([sys.executable, "-I", str(A / "tools/package_docs.py"), str(ROOT), "--check"], capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)

    def test_manifest_contract(self):
        for i in IDS:
            t = self.theme(i)
            self.assertEqual(t["id"], i); self.assertEqual(t["format"], 1)
            self.assertEqual(t["min_shell_version"], "0.3.0", "new panel keys need the framework release")
            self.assertEqual(t["license"], "MIT AND GPL-3.0-only")
            self.assertEqual((ROOT / "themes" / i / "LICENSES/GPL-3.0-only.txt").read_bytes(), (A / "LICENSES/GPL-3.0-only.txt").read_bytes())
            self.assertIn("Separate bundled artwork", (ROOT / "themes" / i / "LICENSE.md").read_text())
            for k in ("wallpaper", "icons", "cursor", "desktop"):
                self.assertNotIn(k, t, f"{i}: {k} must stay a separate user choice")
            self.assertTrue({"layouts", "mfact", "nmaster", "smart_gaps"}.isdisjoint(t.get("rdwm", {})), f"{i}: global tiling settings are not the theme's")
            self.assertTrue(t["bar"].get("applets"), f"{i}: explicit default applets")
            self.assertEqual(t["branding"]["logo"], "icons/kylin-startmenu.png", "the real UKUI start button, never the neutral grid")
            d = t["decoration"]
            self.assertEqual((d["style_light"], d["style_dark"]), ("openkylin", "openkylin-dark"), "the measured pair, in both modes, not a Windows style")
            self.assertEqual((d["fallback_style_light"], d["fallback_style_dark"], d["style"]), ("adwaita-light", "adwaita", "adwaita"))
            self.assertNotIn("height", d, "UKUI's native 38 px header comes from the style")
            self.assertEqual(d.get("radius"), 0 if i == "openkylin-classic" else None, "only Classic (window-radius 0) overrides the native 12 px")
            self.assertEqual((t["control_center"]["style"], t["launcher"]["style"]), ("ukui", "ukui"))
            d = ROOT / "themes" / i
            self.assertEqual(sorted(p.name for p in d.iterdir()), ["ATTRIBUTION.md", "LICENSE.md", "LICENSES", "README.md", "icons", "theme.toml", "widgets"], f"{i}: only the pinned start-button logo is bundled")
            self.assertEqual([p.name for p in (d / "icons").iterdir()], ["kylin-startmenu.png"])
            data = (d / "icons/kylin-startmenu.png").read_bytes()
            self.assertEqual(hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest(), LOGO_BLOBS[i], f"{i}: the logo is the pinned openkylin-theme 3c78d71 blob")
            self.assertEqual(sorted(p.name for p in (d / "widgets").iterdir()), ["ukui-time-12.toml", "ukui-time-24.toml"])
            # the only widget placement is the saved-but-off UKUI time widget: a theme apply must not switch user widgets on or replace them
            self.assertEqual([(w["use"], w["enabled"]) for w in t["widgets"]], [("ukui-time-24", False)], i)

    def test_applets_are_registry_keys(self):
        docs = (pathlib.Path(os.environ.get("UNIVERSAL_PANELS", "/nonexistent"))).read_text() if os.environ.get("UNIVERSAL_PANELS") else None
        known = {"launcher", "app_menu", "workspaces", "window_title", "tasks", "layout", "media", "clock", "weather", "tray", "clipboard", "cpu", "memory", "notifications", "stay_awake", "agents", "lock_keys", "volume", "network", "bluetooth", "battery", "brightness", "search", "dashboard", "control_center", "power", "show_desktop"}
        for i in IDS:
            self.assertLessEqual(set(self.theme(i)["bar"]["applets"]), known, i)
            if docs:
                for k in self.theme(i)["bar"]["applets"]: self.assertIn(f"`{k}`", docs)

    def test_window_material_is_the_window_colour_at_the_global_themes_blur_transparency(self):
        # ukui-globaltheme (gitee, 5274c41) theme.conf: transparencyBlur 65 (HeYin 75), transparencyNoBlur 90; the start menu / sidebar are windows: window colour, not the menu colour
        want = {"openkylin": 0.65, "openkylin-islands": 0.65, "openkylin-light-seeking": 0.65, "openkylin-heyin": 0.75, "openkylin-classic": 0.65}
        for i, a in want.items():
            t = self.theme(i)
            self.assertEqual((t["metrics"]["panel_alpha"], t["metrics"]["dock_alpha"], t["metrics"]["bar_alpha"]), (a, a, a), i)
            for mode in ("light", "dark"):
                self.assertEqual(t["colors"][mode]["overlay"], t["colors"][mode]["background"], f"{i} {mode}: overlay = window-active")
                self.assertEqual(t["colors"][mode]["titlebar"], t["colors"][mode]["background"], f"{i} {mode}: the active title bar is the window colour")
                # window-inactive equals window-active in the source tables of Light-Seeking and HeYin (both modes): their inactive state is not a colour change
                same = t["colors"][mode]["titlebar_inactive"] == t["colors"][mode]["titlebar"]
                self.assertEqual(same, i in ("openkylin-light-seeking", "openkylin-heyin"), f"{i} {mode}")

    def test_panel_modes_follow_upstream(self):
        for i in ("openkylin", "openkylin-light-seeking", "openkylin-heyin", "openkylin-classic"):
            t = self.theme(i)
            self.assertEqual((t["dock"]["style"], t["dock"]["position"], t["dock"]["align"], t["dock"]["size"]), ("windows", "bottom", "left", 32), i)  # 48 px panel, 4+4 px insets -> 32 px icon
            self.assertEqual(t["bar"]["enabled"], "auto")
            self.assertEqual(t["taskbar"], {"thickness": 48, "icon_size": 32, "cell_width": 48, "gap": 0, "inset": 4, "search": "button"}, f"{i}: UKUI classic panel geometry (48 px strip, 32 px icons, a search button not the Windows box)")
        t = self.theme("openkylin-islands")
        self.assertEqual((t["bar"]["style"], t["bar"]["background"], t["bar"]["position"], t["bar"]["height"], t["bar"]["margin"], t["bar"]["spacing"], t["bar"]["radius"]), ("dux", "islands", "bottom", 58, 7, 4, 12))
        # one content-sized centred run (generic bar.cluster): faces 48 px = height - 10, standing 12 px above the edge = margin + 5; 8 px between islands, 12 px padding
        self.assertEqual((t["bar"]["cluster"], t["bar"]["group_gap"], t["bar"]["group_padding"]), (True, 8, 12))
        self.assertEqual(t["bar"]["height"] - 10, 48); self.assertEqual(t["bar"]["margin"] + 5, 12)
        self.assertEqual((t["bar"]["left"], t["bar"]["center"]), (["launcher"], ["tasks", "search"]))
        self.assertIs(t["dock"]["enabled"], False)

    def test_radii_follow_global_theme(self):
        self.assertEqual({i: self.theme(i)["metrics"]["radius"] for i in IDS}, {"openkylin": 12, "openkylin-islands": 12, "openkylin-light-seeking": 12, "openkylin-heyin": 12, "openkylin-classic": 0})

    def test_readable_in_every_variant(self):
        for i in IDS:
            t = self.theme(i)
            for mode in ("light", "dark"):
                c = t["colors"][mode]
                for what in ("background", "surface", "popup", "titlebar", "sidebar"):
                    self.assertGreaterEqual(contrast(c["text"], c[what]), 4.5, f"{i} {mode} text on {what}")
                self.assertGreaterEqual(contrast(c["bar_text"], c["bar"]), 4.5, f"{i} {mode} bar text")
                self.assertGreaterEqual(contrast(c["accent_text"], c["accent"]), 3.0, f"{i} {mode} accent text")
                self.assertGreaterEqual(contrast(c["text_secondary"], c["background"]), 4.5, f"{i} {mode} secondary")
                self.assertGreaterEqual(contrast(c["text_faint"], c["background"]), 3.0, f"{i} {mode} faint")
                lum = dp.lum(rgb(c["background"]))
                self.assertEqual(lum > 0.3, mode == "light", f"{i} {mode} brightness")

    def test_known_upstream_values(self):
        t = self.theme("openkylin")["colors"]
        self.assertEqual((t["light"]["background"], t["light"]["accent"], t["dark"]["background"], t["dark"]["surface"]), ("#f5f5f5", "#3676f5", "#2e2e2e", "#1a1a1a"))
        self.assertEqual(self.theme("openkylin-heyin")["colors"]["dark"]["accent"], "#8926eb")
        self.assertEqual(self.theme("openkylin-classic")["colors"]["light"]["accent"], "#3790fa")
        self.assertEqual(self.theme("openkylin-light-seeking")["colors"]["light"]["accent"], "#3790fa")


class Provenance(unittest.TestCase):
    def test_sources_pinned(self):
        s = json.loads((A / "sources.json").read_text())
        names = {r["repo"].rsplit("/", 1)[1] for r in s["repos"]}
        self.assertTrue({"ukui-panel", "ukui-globaltheme", "ukui-themes", "openkylin-theme", "dmz-cursor-theme", "ukui-control-center"} <= names)
        for r in s["repos"]:
            self.assertRegex(r["commit"], r"^[0-9a-f]{40}$"); self.assertTrue(r["repo"].startswith("https://github.com/openkylin/"))
            self.assertIn("licence_declared_in_debian_copyright", r)
        for k, v in s["verified_trees"].items(): self.assertRegex(v["git_tree"], r"^[0-9a-f]{40}$", k)

    def test_token_tables_resolved(self):
        d = A / "derived"
        for t in ("v11", "Light-Seeking", "HeYin", "Classic"):
            for m in ("light", "dark"):
                tok = json.loads((d / f"tokens-{t}-{m}.json").read_text())
                self.assertGreater(len(tok), 150)
                # upstream tables contain a few dangling references (e.g. HeYin `--KFont-KWhite`, Classic `--KComponet-Alpha-Click`); every
                # token the palette mapping consumes must resolve
                used = "window-active window-inactive base-active kfont-primary kfont-secondary kdivider kbrand-normal kbrand1 kerror-normal ksuccess-normal kwarning-normal kgray-3 kcontain-secondary-normal".split()
                self.assertEqual([k for k in used if k not in tok or "var(" in tok[k]], [], f"{t}-{m}")

    def catalogue(self): return tomllib.loads((A / "asset-packs.toml").read_text())

    def test_asset_catalogue_matches_pins_and_covers_every_package(self):
        c = self.catalogue(); s = json.loads((A / "sources.json").read_text())["verified_trees"]
        self.assertEqual((c["format"], c["family"]), (1, "openkylin"))
        src = {x["id"]: x for x in c["source"]}
        for x in src.values():
            self.assertRegex(x["commit"], r"^[0-9a-f]{40}$"); self.assertTrue(x["repo"].startswith("https://") and x["archive_url"].startswith("https://"))
            self.assertTrue(x["license"] and x["license_files"])
        packs = {p["id"]: p for p in c["pack"]}
        for p in packs.values():
            self.assertRegex(p["tree"], r"^[0-9a-f]{40}$")
            self.assertEqual(s[f"{p['source']}:{p['path']}"]["git_tree"], p["tree"], f"{p['id']}: the catalogue tree equals the one verified in sources.json")
            self.assertFalse({"command", "script", "run", "exec"} & set(p), "data only")
        self.assertEqual(src["dmz-cursor-theme"]["license"], "CC-BY-SA-3.0")
        # licence blobs are pinned and equal the ones recorded in sources.json; the Moka provenance stays explicitly unresolved (never upgraded)
        blobs = {r["repo"].rsplit("/", 1)[1]: r["licence_blobs"] for r in json.loads((A / "sources.json").read_text())["repos"]}
        for sid, x in src.items():
            self.assertEqual(len(x["license_oids"]), len(x["license_files"]), sid)
            self.assertEqual(x["license_oids"], [blobs[sid][f] for f in x["license_files"]], f"{sid}: pinned blob equals the one in sources.json")
        un = json.loads((A / "sources.json").read_text())["unresolved_provenance"]
        self.assertTrue(un and "moka" in un[0]["evidence"].lower() and un[0]["status"].startswith("UNRESOLVED"))
        for pid in ("openkylin-ukui-default", "openkylin-ukui-classical", "openkylin-ukui-fashion"):
            self.assertIn("oka", {p["id"]: p for p in c["pack"]}[pid]["description"], f"{pid}: the Moka provenance note stays in the description")
        rec = {r["theme"]: r for r in c["recommend"]}
        self.assertEqual(set(rec), set(IDS), "every package suggests a pack pair")
        for r in rec.values():
            self.assertIn("icons", packs[r["icons"]]["provides"]); self.assertIn("cursors", packs[r["cursors"]]["provides"])
        self.assertEqual((rec["openkylin-heyin"]["icons"], rec["openkylin-heyin"]["cursors"]), ("openkylin-ukui-fashion", "openkylin-dmz-white"))
        self.assertEqual(rec["openkylin-classic"]["icons"], "openkylin-ukui-classical")

    def test_cursor_tables_are_data_and_sane(self):
        c = self.catalogue()
        for p in c["pack"]:
            tabs = p.get("xcursor", [])
            if p["id"] in ("openkylin-dark-sense", "openkylin-blue-crystal"):
                self.assertEqual(len(tabs), 57, p["id"])
                names = {t["name"] for t in tabs}; self.assertIn("left_ptr", names)
                aliases = [a for t in tabs for a in t.get("aliases", [])]
                self.assertEqual(len(aliases), len(set(aliases)), "no alias twice"); self.assertFalse(names & set(aliases), "an alias never shadows a built cursor")
                self.assertTrue({"default", "pointer", "text", "not-allowed", "col-resize"} <= set(aliases), "CSS names added")
                for t in tabs:
                    for line in t["config"].strip().splitlines():
                        f = line.split(); size, xh, yh, png = int(f[0]), int(f[1]), int(f[2]), f[3]
                        self.assertTrue(0 <= xh < size and 0 <= yh < size, (p["id"], t["name"], line))
                        self.assertRegex(png, r"^\d+x\d+/[A-Za-z0-9_.+-]+\.png$")
            else:
                self.assertFalse(tabs, p["id"])
        left = next(t for t in self.catalogue()["pack"] if t["id"] == "openkylin-dark-sense")["xcursor"]
        self.assertIn("24 4 0 24x24/left_ptr.png", next(t for t in left if t["name"] == "left_ptr")["config"])

    def test_catalogue_is_regenerated_from_the_pins(self):
        # the generator needs the local clones; skip when absent, otherwise the committed file must equal its output
        clones = pathlib.Path.home() / ".cache/decebal-upstreams/openkylin"
        if not (clones / "openkylin-theme/.git").exists(): self.skipTest("no local upstream clones")
        with tempfile.TemporaryDirectory() as td:
            out = pathlib.Path(td) / "c.toml"
            r = subprocess.run([sys.executable, "-I", str(A / "tools/make_asset_catalog.py"), str(clones), str(out)], capture_output=True, text=True)
            self.assertEqual(r.returncode, 0, r.stderr); self.assertEqual(out.read_text(), (A / "asset-packs.toml").read_text())

    def test_no_private_installer_remains(self):
        for gone in ("packs.json", "tools/fetch_pack.py", "tools/make_packs.py", "tools/xcursor.py"):
            self.assertFalse((A / gone).exists(), f"{gone}: installing and building packs belongs to the shared installer")
        for f in (A / "tools").glob("*.py"):
            self.assertNotRegex(f.read_text(), r"(?m)^\s*(shutil\.(rmtree|copytree)|os\.(rename|replace|symlink)\()|subprocess.*(clone|xcursorgen)", f"{f.name}: no install/clone/build code")

    def check_catalogue_state(self, w, path):
        """Either state is valid. Pre-integration: none of this port's themes/images is in the root catalogue, and the merge adds them without a clash while the merged result keeps the
        invariants of tests/curated_theme.rs. Post-integration: the root catalogue holds EXACTLY this fragment's records (deep-equal) for all five themes and ten images. Partial = failure."""
        c = json.loads(pathlib.Path(path).read_text())
        mine_t, mine_i = set(w["themes"]), set(w["images"])
        in_t, in_i = mine_t & set(c["themes"]), mine_i & set(c["images"])
        if in_t or in_i:
            self.assertEqual((in_t, in_i), (mine_t, mine_i), "partially integrated: every theme and image of the fragment must be present")
            for k in mine_t: self.assertEqual(c["themes"][k], w["themes"][k], f"integrated theme record {k} differs from the fragment")
            for k in mine_i: self.assertEqual(c["images"][k], w["images"][k], f"integrated image record {k} differs from the fragment")
            m = c
        else:
            with tempfile.TemporaryDirectory() as td:
                out = pathlib.Path(td) / "c.json"
                r = subprocess.run([sys.executable, "-I", str(A / "tools/merge_catalogue.py"), str(path), str(out)], capture_output=True, text=True)
                self.assertEqual(r.returncode, 0, r.stderr); m = json.loads(out.read_text())
        # invariants of tests/curated_theme.rs on the (integrated or merged) catalogue: every curated choice is distinct and every image is used
        used = [m["themes"][k][md]["image_id"] for k in m["themes"] for md in ("dark", "light") if k != "dux-lavender"]
        self.assertEqual(len(used), len(set(used)), "every curated choice stays distinct")
        self.assertEqual(len({m["themes"][k][md]["image_id"] for k in m["themes"] for md in ("dark", "light")}), len(m["images"]), "every image is used")

    def test_catalogue_state_accepts_pre_and_post_integration(self):
        w = json.loads((A / "wallpapers.json").read_text())
        with tempfile.TemporaryDirectory() as td:
            root = pathlib.Path(td); cat = json.loads((ROOT / "data/wallhaven-curated.json").read_text())
            # Start this simulated pre-integration fixture before the port even when
            # the real repository already contains its exact records.
            for section in ("themes", "images"):
                for key in w[section]:
                    cat[section].pop(key, None)
            pre = root / "pre.json"; pre.write_text(json.dumps(cat))
            self.check_catalogue_state(w, pre)
            post = root / "post.json"
            r = subprocess.run([sys.executable, "-I", str(A / "tools/merge_catalogue.py"), str(pre), str(post)], capture_output=True, text=True); self.assertEqual(r.returncode, 0, r.stderr)
            self.check_catalogue_state(w, post)                       # what root's integration produces
            bad = json.loads(post.read_text()); bad["images"][next(iter(w["images"]))]["subject"] = "edited"
            (root / "bad.json").write_text(json.dumps(bad))
            with self.assertRaises(AssertionError): self.check_catalogue_state(w, root / "bad.json")      # a differing integrated record is caught
            part = json.loads(post.read_text()); del part["themes"][next(iter(w["themes"]))]
            (root / "part.json").write_text(json.dumps(part))
            with self.assertRaises(AssertionError): self.check_catalogue_state(w, root / "part.json")     # a partial integration is caught

    def test_wallpapers(self):
        w = json.loads((A / "wallpapers.json").read_text())
        used = [w["themes"][i][m]["image_id"] for i in IDS for m in ("light", "dark")]
        self.assertEqual(len(used), len(set(used)), "ten distinct images")
        for k, v in w["images"].items():
            self.assertEqual((v["purity"], v["category"]), ("sfw", "general")); fr = v["full_resolution_review"]
            self.assertRegex(fr["sha256"], r"^[0-9a-f]{64}$"); self.assertTrue(fr["dimensions_verified"]); self.assertLessEqual(fr["bytes"], 32 << 20)
            self.assertRegex(v["evidence"]["detail_sha256"], r"^[0-9a-f]{64}$"); self.assertRegex(v["evidence"]["thumbnail_sha256"], r"^[0-9a-f]{64}$")
            self.assertTrue(v["thumbnail_url"].startswith("https://th.wallhaven.cc/lg/")); self.assertFalse(v["ai_review"]["flagged_ai"])
            self.assertGreaterEqual(v["resolution"][0], 2560); self.assertGreaterEqual(v["resolution"][1], 1440)
            self.assertNotIn(133451, [t["id"] for t in v["tags"]])
            self.assertIsNone(v["license"]["identifier"]); self.assertIsNone(v["uploader"]["is_author"]); self.assertIsNone(v["author"]["name"])
            self.assertTrue(v["full_url"].startswith("https://w.wallhaven.cc/full/"))
        self.check_catalogue_state(w, ROOT / "data/wallhaven-curated.json")
        self.assertFalse([p for p in (ROOT / "themes").glob("openkylin*/**/*") if p.suffix.lower() in (".jpg", ".jpeg", ".png", ".webp") and p.name != "kylin-startmenu.png"], "no image in packages but the start-button logo")


class CursorData(unittest.TestCase):
    def test_table_parser_is_text_only_and_strict(self):
        text = ('  echo "24 11 11 ../24x24/all-scroll.png 0 "> cursors/all-scroll.in\n  echo "24 3 0 ../24x24/w_1.png 50" > cursors/w.in\n  echo "24 3 0 ../24x24/w_2.png 50" >> cursors/w.in\n'
                '  xcursorgen all-scroll.in all-scroll\n  xcursorgen w.in w\n  ln -s w wait2\n  rm -rf /\n  echo "24 1 1 ../../x.png 0 "> cursors/../esc.in\n  ln -s a/b c\n')
        built, frames, links = ct.parse(text)
        self.assertEqual(built, ["all-scroll", "w"]); self.assertEqual(frames["w"], [(24, 3, 0, "24x24/w_1.png", 50), (24, 3, 0, "24x24/w_2.png", 50)])
        self.assertEqual(links, [("w", "wait2")]); self.assertNotIn("esc", frames)

    def test_css_aliases_are_additive(self):
        text = '  echo "24 4 0 ../24x24/left_ptr.png 0 "> cursors/left_ptr.in\n  echo "24 4 0 ../24x24/pointer.png 0 "> cursors/pointer.in\n  xcursorgen left_ptr.in left_ptr\n  xcursorgen pointer.in pointer\n'
        tables, n, a = ct.toml_tables(text)
        self.assertEqual(n, 2); self.assertIn('"default"', tables); self.assertEqual(tables.count('"pointer"'), 1, "an upstream cursor is never aliased over")
        self.assertNotIn('"text"', tables, "no alias to a missing target")

    def test_reader_enforces_limits(self):
        with tempfile.TemporaryDirectory() as td:
            f = pathlib.Path(td) / "c"; f.write_bytes(b"Xcur" + struct.pack("<III", 16, 0x10000, 100000))
            with self.assertRaises(ValueError): xr.read(f)
            f.write_bytes(b"Xcur" + struct.pack("<III", 16, 0x10000, 1) + struct.pack("<III", xr.IMAGE, 24, 28) + struct.pack("<IIIIIIIII", 36, xr.IMAGE, 24, 1, 5000, 5000, 0, 0, 0))
            with self.assertRaises(ValueError): xr.read(f)


class HarnessDisplays(unittest.TestCase):
    """The native harness must never touch :0 or a foreign X server (live display safety)."""

    def harness(self, out=None):
        src = (ROOT / "tests/e2e/openkylin-native.py").read_text().replace("\nmain()\n", "\n")
        if out is None:
            scratch = tempfile.TemporaryDirectory(prefix="okn-harness-test-", dir="/var/tmp")
            self.addCleanup(scratch.cleanup)
            out = str(pathlib.Path(scratch.name) / "evidence")
        old = sys.argv; sys.argv = ["x", "/nonexistent", out, "--matrix", "none"]
        try:
            ns = {"__file__": str(ROOT / "tests/e2e/openkylin-native.py"), "__name__": "harness"}
            exec(compile(src, "openkylin-native", "exec"), ns)
        finally: sys.argv = old
        return ns

    def test_existing_evidence_is_refused_without_touching_it(self):
        h = self.harness()
        sentinel = h["OUT"] / "keep.txt"
        sentinel.write_bytes(b"keep evidence\n")
        with self.assertRaises(FileExistsError):
            self.harness(str(h["OUT"]))
        self.assertEqual(sentinel.read_bytes(), b"keep evidence\n")

    def test_reserved_high_range_and_refusals(self):
        h = self.harness(); taken = set(); n = h["free_display"](taken)
        self.assertGreaterEqual(n, 700); self.assertIn(n, taken)
        self.assertNotEqual(h["free_display"](taken), n, "a number is never handed out twice")
        class Dead:   # a stand-in server that has exited
            pid = 1
            def poll(self): return 1
        for bad in (0, 1, 699):
            with self.assertRaises(AssertionError): h["verify_owned"](bad, Dead())
        with self.assertRaises(AssertionError): h["verify_owned"](n, Dead())   # not our live server: refuse before any input

    def test_no_auto_probe_or_foreign_cleanup_in_harness(self):
        src = (ROOT / "tests/e2e/openkylin-native.py").read_text()
        self.assertNotIn("-displayfd", src); self.assertNotIn("DISPLAY=\":0\"", src)
        self.assertNotRegex(src, r"unlink\(|os\.remove\(|shutil\.rmtree|rm -rf")


@unittest.skipUnless(os.environ.get("ASSET_DECTHEMECTL"), "set ASSET_DECTHEMECTL (a decthemectl with `assets`) to run the shared validator")
class SharedAssetValidator(unittest.TestCase):
    def test_catalogue_check(self):
        with tempfile.TemporaryDirectory() as td:
            env = dict(PATH="/usr/bin:/bin", HOME=td)
            r = subprocess.run([os.environ["ASSET_DECTHEMECTL"], "assets", "catalog", "check", str(A / "asset-packs.toml")], capture_output=True, text=True, env=env)
            self.assertEqual(r.returncode, 0, r.stdout + r.stderr); self.assertIn("2 sources, 6 packs, 5 recommendations", r.stdout)


@unittest.skipUnless(os.environ.get("DECTHEMECTL"), "set DECTHEMECTL to validate with the real CLI")
class Cli(unittest.TestCase):
    def test_validate(self):
        for i in IDS:
            r = subprocess.run([os.environ["DECTHEMECTL"], "theme", "validate", str(ROOT / "themes" / i)], capture_output=True, text=True)
            self.assertEqual(r.returncode, 0, r.stdout + r.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=1)
