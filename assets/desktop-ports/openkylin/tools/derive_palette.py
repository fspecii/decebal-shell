#!/usr/bin/env python3
"""Derive Decebal `[colors.*]` blocks from the resolved UKUI token tables (derived/tokens-*.json).

Usage: python3 -I derive_palette.py DERIVED_DIR THEME MODE      (THEME: v11 | Light-Seeking | HeYin | Classic)
Prints a TOML fragment. The mapping is the documented table in docs/desktop-ports/openkylin/palette.md; alpha tokens are
composited over the role's own backdrop so every output is an opaque #rrggbb (Decebal colours have no alpha).
"""
import json, re, sys, pathlib

def rgba(s):
    m = re.fullmatch(r"rgba\(\s*(\d+),\s*(\d+),\s*(\d+),\s*([0-9.]+)\s*\)", s.strip())
    if m: return int(m[1]), int(m[2]), int(m[3]), float(m[4])
    m = re.fullmatch(r"#([0-9a-fA-F]{6})", s.strip())
    if m: v = int(m[1], 16); return v >> 16, v >> 8 & 255, v & 255, 1.0
    # `linear-gradient(0deg, rgba(W,..,a) 0%, rgba(W,..,a) 100%), rgba(base)`: a flat wash over a base colour
    m = re.fullmatch(r"linear-gradient\(0deg, (rgba\([^)]*\)) 0%, rgba\([^)]*\) 100%\), (rgba\([^)]*\))", s.strip())
    if m:
        wash, base = rgba(m[1]), rgba(m[2]); return over(wash, base)
    raise ValueError(s)

def over(fg, bg):
    a = fg[3]; return tuple(round(fg[i] * a + bg[i] * (1 - a)) for i in range(3)) + (1.0,)

def hexs(c): return "#%02x%02x%02x" % tuple(int(x) for x in c[:3])

def lum(c):
    f = lambda v: (v / 255 / 12.92) if v / 255 <= 0.03928 else (((v / 255) + 0.055) / 1.055) ** 2.4
    return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])

def contrast(a, b):
    la, lb = sorted((lum(a), lum(b)), reverse=True); return (la + 0.05) / (lb + 0.05)

def base_of(s):
    """The solid colour behind a UKUI state token (for `linear-gradient(wash), rgba(base)` states the base itself)."""
    m = re.search(r"rgba\([^)]*\)\s*$", s.strip())
    return rgba(m.group(0))

def lerp(a, b, t): return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3)) + (1.0,)

def at_least(fg, bg, ref, floor):
    """Move `fg` towards `ref` (the primary text colour) until it reaches `floor`:1 contrast on `bg`."""
    for i in range(101):
        c = lerp(fg, ref, i / 100)
        if contrast(c, bg) >= floor: return c
    return ref

def derive(T, mode, flat=False):
    """flat: use the solid brand/semantic colours instead of the wash states (UKUI Classic washes its highlights white/black,
    which reads as a selection fill, not as an accent colour on text and thin lines)."""
    g = lambda k: rgba(T[k])
    window, base = g("window-active"), g("base-active")
    text = over(g("kfont-primary"), window)
    sec = over(g("kfont-secondary"), window)
    menu = over(g("kmenu"), window) if T.get("kmenu") not in (None, "None") else window
    sem = (lambda k: base_of(T[k])) if flat else (lambda k: over(g(k), window))
    accent = base_of(T["kbrand1"]) if flat else over(g("kbrand-normal"), window)
    on_accent = over(g("highlightedtext-active"), accent) if not T["highlightedtext-active"].startswith("var(") else (255, 255, 255, 1.0)
    if flat or contrast(on_accent, accent) < 3.0:
        on_accent = max([(255, 255, 255, 1.0), (0, 0, 0, 1.0)], key=lambda c: contrast(c, accent))
    return {
        "background": window,
        "sidebar": over(g("kcontain-secondary-normal"), window) if mode == "dark" else g("kgray-3"),
        "surface": base,
        "bar": window,
        "popup": menu,
        "dock": window,
        "overlay": window,   # the start menu and the sidebar are ordinary UKUI windows: window colour x transparency, not the menu colour
        "titlebar": window,
        "titlebar_inactive": g("window-inactive"),
        "border": over(g("kdivider"), window),
        "text": text,
        "text_secondary": at_least(sec, window, text, 4.5),
        "text_faint": at_least(sec, window, text, 3.0),
        "bar_text": text,
        "accent": accent,
        "accent_text": on_accent,
        "danger": sem("kerror-normal"),
        "ok": sem("ksuccess-normal"),
        "warn": sem("kwarning-normal"),
    }

if __name__ == "__main__":
    d, theme, mode = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
    T = json.load(open(d / f"tokens-{theme}-{mode}.json"))
    c = derive(T, mode, flat=(theme == 'Classic'))
    # text_faint: UKUI's secondary text is 40-65 % alpha; keep it as the role but never below 3:1 on the background
    for k, v in c.items(): print(f'{k} = "{hexs(v)}"')
    print(f"# contrast text/background {contrast(c['text'], c['background']):.2f}, text_secondary/background {contrast(c['text_secondary'], c['background']):.2f}, accent_text/accent {contrast(c['accent_text'], c['accent']):.2f}")
