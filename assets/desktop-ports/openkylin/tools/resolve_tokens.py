#!/usr/bin/env python3
"""Resolve the UKUI global-theme CSS token tables (var() chains) into flat JSON.

Usage: python3 -I resolve_tokens.py UKUI_GLOBALTHEME_CHECKOUT OUT_DIR

Reads only data files (theme.conf, k*-{light,dark}.css); runs nothing from the checkout.
Independent implementation: the token *values* are facts about the design, nothing is copied as code.
"""
import json, re, sys, pathlib, configparser, hashlib, subprocess

root = pathlib.Path(sys.argv[1]); out = pathlib.Path(sys.argv[2]); out.mkdir(parents=True, exist_ok=True)
gt = root / "globaltheme"
rev = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()

decl = re.compile(r"--([a-z0-9-]+)\s*:\s*([^;]+);", re.I)
var = re.compile(r"var\(--([a-z0-9-]+)\)", re.I)


def resolve(table, key, seen=()):
    v = table[key]
    def sub(m):
        k = m.group(1).lower()
        if k in seen or k not in table:
            return m.group(0)
        return resolve(table, k, seen + (key,))
    return var.sub(sub, v).strip()


summary = {"source_revision": rev, "themes": {}}
for tdir in sorted(p for p in gt.iterdir() if p.is_dir()):
    conf = configparser.ConfigParser(interpolation=None, strict=False)
    conf.optionxform = str
    conf.read(tdir / "theme.conf", encoding="utf-8")
    info = {s: dict(conf[s]) for s in conf.sections()}
    summary["themes"][tdir.name] = {"conf": info, "css": {}}
    for css in sorted(tdir.glob("*.css")):
        mode = "dark" if css.stem.endswith("dark") else "light"
        text = css.read_text(encoding="utf-8")
        table = {m.group(1).lower(): m.group(2).strip() for m in decl.finditer(text)}
        flat = {k: resolve(table, k) for k in sorted(table)}
        (out / f"tokens-{tdir.name}-{mode}.json").write_text(json.dumps(flat, indent=1, sort_keys=True) + "\n")
        summary["themes"][tdir.name]["css"][mode] = {
            "file": f"globaltheme/{tdir.name}/{css.name}", "sha256": hashlib.sha256(text.encode()).hexdigest(),
            "tokens": len(flat)}
(out / "summary.json").write_text(json.dumps(summary, indent=1, sort_keys=True, ensure_ascii=False) + "\n")
print(json.dumps({t: v["css"] for t, v in summary["themes"].items()}, indent=1))
