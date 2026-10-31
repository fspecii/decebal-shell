#!/usr/bin/env python3
"""Read an upstream `cursors.sh` AS TEXT and return the cursor tables (data only; never run, never builds anything).

The shared decebal installer builds the Xcursor files natively from these tables (`[[pack.xcursor]]` in asset-packs.toml). This module
only extracts facts with strict regular expressions, so a hostile script can change data but cannot execute or write anything.
"""
import re

NAME = r"[A-Za-z0-9][A-Za-z0-9_.+-]{0,63}"
ECHO = re.compile(r'^\s*echo\s+"\s*(\d{1,3})\s+(\d{1,3})\s+(\d{1,3})\s+(?:\.\./)?(\d{1,3}x\d{1,3}/' + NAME + r'\.png)\s*(\d{0,5})\s*"\s*(>>?)\s*cursors/(' + NAME + r')\.in\s*$')
BUILD = re.compile(r"^\s*xcursorgen\s+(" + NAME + r")\.in\s+(" + NAME + r")\s*$")
LINK = re.compile(r"^\s*ln\s+-sf?\s+(" + NAME + r")\s+(" + NAME + r")\s*$")

# CSS / freedesktop cursor-spec names -> the legacy X names the UKUI themes ship (additive: only names the theme lacks)
CSS_ALIASES = {
    "default": "left_ptr", "pointer": "hand2", "text": "xterm", "wait": "watch", "progress": "left_ptr_watch", "not-allowed": "crossed_circle",
    "no-drop": "dnd-none", "grab": "hand2", "col-resize": "sb_h_double_arrow", "row-resize": "sb_v_double_arrow", "ew-resize": "h_double_arrow",
    "ns-resize": "v_double_arrow", "nesw-resize": "fd_double_arrow", "nwse-resize": "bd_double_arrow", "context-menu": "left_ptr", "cell": "plus",
    "alias": "link", "e-resize": "right_side", "w-resize": "left_side", "n-resize": "top_side", "s-resize": "bottom_side",
    "ne-resize": "top_right_corner", "nw-resize": "top_left_corner", "se-resize": "bottom_right_corner", "sw-resize": "bottom_left_corner",
}


def parse(text):
    """-> (built: ordered names, frames: name -> [(size, xhot, yhot, file, delay)], links: [(target, alias)])."""
    frames, built, links = {}, [], []
    for line in text.splitlines():
        m = ECHO.match(line)
        if m:
            size, xh, yh, f, delay, op, name = m.groups()
            if op == ">" and name in frames: frames[name] = []        # `>` starts the file over, `>>` appends
            frames.setdefault(name, []).append((int(size), int(xh), int(yh), f, int(delay or 0)))
            continue
        m = BUILD.match(line)
        if m and m.group(1) == m.group(2) and m.group(1) not in built: built.append(m.group(1)); continue
        m = LINK.match(line)
        if m: links.append((m.group(1), m.group(2)))
    missing = [n for n in built if n not in frames]
    if missing: raise ValueError(f"no frame lines for {missing}")
    return built, frames, links


def toml_tables(text, css=True):
    built, frames, links = parse(text)
    aliases = {}
    for target, alias in links:
        if alias not in built: aliases.setdefault(target, []).append(alias)
    have = set(built) | {a for v in aliases.values() for a in v}
    if css:
        for name, target in CSS_ALIASES.items():
            if name not in have and (target in built or any(target in v for v in aliases.values())):
                tgt = target if target in built else next(t for t, v in aliases.items() if target in v)
                aliases.setdefault(tgt, []).append(name); have.add(name)
    out = []
    for n in built:
        lines = "\n".join(f"{s} {x} {y} {f}" + (f" {d}" if d else "") for s, x, y, f, d in frames[n])
        out.append(f'[[pack.xcursor]]\nname = "{n}"\nconfig = """\n{lines}\n"""\n' + (("aliases = [" + ", ".join(f'"{a}"' for a in aliases[n]) + "]\n") if n in aliases else ""))
    return "\n".join(out), len(built), sum(len(v) for v in aliases.values())
