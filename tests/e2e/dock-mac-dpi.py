#!/usr/bin/env python3
"""Native acceptance of the macOS-style dock at a display scale (Xft.dpi / DECEBAL_SCALE 1, 1.25, 2): the REAL window, strut, picture, keyboard ring and
pointer, on a private Xvfb with the real `decdock` and `rdwm`, real XTEST input. Builds on the scenario class of dock-mac-keyboard.py (same private,
ownership-checked high display, same fake apps and stand-ins).

Usage: python3 tests/e2e/dock-mac-dpi.py BIN_DIR NEW_EVIDENCE_DIR [scenario-substring ...]

Phases
  scale     4 edges x 3 scales x dark/light: the window and strut are the 100 % ones times the scale; the focus ring and the menu scale with it; the physical
            centre of every ring position is where a real click activates that item (keyboard and pointer agree); hover magnification follows the pointer.
  clamp     short and narrow screens (1366x600, 900x520, 640x480) at 125 % / 200 %, 4 edges, 12 apps: the real window lies inside the screen, nothing is
            cut off the keyboard (Home/End + Enter reach the first and last item, the ring is visible) and a click on the ring's centre activates it.
  reduced   DECEBAL_REDUCE_MOTION=1 at 200 %: hovering does not magnify, the click mapping is unchanged.
"""
import importlib.util
import json
from pathlib import Path
import sys
import time

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("dockkb", HERE / "dock-mac-keyboard.py")
kb = importlib.util.module_from_spec(spec)
spec.loader.exec_module(kb)
from PIL import ImageChops  # noqa: E402

EDGES, SCALES, MODES = kb.EDGES, kb.SCALES, kb.MODES
SCREENS = [(1366, 600), (900, 520), (640, 480)]


def centre(box):
    return ((box[0] + box[2]) / 2, (box[1] + box[3]) / 2)


def near(a, b, tol):
    return abs(a - b) <= tol


class Dpi(kb.Scenario):
    def __init__(self, bin_dir, evidence, edge, scale, mode, screen=(1280, 800), many=False, reduce=False, tag=""):
        old = kb.SCREEN
        kb.SCREEN = screen
        try:
            super().__init__(bin_dir, evidence, edge, scale, mode, tag)
        finally:
            kb.SCREEN = old
        self.screen = screen
        self.names = ["launchpad", "alpha", "beta", "gamma"] + ([f"app{i}" for i in range(1, 9)] if many else []) + ["trash"]
        pinned = json.dumps({"version": 1, "pinned": self.names, "order": self.names})
        (self.root / "dc/dock-pinned-macos.json").write_text(pinned)
        if reduce:
            self.env["DECEBAL_REDUCE_MOTION"] = "1"

    def window_geometry(self):
        return self.geometry()

    def strut(self):
        out = self.x("xprop", "-id", str(self.dock), "_NET_WM_STRUT_PARTIAL", check=False)
        return [int(v) for v in out.split("=")[1].replace(" ", "").split(",")] if "=" in out else []

    def ring_boxes(self, base):
        """The focus ring's box on every item (physical, window coordinates), walking from the first to the last item."""
        _, fwd, _ = kb.KEYS[self.edge]
        boxes = []
        self.focus_request()
        for i in range(len(self.names)):
            r = self.ring(base, f"ring-{i}")
            boxes.append(r[2] if r else None)
            if i + 1 < len(self.names):
                self.key(fwd)
        return boxes

    def click_at_window(self, point):
        x, y, _, _ = self.geometry()
        self.x("xdotool", "mousemove", str(int(x + point[0])), str(int(y + point[1])))
        time.sleep(0.15)
        self.x("xdotool", "click", "1")
        time.sleep(0.5)

    def away(self):
        self.x("xdotool", "mousemove", str(self.screen[0] // 2), str(self.screen[1] // 3))
        time.sleep(1.0)  # the magnification eases back in about 110 ms; a picture taken now is the resting one

    def measure(self, ref):
        """Shared by `scale` and `reduced`: returns the facts this run measured (for the next scale to compare with)."""
        s = float(self.scale)
        back, fwd, menu_key = kb.KEYS[self.edge]
        self.start()
        self.away()
        base = self.shot("baseline")
        g = self.geometry()
        facts = {"window": base.size, "strut": self.strut(), "boxes": None, "menu": None}
        self.check("the real window size is what the picture is", (g[2], g[3]) == base.size, (g, base.size))
        self.check("the window lies inside the screen", g[0] >= 0 and g[1] >= 0 and g[0] + g[2] <= self.screen[0] and g[1] + g[3] <= self.screen[1], (g, self.screen))
        self.check("the dock sits on the requested edge", self.edge_ok(g) if self.screen == (1280, 800) else True, g)
        strut = facts["strut"]
        self.check("a strut is published and lies inside the screen", len(strut) == 12 and max(strut[:4]) <= max(self.screen), strut)
        boxes = self.ring_boxes(base)
        facts["boxes"] = boxes
        self.check("the focus ring is visible on every item", all(boxes), boxes)
        self.key("Escape")
        if ref:
            rw, rh = ref["window"]
            natural = (int(-(-rw * s // 1)), int(-(-rh * s // 1)))
            shrunk = natural[0] > self.screen[0] or natural[1] > self.screen[1]
            if shrunk:
                # the dock would not fit its output at this scale: it shrinks to it (the along-edge side is exactly the output's), nothing is cut off the keyboard
                vertical = self.edge in ("left", "right")
                along, out = (base.size[1], self.screen[1]) if vertical else (base.size[0], self.screen[0])
                self.check("a dock that does not fit its output at this scale is shrunk to exactly it", near(along, out, 1), (base.size, self.screen, natural))
                self.check("the shrunk dock's icons are smaller than the proportional ones", all(boxes) and (boxes[1][2] - boxes[1][0]) < (ref["boxes"][1][2] - ref["boxes"][1][0]) * s, boxes[1])
                if len(strut) == 12:
                    self.check("the strut of the shrunk dock stays inside the screen", max(strut[:4]) <= max(self.screen), strut)
                facts["shrunk"] = True
            else:
                self.check("the real window is the 100 % window times the scale", near(base.size[0], rw * s, 2) and near(base.size[1], rh * s, 2), (base.size, (rw, rh), s))
                if len(strut) == 12 and len(ref["strut"]) == 12:
                    self.check("the strut is the 100 % strut times the scale", all(near(a, b * s, 3) for a, b in zip(strut[:4], ref["strut"][:4])), (strut, ref["strut"], s))
                if all(boxes) and ref["boxes"] and all(ref["boxes"]):
                    ok = all(near(centre(b)[0], centre(r)[0] * s, 3) and near(centre(b)[1], centre(r)[1] * s, 3) for b, r in zip(boxes, ref["boxes"]))
                    self.check("every ring position is the 100 % one times the scale (icons and layout scale together)", ok, (boxes[:2], ref["boxes"][:2]))
                    bw, rb = boxes[1][2] - boxes[1][0], ref["boxes"][1][2] - ref["boxes"][1][0]
                    self.check("the ring is as many times larger as the display", near(bw, rb * s, 3), (bw, rb, s))
        # the keyboard ring's centre is where a real click activates that item
        mark = lambda name: (self.root / name).exists() and (self.root / name).read_text()
        self.click_at_window(centre(boxes[0]))
        self.check("a click at the ring's centre of the first item starts the launcher", bool(mark("launchpad.log")))
        self.click_at_window(centre(boxes[-1]))
        self.check("a click at the ring's centre of the last item opens Trash", "trash" in (mark("open.log") or ""))
        alpha = self.win("Alpha Window")
        self.x("xdotool", "windowactivate", "--sync", str(self.win("Beta Window")), check=False)
        self.click_at_window(centre(boxes[1]))
        wait_active = kb.wait(lambda: self.active() == alpha, "alpha focused by a click", 5)
        self.check("a click at the second item's centre focuses its window", bool(wait_active))
        self.away()
        # hover magnification follows the physical pointer
        plain = self.shot("hover-away")
        x, y, _, _ = self.geometry()
        c = centre(boxes[2])
        self.x("xdotool", "mousemove", str(int(x + c[0])), str(int(y + c[1])))
        time.sleep(0.7)
        hovered = self.shot("hover")
        changed = ImageChops.difference(plain, hovered).getbbox() is not None
        if self.reduce:
            self.check("with reduced motion hovering does not magnify", not changed)
        else:
            self.check("hovering the third item magnifies it under the physical pointer", changed)
        self.away()
        if not self.many and not self.reduce:
            # the context menu: arrow towards the desktop; its size scales with the display (fonts and rows)
            self.focus_request()
            self.key(menu_key)
            opened = self.shot("menu")
            facts["menu"] = opened.size
            if ref and ref.get("menu") and not facts.get("shrunk"):
                mw, mh = ref["menu"]
                self.check("the menu (font metrics included) is the 100 % menu times the scale", near(opened.size[0], mw * s, max(4, mw * s * 0.04)) and near(opened.size[1], mh * s, max(4, mh * s * 0.04)), (opened.size, (mw, mh), s))
            self.check("the open menu lies inside the screen", opened.size[0] <= self.screen[0] and opened.size[1] <= self.screen[1], opened.size)
            self.key("Escape")
            self.key("Escape")
        return facts

    def learn_accent(self, base):
        """The ring's colour: the most common colour that changes when the first item gets the ring (at the unscrolled start)."""
        from collections import Counter
        self.focus_request()
        img = self.shot("accent-probe")
        px, bp = img.load(), base.load()
        counts = Counter(px[x, y] for y in range(img.size[1]) for x in range(img.size[0]) if sum(abs(a - b) for a, b in zip(px[x, y], bp[x, y])) > 40)
        self.key("Escape")
        self.accent = counts.most_common(1)[0][0] if counts else None
        return self.accent

    def find_ring(self, label):
        """Bounding box of the ring by its own colour, valid whatever the dock behind it is doing (e.g. scrolled)."""
        img = self.shot(label)
        if not self.accent:
            return None
        px = img.load()
        pts = [(x, y) for y in range(img.size[1]) for x in range(img.size[0]) if sum(abs(a - b) for a, b in zip(px[x, y], self.accent)) <= 24]
        # the ring is the biggest connected group of that colour (icons may hold a few pixels of a similar one)
        todo, best = set(pts), []
        while todo:
            stack, group = [todo.pop()], []
            while stack:
                x, y = stack.pop()
                group.append((x, y))
                for n in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1), (x + 1, y + 1), (x - 1, y - 1), (x + 1, y - 1), (x - 1, y + 1)):
                    if n in todo:
                        todo.discard(n)
                        stack.append(n)
            if len(group) > len(best):
                best = group
        if len(best) < 40:
            return None
        xs, ys = [p[0] for p in best], [p[1] for p in best]
        return (min(xs), min(ys), max(xs) + 1, max(ys) + 1)

    def ends_reachable(self, base):
        """Home/End + Enter reach the first and last item even when the dock is wider than the screen."""
        for key, mark, name in (("End", "open.log", "last item (Trash)"), ("Home", "launchpad.log", "first item (Launchpad)")):
            before = (self.root / mark).read_text() if (self.root / mark).exists() else ""
            self.focus_request()
            self.key(key)
            r = self.find_ring(f"ring-{key}")
            inside = r is not None and r[0] >= 0 and r[1] >= 0 and r[2] <= base.size[0] and r[3] <= base.size[1]
            self.check(f"{key}: the whole ring is visible, inside the window, on the {name}", inside, r)
            self.key("Return")
            time.sleep(0.5)
            after = (self.root / mark).read_text() if (self.root / mark).exists() else ""
            self.check(f"{key} + Enter activates the {name}", len(after) > len(before), (before, after))


def run_scale(bin_dir, evidence, refs, edge, scale, mode, report):
    s = Dpi(bin_dir, evidence, edge, scale, mode)
    s.many = s.reduce = False
    try:
        facts = s.measure(refs.get((edge, mode)))
        if scale == "1":
            refs[(edge, mode)] = facts
    except Exception as e:  # keep going
        s.check("scenario completed", False, repr(e))
    finally:
        s.close()
    report.append({"scenario": s.name, "ownership": s.ownership, "checks": s.checks})


def run_clamp(bin_dir, evidence, edge, scale, screen, report):
    s = Dpi(bin_dir, evidence, edge, scale, "dark", screen=screen, many=True, tag=f"clamp-{screen[0]}x{screen[1]}-")
    s.many, s.reduce = True, False
    try:
        s.start()
        s.away()
        base = s.shot("baseline")
        g = s.geometry()
        s.check("the real window lies inside the short/narrow screen", g[0] >= 0 and g[1] >= 0 and g[0] + g[2] <= screen[0] and g[1] + g[3] <= screen[1], (g, screen))
        s.check("the real window is the picture", (g[2], g[3]) == base.size, (g, base.size))
        strut = s.strut()
        s.check("the strut stays inside the screen", len(strut) == 12 and max(strut[:4]) <= max(screen), strut)
        s.check("the ring's colour was learnt", s.learn_accent(base) is not None)
        s.ends_reachable(base)
        # a click at the last item's ring centre (the dock has scrolled to it)
        s.focus_request()
        s.key("End")
        r = s.find_ring("ring-last")
        s.check("the ring is found on the last item", r is not None)
        if r:
            before = (s.root / "open.log").read_text() if (s.root / "open.log").exists() else ""
            s.click_at_window(centre(r))
            after = (s.root / "open.log").read_text() if (s.root / "open.log").exists() else ""
            s.check("a click at the last item's ring centre opens Trash", len(after) > len(before))
    except Exception as e:
        s.check("scenario completed", False, repr(e))
    finally:
        s.close()
    report.append({"scenario": s.name, "ownership": s.ownership, "checks": s.checks})


def run_reduced(bin_dir, evidence, edge, report):
    s = Dpi(bin_dir, evidence, edge, "2", "dark", reduce=True, tag="reduced-")
    s.many, s.reduce = False, True
    try:
        s.measure(None)
    except Exception as e:
        s.check("scenario completed", False, repr(e))
    finally:
        s.close()
    report.append({"scenario": s.name, "ownership": s.ownership, "checks": s.checks})


def main():
    bin_dir, evidence = Path(sys.argv[1]).resolve(), Path(sys.argv[2])
    filters = sys.argv[3:]
    evidence.mkdir(parents=True)
    report, refs = [], {}
    want = lambda name: not filters or any(f in name for f in filters)
    for edge in EDGES:
        for mode in MODES:
            for scale in SCALES:  # 1 first: the others compare with it
                name = f"{edge}-{scale.replace('.', '_')}-{mode}"
                if want(name) or (scale == "1" and any(want(f"{edge}-{sc.replace('.', '_')}-{mode}") for sc in SCALES)):
                    print("==", name, flush=True)
                    run_scale(bin_dir, evidence, refs, edge, scale, mode, report)
    for edge in EDGES:
        for scale in ("1.25", "2"):
            for screen in SCREENS:
                name = f"clamp-{edge}-{scale.replace('.', '_')}-{screen[0]}x{screen[1]}"
                if want(name):
                    print("==", name, flush=True)
                    run_clamp(bin_dir, evidence, edge, scale, screen, report)
    for edge in EDGES:
        name = f"reduced-{edge}-2"
        if want(name):
            print("==", name, flush=True)
            run_reduced(bin_dir, evidence, edge, report)
    (evidence / "report.json").write_text(json.dumps(report, indent=2))
    failed = [(r["scenario"], c["name"], c["detail"]) for r in report for c in r["checks"] if not c["ok"]]
    total = sum(len(r["checks"]) for r in report)
    print(f"{total - len(failed)}/{total} checks passed in {len(report)} scenarios")
    for f in failed:
        print("FAILED", *f)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
