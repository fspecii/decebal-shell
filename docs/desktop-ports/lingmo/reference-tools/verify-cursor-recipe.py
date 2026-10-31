#!/usr/bin/python3 -I
# NOT AN INSTALLER FOR USERS. This is a verification script that checks the recipe in docs/desktop-ports/lingmo/reference-recipes.toml against the pinned upstream
# commit (tree hashes, link conversion, Xcursor validity) into a throwaway directory. Production installs use the shared asset installer
# (`decthemectl assets install`, contract docs/desktop-asset-packs.md) and the catalogue assets/desktop-ports/lingmo/asset-packs.toml. Never run it on a real icon directory.
"""Verification of the `lingmo-cursors` recipe in docs/desktop-ports/lingmo/reference-recipes.toml (a contract check, not the shared installer).

usage: install-cursor-pack.py --i-understand-the-licence-is-not-granted ASSETS_TOML GIT_CLONE DEST_ICONS_DIR

The upstream repository grants no licence (see sources.json), so this refuses to run without the explicit flag; the shared installer must
show the same uncertainty and act only on a user's request. Verifies the commit and the two source tree hashes with git plumbing, extracts
with `git archive` into a new temporary directory, copies each theme directory to DEST/<name>, then validates it: index.theme present, every
entry of cursors/ is an Xcursor file ("Xcur" magic) or a symlink that resolves inside cursors/, and `left_ptr` exists. Prints a JSON summary
with the nominal sizes found in `left_ptr`. Nothing outside DEST and the temp directory is written; no upstream script runs.
"""
import json, os, struct, subprocess, sys, tarfile, tempfile, tomllib

FLAG = "--i-understand-the-licence-is-not-granted"

def git(clone, *args):
    return subprocess.run(["git", "-C", clone, *args], capture_output=True, text=True, check=True).stdout.strip()

def sizes(path):
    with open(path, "rb") as f:
        head = f.read(16)
        if head[:4] != b"Xcur":
            return None
        _, _, _, ntoc = struct.unpack("<4I", head)
        out = set()
        for _ in range(ntoc):
            kind, subtype, _pos = struct.unpack("<3I", f.read(12))
            if kind == 0xFFFD0002:
                out.add(subtype)
        return sorted(out)

def main():
    args = [a for a in sys.argv[1:] if a != FLAG]
    if FLAG not in sys.argv[1:] or len(args) != 3:
        sys.exit(__doc__)
    assets, clone, dest = args
    pack = next(p for p in tomllib.load(open(assets, "rb"))["pack"] if p["id"] == "lingmo-cursors")
    commit = pack["commit"]
    git(clone, "cat-file", "-e", commit + "^{commit}")
    for path, tree in pack["source_trees"].items():
        got = git(clone, "rev-parse", f"{commit}:{path}")
        if got != tree:
            sys.exit(f"tree mismatch for {path}: {got} != {tree}")
    report = {}
    with tempfile.TemporaryDirectory() as tmp:
        arch = os.path.join(tmp, "a.tar")
        paths = [v["copy"][0].rstrip("/") for v in pack["variant"]]
        subprocess.run(["git", "-C", clone, "archive", "--format=tar", "-o", arch, commit, *paths], check=True)
        root = os.path.join(tmp, "x")
        os.mkdir(root)
        with tarfile.open(arch) as t:
            t.extractall(root, filter="data")
        for v in pack["variant"]:
            name = v["name"]
            out = os.path.join(dest, name)
            if os.path.exists(out):
                sys.exit(f"{out} exists")
            os.makedirs(dest, exist_ok=True)
            subprocess.run(["cp", "-a", os.path.join(root, v["copy"][0].rstrip("/")), out], check=True)
            cur = os.path.join(out, "cursors")
            files = links = bad = 0
            for entry in sorted(os.listdir(cur)):
                p = os.path.join(cur, entry)
                if os.path.islink(p):
                    target = os.path.realpath(p)
                    if os.path.dirname(target) == os.path.realpath(cur) and os.path.isfile(target):
                        links += 1
                    else:
                        bad += 1
                elif sizes(p) is not None:
                    files += 1
                else:
                    bad += 1
            report[name] = {"has_index_theme": os.path.isfile(os.path.join(out, "index.theme")), "xcursor_files": files, "symlinks_resolving": links, "invalid": bad,
                            "left_ptr_sizes": sizes(os.path.join(cur, "left_ptr"))}
    print(json.dumps({"commit": commit, "themes": report}))

main()
