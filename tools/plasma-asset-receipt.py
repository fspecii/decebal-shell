#!/usr/bin/python3
"""Receipts for the Plasma asset-pack catalogue: an emulation of the verification steps in docs/desktop-asset-packs.md
(asset-owner branch feat/claude-desktop-asset-packs, commit 76b9285). It is NOT the contract's installer, which was not committed
when this was written; its content_hash is computed with the documented formula over the documented file filter and must be
re-checked with `decthemectl assets verify` once that installer exists.

usage: plasma-asset-receipt.py CATALOGUE.toml DOWNLOAD_DIR PREFIX OUT.json
  DOWNLOAD_DIR holds the archives already downloaded from each source's archive_url (this script never touches the network).
  PREFIX is a private prefix (under ~/.cache or /var/tmp); packs are staged as PREFIX/share/icons/<install_name>.
Run with `python3 -I`. Nothing outside PREFIX is written and no upstream script is executed.
"""
import hashlib, json, os, shutil, subprocess, sys, tarfile, tempfile, tomllib

TEXT = ("README", "COPYING", "LICENSE", "AUTHORS")


def sha256_file(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def keep(rel, path):
    """The documented filter for icon/cursor packs: icon images, theme files, Xcursor files under cursors/, text."""
    base = os.path.basename(rel)
    if base in ("index.theme", "cursor.theme") or base.lower().endswith((".svg", ".png", ".xpm", ".md", ".txt")) or base.startswith(TEXT):
        return True
    if rel.startswith("cursors/"):
        with open(path, "rb") as f:
            return f.read(4) == b"Xcur"
    return False


def content_hash(root):
    entries = []
    for dp, dn, fn in os.walk(root):
        for n in dn + fn:
            p = os.path.join(dp, n)
            rel = os.path.relpath(p, root).replace(os.sep, "/")
            if rel == ".decebal-asset.json":
                continue
            if os.path.islink(p):
                entries.append((rel, f"{rel}\0L\0{os.readlink(p)}\n".encode()))
            elif os.path.isfile(p):
                entries.append((rel, f"{rel}\0F\0{os.path.getsize(p)}\0{sha256_file(p)}\n".encode()))
    h = hashlib.sha256()
    for _, e in sorted(entries):
        h.update(e)
    return "sha256:" + h.hexdigest()


def safe(name):
    parts = name.split("/")
    return name and not name.startswith("/") and "\\" not in name and all(p not in ("", ".", "..") for p in parts) and all(ord(c) >= 32 for c in name)


def extract(archive, strip, wanted, dest):
    """Extract only the wanted path prefixes, refusing anything unsafe; returns the member names kept."""
    got = []
    with tarfile.open(archive, "r:gz") as tf:
        for m in tf:
            if m.name == "pax_global_header" or m.ischr() or m.isblk() or m.isfifo():
                continue
            parts = m.name.split("/")[strip:]
            rel = "/".join(parts)
            if not rel or not any(rel == w or rel.startswith(w.rstrip("/") + "/") for w in wanted):
                continue
            if not safe(rel):
                raise SystemExit(f"unsafe member {m.name!r}")
            out = os.path.join(dest, rel)
            if m.isdir():
                os.makedirs(out, exist_ok=True)
            elif m.issym():
                if os.path.isabs(m.linkname) or ".." in m.linkname.split("/"):
                    raise SystemExit(f"link escapes the source: {m.name} -> {m.linkname}")
                os.makedirs(os.path.dirname(out), exist_ok=True)
                os.symlink(m.linkname, out)
            elif m.isfile():
                os.makedirs(os.path.dirname(out), exist_ok=True)
                with open(out, "wb") as f:
                    f.write(tf.extractfile(m).read())
                os.chmod(out, 0o755 if m.mode & 0o111 else 0o644)
            else:
                raise SystemExit(f"unsupported member {m.name!r}")
            got.append(rel)
    return got


def git_tree(folder):
    """The git tree OID of `folder`'s contents, computed by git itself in a throw-away repository (no hooks, no user config)."""
    gd = tempfile.mkdtemp(dir=os.path.dirname(folder))
    env = {"PATH": os.environ["PATH"], "HOME": gd, "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1"}
    git = lambda *a: subprocess.run(["git", "--git-dir", os.path.join(gd, "g"), "--work-tree", folder, *a], env=env, check=True, capture_output=True, text=True).stdout.strip()
    subprocess.run(["git", "init", "-q", "--bare", os.path.join(gd, "g")], env=env, check=True, capture_output=True)
    git("add", "-A", "-f")
    oid = git("write-tree")
    shutil.rmtree(gd)
    return oid


def main():
    if sys.argv[1] == "hash":  # plasma-asset-receipt.py hash DIR...  -> content hash and counts, same formula as the contract
        for d in sys.argv[2:]:
            files = sum(1 for dp, _, fn in os.walk(d) for n in fn if not os.path.islink(os.path.join(dp, n)))
            links = sum(1 for dp, dn, fn in os.walk(d) for n in dn + fn if os.path.islink(os.path.join(dp, n)))
            print(json.dumps({"dir": d, "files": files, "symlinks": links, "content_hash": content_hash(d)}))
        return
    cat_path, dl, prefix, out = sys.argv[1:5]
    cat = tomllib.load(open(cat_path, "rb"))
    assert cat["format"] == 1
    if not prefix.startswith((os.path.expanduser("~/.cache/"), "/var/tmp/")):
        raise SystemExit("prefix must be private (~/.cache or /var/tmp)")
    shutil.rmtree(prefix, ignore_errors=True)
    os.makedirs(prefix)
    sources = {s["id"]: s for s in cat["source"]}
    receipts = {"catalogue": os.path.basename(cat_path), "emulated_contract": "docs/desktop-asset-packs.md @ 76b9285", "sources": {}, "packs": {}}
    work = tempfile.mkdtemp(dir=prefix)
    for sid, s in sources.items():
        arch = os.path.join(dl, os.path.basename(s["archive_url"]))
        got = sha256_file(arch)
        pax = subprocess.run(["sh", "-c", f"gzip -dc '{arch}' | git get-tar-commit-id"], capture_output=True, text=True).stdout.strip()
        receipts["sources"][sid] = {"archive": os.path.basename(arch), "bytes": os.path.getsize(arch), "archive_sha256": got, "archive_sha256_matches_catalogue": got == s["archive_sha256"],
                                    "pax_commit": pax, "pax_commit_matches_catalogue": pax == s["commit"], "commit": s["commit"]}
        assert got == s["archive_sha256"], f"{sid}: archive hash mismatch"
        assert pax == s["commit"], f"{sid}: archive commit mismatch"
    for p in cat["pack"]:
        s = sources[p["source"]]
        arch = os.path.join(dl, os.path.basename(s["archive_url"]))
        lic = list(dict.fromkeys(s["license_files"] + p.get("license_files", [])))
        src = os.path.join(work, p["id"])
        extract(arch, s.get("strip_components", 0), [p["path"]] + lic, src)
        # the tree OID of `path`: stage just that folder as the repository root of a throw-away repo
        only = os.path.join(work, p["id"] + ".tree")
        shutil.copytree(os.path.join(src, p["path"]), only, symlinks=True)
        oid = git_tree(only)
        receipts_tree = {"expected": p["tree"], "computed": oid, "matches": oid == p["tree"]}
        assert oid == p["tree"], f"{p['id']}: tree {oid} != {p['tree']}"
        dest = os.path.join(prefix, "share", "icons", p.get("install_name", p["path"].rsplit("/", 1)[-1]))
        os.makedirs(dest)
        kept = skipped = links = 0
        for dp, dn, fn in os.walk(only):
            for n in sorted(fn):
                full = os.path.join(dp, n)
                rel = os.path.relpath(full, only).replace(os.sep, "/")
                os.makedirs(os.path.dirname(os.path.join(dest, rel)), exist_ok=True)
                if os.path.islink(full):
                    tgt = os.readlink(full)
                    if os.path.isabs(tgt) or ".." in tgt.split("/"):
                        raise SystemExit(f"{rel}: unsafe link target {tgt}")
                    os.symlink(tgt, os.path.join(dest, rel))
                    links += 1
                elif keep(rel, full):
                    shutil.copyfile(full, os.path.join(dest, rel))
                    os.chmod(os.path.join(dest, rel), 0o644)
                    kept += 1
                else:
                    skipped += 1
        os.makedirs(os.path.join(dest, "LICENSES"))
        for l in lic:
            shutil.copyfile(os.path.join(src, l), os.path.join(dest, "LICENSES", l.replace("/", "_")))
        # cursor packs need a core pointer (left_ptr or default)
        core = [c for c in ("left_ptr", "default") if os.path.lexists(os.path.join(dest, "cursors", c))]
        receipts["packs"][p["id"]] = {"install_name": os.path.basename(dest), "provides": p["provides"], "path": p["path"], "tree": receipts_tree, "files_kept": kept, "symlinks": links,
                                      "files_skipped_by_filter": skipped, "core_pointer": core, "license": p.get("license", s["license"]), "license_files": lic,
                                      "content_hash": content_hash(dest), "staged_at": os.path.relpath(dest, prefix)}
    shutil.rmtree(work)
    json.dump(receipts, open(out, "w"), indent=1)
    print(json.dumps({k: {"tree_ok": v["tree"]["matches"], "kept": v["files_kept"], "links": v["symlinks"], "skipped": v["files_skipped_by_filter"], "core": v["core_pointer"]} for k, v in receipts["packs"].items()}, indent=1))


main()
