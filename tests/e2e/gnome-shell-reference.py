#!/usr/bin/env python3
"""Builds tests/fixtures/gnome-theme-ports/gnome-shell-51.1-quick-settings-reference.json from the pinned gnome-shell stylesheet COMPILED by the
compiler upstream uses (sassc/libsass), so the numbers do not depend on anyone's reading of the SCSS.

Usage: python3 tests/e2e/gnome-shell-reference.py DARK_CSS LIGHT_CSS PROVENANCE_JSON OUT_JSON

DARK_CSS / LIGHT_CSS: `sassc -a gnome-shell-{dark,light}.scss …` output (see docs/gnome-theme-ports.md, "Source reference").
PROVENANCE_JSON: the commit, the package filenames/hashes and signature checks recorded when the tool was fetched.
Nothing is executed from the downloaded material; the CSS is only parsed as text.

The compiled CSS still contains the Shell's run-time colour functions (st-mix, st-lighten, st-darken, st-transparentize: St evaluates them in
src/st/st-theme-node.c). They are evaluated here with a port of exactly that C code (opaque colours: `mix` weights the FIRST colour by the factor,
`lighten`/`darken` move the HSL lightness by the factor), with the accent inputs `-st-accent-color`/`-st-accent-fg-color` set to libadwaita's default blue.
"""
import colorsys
import hashlib
import json
from pathlib import Path
import re
import sys

ACCENT = (0x35, 0x84, 0xE4)
ACCENT_FG = (255, 255, 255)
EM_PX = 11.0 * 96.0 / 72.0  # 1em = 11 pt at 96 dpi (`$base_font_size`)


def rules(css):
    css = re.sub(r"/\*.*?\*/", "", css, flags=re.S)
    out = []
    for selectors, body in re.findall(r"([^{}]+)\{([^{}]*)\}", css):
        decls = {}
        for d in body.split(";"):
            if ":" in d:
                k, v = d.split(":", 1)
                decls[k.strip()] = " ".join(v.split())  # the last declaration of a property wins, as in CSS
        out.append(([" ".join(s.split()) for s in selectors.split(",")], decls))
    return out


def declarations(css, selector):
    """Merged declarations of every rule whose selector list contains exactly `selector` (later rules override)."""
    merged = {}
    for selectors, decls in rules(css):
        if selector in selectors:
            merged.update(decls)
    return merged


# ---- St colour functions (src/st/st-theme-node.c): opaque colours, floats, bytes at the end
def parse_args(text):
    args, depth, cur = [], 0, ""
    for ch in text:
        if ch == "," and depth == 0:
            args.append(cur.strip())
            cur = ""
            continue
        depth += ch == "("
        depth -= ch == ")"
        cur += ch
    args.append(cur.strip())
    return args


def evaluate(expr):
    expr = expr.strip()
    if expr == "-st-accent-color":
        return ACCENT
    if expr == "-st-accent-fg-color":
        return ACCENT_FG
    if expr.startswith("#"):
        h = expr[1:]
        if len(h) == 3:
            h = "".join(c * 2 for c in h)
        return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))
    m = re.match(r"^(st-[a-z]+)\((.*)\)$", expr, re.S)
    if not m:
        raise ValueError(f"unsupported colour expression {expr!r}")
    name, args = m.group(1), parse_args(m.group(2))
    base = evaluate(args[0])
    if name == "st-mix":
        other, factor = evaluate(args[1]), float(args[2].rstrip("%")) / 100.0
        factor = 1 - factor  # "SCSS mix() inverts the factor"
        return tuple(max(0, min(255, int(a + (b - a) * factor))) for a, b in zip(base, other))
    if name in ("st-lighten", "st-darken"):
        amount = float(args[1].rstrip("%")) / 100.0
        amount = amount if name == "st-lighten" else -amount
        h, l, s = colorsys.rgb_to_hls(*[v / 255.0 for v in base])
        r, g, b = colorsys.hls_to_rgb(h, max(0.0, min(1.0, l + amount)), s)
        return tuple(max(0, min(255, int(round(v * 255)))) for v in (r, g, b))
    raise ValueError(f"unsupported function {name}")


def colour(expr):
    try:
        return "#%02x%02x%02x" % evaluate(expr)
    except ValueError:
        return None


STATES = [("normal", ""), ("hover", ":hover"), ("active", ":active"), ("checked", ":checked"), ("checked-hover", ":hover:checked"), ("checked-active", ":active:checked"), ("insensitive", ":insensitive")]
COLOUR_SELECTORS = {
    # a quick toggle is a St.Button with style classes `button quick-toggle`: its unchecked colours are `.button`'s, its checked ones `.quick-toggle:checked`'s
    "button": ".button",
    "toggle": ".quick-toggle",
    "menu-button": ".quick-toggle-has-menu .quick-toggle-menu-button",
    "icon-button": ".quick-settings .icon-button",
    "message-card": ".message",
}
LENGTHS = {
    ".quick-settings": ["padding", "border-radius"],
    ".quick-settings-grid": ["spacing-rows", "spacing-columns"],
    ".quick-toggle": ["border-radius", "min-width", "max-width", "min-height"],
    ".quick-toggle > StBoxLayout": ["spacing", "padding"],
    ".quick-toggle:ltr > StBoxLayout": ["padding-left"],
    ".quick-toggle .quick-toggle-icon": ["icon-size"],
    ".quick-toggle .quick-toggle-title": ["font-weight", "font-size"],
    ".quick-toggle .quick-toggle-subtitle": ["font-weight", "font-size"],
    ".quick-toggle-has-menu .quick-toggle:ltr > StBoxLayout": ["padding-right"],
    ".quick-toggle-has-menu .quick-toggle:ltr": ["border-radius"],
    ".quick-toggle-has-menu .quick-toggle-menu-button": ["padding"],
    ".quick-toggle-has-menu .quick-toggle-menu-button:ltr": ["border-radius"],
    ".quick-toggle-has-menu .quick-toggle-separator": ["width"],
    ".quick-slider > StBoxLayout": ["spacing"],
    ".quick-slider .icon-button": ["padding"],
    ".quick-slider .slider-bin": ["padding", "border-radius"],
    ".quick-settings .icon-button": ["padding"],
    ".quick-settings-system-item > StBoxLayout": ["spacing"],
    ".slider": ["-barlevel-height", "-slider-handle-radius"],
    ".icon-button": ["border-radius", "min-height", "padding"],
    ".message": ["border-radius", "padding"],
    ".popup-menu-content": ["border-radius", "padding"],
}


def px(value):
    """CSS length in px at 100 % (em = 11 pt = 14.667 px; a `a b c d` shorthand returns the list)."""
    out = []
    for token in value.split():
        if token.endswith("px"):
            out.append(round(float(token[:-2]), 4))
        elif token.endswith("em"):
            out.append(round(float(token[:-2]) * EM_PX, 4))
        else:
            out.append(token)
    return out[0] if len(out) == 1 else out


def main():
    dark_css, light_css = (Path(p).read_text() for p in sys.argv[1:3])
    prov = json.loads(Path(sys.argv[3]).read_text())
    ref = {
        "format": 1,
        "what": "gnome-shell 51.1 quick-settings facts from the pinned stylesheet compiled by sassc/libsass",
        "provenance": prov | {"css_sha256": {"dark": hashlib.sha256(dark_css.encode()).hexdigest(), "light": hashlib.sha256(light_css.encode()).hexdigest()}},
        "units": {"em_px": EM_PX, "note": "1em = 11 pt at 96 dpi; lengths below are px at 100 % text scale"},
        "accent_inputs": {"-st-accent-color": "#3584e4", "-st-accent-fg-color": "#ffffff"},
        "lengths": {},
        "colours": {"dark": {}, "light": {}},
        "colour_expressions": {"dark": {}, "light": {}},
    }
    for selector, props in LENGTHS.items():
        decls = declarations(dark_css, selector)
        ref["lengths"][selector] = {p: {"css": decls[p], "px": px(decls[p])} for p in props if p in decls}
    for variant, css in (("dark", dark_css), ("light", light_css)):
        for group, selector in COLOUR_SELECTORS.items():
            for state, suffix in STATES:
                decls = declarations(css, selector + suffix)
                for prop in ("background-color", "color"):
                    if prop in decls:
                        key = f"{group}/{state}/{prop}"
                        ref["colour_expressions"][variant][key] = decls[prop]
                        value = colour(decls[prop])
                        if value:
                            ref["colours"][variant][key] = value
        for sel, key in ((".quick-toggle-has-menu .quick-toggle-separator", "separator"), (".quick-toggle-has-menu:checked .quick-toggle-separator", "separator-checked")):
            expr = declarations(css, sel).get("background-color")
            if expr:
                ref["colour_expressions"][variant][key] = expr
        focus = declarations(css, ".button:focus")
        ref["colour_expressions"][variant]["focus/box-shadow"] = focus.get("box-shadow")
        panel = declarations(css, ".popup-menu-content")
        ref["colour_expressions"][variant]["panel/background-color"] = panel.get("background-color")
        ref["colour_expressions"][variant]["panel/border"] = panel.get("border")
    Path(sys.argv[4]).write_text(json.dumps(ref, indent=2, sort_keys=True) + "\n")
    print(f"{len(ref['colours']['dark'])} dark and {len(ref['colours']['light'])} light colours, {sum(len(v) for v in ref['lengths'].values())} lengths")


if __name__ == "__main__":
    main()
