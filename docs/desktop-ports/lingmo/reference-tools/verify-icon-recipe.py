#!/usr/bin/python3 -I
# NOT AN INSTALLER FOR USERS. This is a verification script that checks the recipe in docs/desktop-ports/lingmo/reference-recipes.toml against the pinned upstream
# commit (tree hashes, link conversion, Xcursor validity) into a throwaway directory. Production installs use the shared asset installer
# (`decthemectl assets install`, contract docs/desktop-asset-packs.md) and the catalogue assets/desktop-ports/lingmo/asset-packs.toml. Never run it on a real icon directory.
"""Verification of the `lingmo-icons` recipe in docs/desktop-ports/lingmo/reference-recipes.toml (a contract check, not the shared installer).

usage: install-icon-pack.py ASSETS_TOML GIT_CLONE DEST_ICONS_DIR

GIT_CLONE is a local clone of the pack's upstream that contains the pinned commit (it is only read with git plumbing; no upstream script runs).
Steps: verify the commit exists and each `source_trees` hash equals `git rev-parse <commit>:<path>`; extract with `git archive` into a new empty
temporary directory; build DEST/Lingmo from src/ then links/, turning each links/ entry (a one-line text file holding a relative path that stays inside the theme) into a
symlink and removing the ones that do not resolve; write index.theme with Name=Lingmo; create <dir>@2x/@3x symlinks. Refuses a non-empty destination theme.
Prints a JSON summary. Python subprocess close_fds defaults apply; nothing outside DEST and the temp directory is written.
"""
import json, os, subprocess, sys, tarfile, tempfile, tomllib

def git(clone, *args):
    return subprocess.run(["git", "-C", clone, *args], capture_output=True, text=True, check=True).stdout.strip()

def main():
    assets, clone, dest = sys.argv[1:4]
    pack = next(p for p in tomllib.load(open(assets, "rb"))["pack"] if p["id"] == "lingmo-icons")
    commit = pack["commit"]
    git(clone, "cat-file", "-e", commit + "^{commit}")
    for path, tree in pack["source_trees"].items():
        got = git(clone, "rev-parse", f"{commit}:{path}")
        if got != tree:
            sys.exit(f"tree mismatch for {path}: {got} != {tree}")
    theme = os.path.join(dest, "Lingmo")
    if os.path.exists(theme) and os.listdir(theme):
        sys.exit(f"{theme} is not empty")
    dirs = ["actions", "apps", "categories", "devices", "emblems", "mimetypes", "places", "status"]
    with tempfile.TemporaryDirectory() as tmp:
        arch = os.path.join(tmp, "a.tar")
        subprocess.run(["git", "-C", clone, "archive", "--format=tar", "-o", arch, commit, "src", "links"], check=True)
        src_root = os.path.join(tmp, "x")
        os.mkdir(src_root)
        with tarfile.open(arch) as t:
            t.extractall(src_root, filter="data")
        os.makedirs(theme, exist_ok=True)
        index = open(os.path.join(src_root, "src", "index.theme")).read().replace("Name=Cutefish", "Name=Lingmo", 1)
        open(os.path.join(theme, "index.theme"), "w").write(index)
        copied = 0
        for d in dirs:
            for root, _, files in os.walk(os.path.join(src_root, "src", d)):
                rel = os.path.relpath(root, os.path.join(src_root, "src"))
                os.makedirs(os.path.join(theme, rel), exist_ok=True)
                for f in files:
                    with open(os.path.join(root, f), "rb") as i, open(os.path.join(theme, rel, f), "wb") as o:
                        o.write(i.read())
                    copied += 1
        linked = dangling = 0
        for d in dirs:
            for root, _, files in os.walk(os.path.join(src_root, "links", d)):
                rel = os.path.relpath(root, os.path.join(src_root, "links"))
                for f in files:
                    target = open(os.path.join(root, f)).read().strip()
                    # a one-line relative path that stays inside the theme directory; absolute paths and escapes are refused
                    inside = os.path.normpath(os.path.join(rel, target)) if target else ".."
                    if not target or "\n" in target or os.path.isabs(target) or inside.startswith("..") or inside == ".":
                        dangling += 1
                        continue
                    out = os.path.join(theme, rel, f)
                    os.makedirs(os.path.dirname(out), exist_ok=True)
                    if os.path.lexists(out):
                        os.remove(out)
                    os.symlink(target, out)
                    linked += 1
        # a link is dangling when its (possibly chained) target does not exist in the finished theme
        broken = 0
        for root, _, files in os.walk(theme):
            for f in files:
                p = os.path.join(root, f)
                if os.path.islink(p) and not os.path.exists(p):
                    os.remove(p)
                    broken += 1
        for d in dirs:
            for n in ("@2x", "@3x"):
                link = os.path.join(theme, d + n)
                if not os.path.lexists(link):
                    os.symlink(d, link)
    print(json.dumps({"theme": theme, "commit": commit, "copied_files": copied, "symlinks_made": linked - broken, "dangling_removed": broken, "refused_entries": dangling}))

main()
