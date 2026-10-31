#!/usr/bin/env python3
"""Checks assets/desktop-ports/gnome/asset-packs.toml against a downloaded source archive, emulating the documented install checks
(docs/desktop-asset-packs.md of the asset owner) without running anything from the archive.

Usage: python3 -I tests/e2e/gnome-asset-receipts.py CATALOGUE.toml SOURCE_ID ARCHIVE.tar.gz

Checks: archive sha256 == archive_sha256, pax comment == commit, every licence file exists, each pack's git tree OID (blobs 100644/100755/120000,
trees sorted git-style) == tree, and every symlink of a pack resolves lexically inside the pack or its link_sources.
"""
import hashlib
import posixpath
import sys
import tarfile
import tomllib


def blob(data):
    return hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()


def tree_oid(entries):
    """entries: {name: (mode, oid or nested dict)} -> git tree OID."""
    out = []
    for name, (mode, val) in entries.items():
        oid = tree_oid(val) if isinstance(val, dict) else val
        out.append((name + "/" if isinstance(val, dict) else name, mode, name, oid))
    body = b"".join(m.encode() + b" " + n.encode() + b"\0" + bytes.fromhex(o) for _, m, n, o in sorted(out, key=lambda e: e[0].encode()))
    return hashlib.sha1(b"tree %d\0" % len(body) + body).hexdigest()


def main():
    cat, source_id, archive = sys.argv[1:4]
    doc = tomllib.load(open(cat, "rb"))
    src = next(s for s in doc["source"] if s["id"] == source_id)
    raw = open(archive, "rb").read()
    ok = True

    def check(label, good, detail=""):
        nonlocal ok
        ok &= good
        print(("ok   " if good else "FAIL ") + label + (f" {detail}" if detail else ""))

    check("archive sha256", hashlib.sha256(raw).hexdigest() == src["archive_sha256"])
    strip = src.get("strip_components", 0)
    root = {}
    files, links = {}, {}
    with tarfile.open(archive, "r:gz") as tf:
        check("pax comment == commit", tf.pax_headers.get("comment") == src["commit"], tf.pax_headers.get("comment", ""))
        for m in tf:
            parts = m.name.strip("/").split("/")[strip:]
            if not parts or parts == [""]:
                continue
            rel = "/".join(parts)
            if m.isdir():
                continue
            if m.issym():
                links[rel] = m.linkname
                node = (("120000"), blob(m.linkname.encode()))
            elif m.isfile():
                data = tf.extractfile(m).read()
                files[rel] = len(data)
                node = ("100755" if m.mode & 0o111 else "100644", blob(data))
            else:
                continue
            d = root
            for p in parts[:-1]:
                d = d.setdefault(p, ("40000", {}))[1]
            d[parts[-1]] = node
    for lic in src["license_files"]:
        check(f"licence file {lic}", lic in files)
    for pack in (p for p in doc["pack"] if p["source"] == source_id):
        node = root
        for p in [] if pack["path"] == "." else pack["path"].split("/"):
            node = node[p][1]
        check(f"tree {pack['id']}", tree_oid(node) == pack["tree"], tree_oid(node))
        allowed = [pack["path"]] + pack.get("link_sources", [])
        bad = 0
        for rel, target in links.items():
            if not rel.startswith(pack["path"] + "/"):
                continue
            if target.startswith("/"):
                bad += 1
                continue
            dest = posixpath.normpath(posixpath.join(posixpath.dirname(rel), target))
            if not any(dest == a or dest.startswith(a + "/") for a in allowed):
                bad += 1
        check(f"symlinks of {pack['id']} stay inside {allowed}", bad == 0, f"{bad} escaping")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
