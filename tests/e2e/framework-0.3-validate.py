#!/usr/bin/env python3
"""Validates the PROPOSED 0.3-framework manifests (tests/fixtures/gnome-theme-ports/framework-0.3/*.theme.toml) with a decthemectl of the 0.3 framework.
Usage: python3 tests/e2e/framework-0.3-validate.py DECTHEMECTL_0_3 [THEMES_DIR]

Each package folder is copied into a private directory with its theme.toml replaced by the proposed one, then `decthemectl theme validate` runs with a
private HOME/XDG and no display or bus. Dual-engine limits, reported, not hidden:
  * a 0.2.x engine refuses `[bar] applets` and `[branding]` ("unknown key") and any 0.3.0 floor; it can only validate the shipped manifests;
  * a 0.3 engine from before the GNOME Control Center look (e.g. universal-panel `71e3434`) knows the framework keys but not `style = "gnome"`; when
    it says so, a copy with style = "card" is validated instead and the substitution is printed;
  * only an engine that has BOTH (the merged tree) validates the proposed manifests exactly as written.
"""
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parents[2]


def control_center_card(text):
    """Swaps only `[control_center] style = "gnome"`; the `[launcher]` style of the same name is a different, valid component."""
    swapped = re.sub(r'(?ms)(^\[control_center\]\n(?:(?!^\[).*\n)*?)style = "gnome"', r'\1style = "card"', text, count=1)
    assert swapped != text, "no [control_center] style = \"gnome\" to replace"
    return swapped


def main():
    ctl = Path(sys.argv[1]).resolve()
    themes = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else HERE / "themes"
    failed = []
    with tempfile.TemporaryDirectory(prefix="framework-validate-") as tmp:
        home = Path(tmp) / "home"
        home.mkdir()
        env = {"PATH": "/usr/bin:/bin", "HOME": str(home), "XDG_CONFIG_HOME": str(home), "XDG_CACHE_HOME": str(home), "DECEBAL_CONFIG": str(home)}
        for proposed in sorted((HERE / "tests/fixtures/gnome-theme-ports/framework-0.3").glob("*.theme.toml")):
            ident = proposed.name.removesuffix(".theme.toml")
            for variant, rewrite in (("as written", None), ("style=card", control_center_card)):
                pkg = Path(tmp) / variant.replace(" ", "_").replace("=", "_") / ident
                shutil.copytree(themes / ident, pkg)
                text = proposed.read_text()
                (pkg / "theme.toml").write_text(rewrite(text) if rewrite else text)
                run = subprocess.run([str(ctl), "theme", "validate", str(pkg)], capture_output=True, text=True, env=env, timeout=120)
                out = (run.stdout + run.stderr).strip().splitlines()
                print(f"{ident} [{variant}]: exit {run.returncode}: {out[-1] if out else ''}")
                if variant == "as written" and run.returncode != 0:
                    failed.append(ident)
    sys.exit(bool(failed))


if __name__ == "__main__":
    main()
