#!/usr/bin/python3 -I
"""Extracts the design numbers the Lingmo package claims straight from the pinned upstream sources.

usage: extract-reference-numbers.py GIT_CLONE OUT_JSON      (GIT_CLONE: a local clone of LingmoOS/lingmo-compositor or lingmo-sdk; read with `git show`, nothing runs)

Every value is read by a regular expression from `git show <commit>:<path>` at the commit pinned in docs/desktop-ports/lingmo/sources.json, so the
manifest's claims (32 px bar, 54 px dock icon, 1.5 magnification, ...) are tied to the original, not to a transcription. A missing or changed
constant fails loudly. The result is committed as native/reference-numbers.json and checked against theme.toml by tests/lingmo_desktop_port.rs.
"""
import json, re, subprocess, sys

COMPOSITOR = "1cdaf0a86596ef3732e23d2450318728a180877b"
CHECKS = [
    # name, path, regex (one capture group), type
    ("status_bar_height", "src/shell/statusbar/status_bar_window.h", r"static constexpr int kHeight = (\d+);", int),
    ("status_bar_blur_sigma", "src/shell/statusbar/status_bar_settings.h", r"kDefaultBlurSigma = ([\d.]+);", float),
    ("status_bar_opacity", "src/shell/statusbar/status_bar_settings.h", r"kDefaultOpacity = ([\d.]+);", float),
    ("bar_icon_size", "src/shell/statusbar/bar_item.h", r"kIconSize = ([\d.]+);", float),
    ("bar_edge_margin", "src/shell/statusbar/bar_item.h", r"kEdgeMargin = ([\d.]+);", float),
    ("bar_item_gap", "src/shell/statusbar/bar_item.h", r"kItemGap = ([\d.]+);", float),
    ("bar_highlight_gap", "src/shell/statusbar/bar_item.h", r"kHighlightGap = ([\d.]+);", float),
    ("dock_base_icon_size", "src/shell/dock/dock_metrics.h", r"static constexpr int kBaseIconSize = (\d+);", int),
    ("dock_icon_gap", "src/shell/dock/dock_metrics.h", r"int iconGap = (\d+);", int),
    ("dock_length_padding", "src/shell/dock/dock_metrics.h", r"int lengthPadding = (\d+);", int),
    ("dock_outer_padding", "src/shell/dock/dock_metrics.h", r"int outerPadding = (\d+);", int),
    ("dock_magnification_factor", "src/shell/dock/dock_settings.h", r"double magnificationFactor = ([\d.]+);", float),
    ("decoration_titlebar_height", "src/decoration/decoration_theme.h", r"int titleBarHeight = (\d+);", int),
    ("decoration_button_size", "src/decoration/decoration_theme.h", r"int buttonSize = (\d+);", int),
    ("decoration_corner_radius", "src/decoration/window_visual_style.cpp", r"constexpr int kRadius = (\d+);", int),
]

def show(clone, commit, path):
    return subprocess.run(["git", "-C", clone, "show", f"{commit}:{path}"], capture_output=True, text=True, check=True).stdout

def main():
    clone, out = sys.argv[1:3]
    head = subprocess.run(["git", "-C", clone, "rev-parse", COMPOSITOR], capture_output=True, text=True, check=True).stdout.strip()
    assert head == COMPOSITOR, head
    result = {"repo": "LingmoOS/lingmo-compositor", "commit": COMPOSITOR, "values": {}}
    for name, path, rx, ty in CHECKS:
        m = re.search(rx, show(clone, COMPOSITOR, path))
        if not m:
            sys.exit(f"{name}: pattern not found in {path}@{COMPOSITOR[:7]}")
        result["values"][name] = {"value": ty(m.group(1)), "file": path}
    json.dump(result, open(out, "w"), indent=1, sort_keys=True)
    open(out, "a").write("\n")

main()
