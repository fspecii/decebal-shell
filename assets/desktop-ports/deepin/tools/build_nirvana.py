#!/usr/bin/env python3
"""Convert Deepin's default icon theme `nirvana` (DCI containers) into a freedesktop PNG theme.

    python3 -I build_nirvana.py --upstream <deepin-desktop-theme checkout> --out <empty dir> [--size 128] [--limit N]

Independent implementation (MIT, see the repository LICENSE) of the *published* container format, the "DSG icon file
specification" (deepin-specifications `unstable/图标文件规范.md`, revision pinned in docs/desktop-ports/deepin/sources.json).
Nothing from dtkgui/dtkcore is used. The artwork is deepin-desktop-theme (UnionTech Software Technology Co., Ltd.,
CC-BY-4.0; `.reuse/dep5` lists `nirvana/*`): the output carries that notice.

Format facts used (all from the specification):
  * file header: magic "DCI", 1 byte version, 3 bytes (little endian) number of root entries;
  * entry: 1 byte type (1 file, 2 directory, 3 link), 63 bytes NUL-terminated UTF-8 name, 8 bytes content size, content;
    a directory's content is its entries; a link's content is the target path;
  * path inside an icon: <size>/<state>.<tone>/<scale>/<priority>.<format> (e.g. 256/normal.light/3/1.webp).

nirvana's files are one WebP per icon, mostly 256 x 3 = 768 px (state normal, tone light; `normal.dark` is a link to it). A layer is *plain*
only when its file name is `<prior>.webp`; the specification's padding, palette, colour-adjust and `.alpha8` fields need the palette engine, so
those icons, icons with several layers and any malformed size or scale directory are refused and listed under `skipped` with the reason (in the
pinned revision: three palette layers named `1.0.webp`, and `deepin-xdgicon-convert`, whose root directory declares 31,888 bytes but whose file
ends after 21,522: a truncated upstream file).

Trust: every DCI file is untrusted. Size, depth, entry-count, name, length, RIFF-size, size/scale-range and layer-name checks run before a byte
is used; ImageMagick runs with hard resource limits and an empty environment; nothing is executed from the upstream tree; the output directory is
new, mode 0700 and never overwritten. Provenance: each input file's bytes are hashed as a Git blob and compared with the object in the pinned
revision's tree (a link: its text, then the sibling it points to), the two licence files likewise, and any file in the directory that the revision
does not list is an untracked input. Any difference refuses the run; `--allow-working-tree` converts anyway and the manifest and NOTICE then say
"working-tree, NOT revision-verified". Output: <out>/nirvana/{index.theme, <size>/<name>.png, NOTICE.txt, LICENSE-CC-BY-4.0.txt, REUSE-dep5.txt}
and <out>/MANIFEST.json.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import stat
import struct
import subprocess
import sys

PINNED = "a0c79e31467ab602d96605475dca69eb835c5d90"  # deepin-desktop-theme, see docs/desktop-ports/deepin/sources.json


# Hard limits: a DCI file is untrusted data. Anything outside them is refused (listed under `skipped`), never partly read.
MAX_DCI_BYTES = 8 << 20          # nirvana's largest file is well under 1 MiB
MAX_ENTRIES_PER_LEVEL = 256
MAX_DEPTH = 6                    # <size>/<state.tone>/<scale>/<layer> is 4 levels
MAX_WEBP_BYTES = 4 << 20
MAX_PIXELS = 1536 * 1536         # nirvana: 768 x 768
ICON_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$")
# <size>/<state>.<tone>/<scale>/<layer>: numbers are plain positive decimals and bounded; a layer file is *plain* only as `<prior>.webp`.
# The specification's other fields (`<prior>.<padding>p.<palette>.<h_s_l_r_g_b_a>.<format>[.alpha8]`) mean a tinted or padded layer: refused.
SIZE_DIR = re.compile(r"^[1-9][0-9]{0,3}$")
MAX_SIZE, MAX_SCALE = 1024, 4
PLAIN_LAYER = re.compile(r"^[1-9][0-9]{0,2}\.webp$")


class Malformed(ValueError):
    """The container breaks the format or a limit; carries the numbers that show it."""


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def read_entries(buf, depth=0):
    """Entries of one directory level: (type, name, content). Every length is checked before it is used."""
    if depth > MAX_DEPTH:
        raise Malformed(f"directories nested deeper than {MAX_DEPTH}")
    out, off = [], 0
    while off < len(buf):
        if len(out) >= MAX_ENTRIES_PER_LEVEL:
            raise Malformed(f"more than {MAX_ENTRIES_PER_LEVEL} entries in one directory")
        if off + 72 > len(buf):
            raise Malformed(f"truncated entry header: {len(buf) - off} of 72 bytes remain")
        kind = buf[off]
        raw = buf[off + 1 : off + 64]
        if b"\0" not in raw:
            raise Malformed("entry name is not NUL-terminated")
        try:
            name = raw.split(b"\0", 1)[0].decode("utf-8")
        except UnicodeDecodeError:
            raise Malformed("entry name is not UTF-8") from None
        if not name or "/" in name or any(ord(c) < 32 for c in name):
            raise Malformed(f"invalid entry name {name!r}")
        (size,) = struct.unpack("<Q", buf[off + 64 : off + 72])
        if size > len(buf) - off - 72:
            raise Malformed(f"entry {name!r} declares {size} bytes but only {len(buf) - off - 72} remain")
        out.append((kind, name, buf[off + 72 : off + 72 + size]))
        off += 72 + size
    return out


def flatten(buf, prefix="", depth=0):
    res = []
    for kind, name, content in read_entries(buf, depth):
        path = prefix + "/" + name
        if kind == 2:
            res += flatten(content, path, depth + 1)
        elif kind in (1, 3):
            res.append((kind, path, content))
        else:
            raise Malformed(f"entry type {kind} at {path}")
    return res


def load_dci(data):
    if len(data) > MAX_DCI_BYTES:
        raise Malformed(f"{len(data)} bytes exceeds the {MAX_DCI_BYTES} byte limit")
    if len(data) < 8 or data[:3] != b"DCI":
        raise Malformed("not a DCI file (magic)")
    if data[3] != 0:
        raise Malformed("magic is not NUL-terminated")
    if data[4] != 1:
        raise Malformed(f"format version {data[4]}, only 1 is understood")
    count = data[5] | data[6] << 8 | data[7] << 16
    entries = read_entries(data[8:])
    if len(entries) != count:
        raise Malformed(f"header says {count} root entries, found {len(entries)}")
    res = []
    for kind, name, content in entries:
        if kind == 2:
            res += flatten(content, "/" + name, 1)
        elif kind in (1, 3):
            res.append((kind, "/" + name, content))
        else:
            raise Malformed(f"entry type {kind} at /{name}")
    return res


def best_layer(entries):
    """The single plain WebP of normal.light at the largest size and scale, or (None, reason). Never raises on a hostile path."""
    files = {}
    for kind, path, content in entries:
        parts = path.strip("/").split("/")
        if kind != 1 or len(parts) != 4 or parts[1] != "normal.light":
            continue
        if not SIZE_DIR.match(parts[0]) or not SIZE_DIR.match(parts[2]):
            return None, f"malformed size or scale directory in {path!r}"
        size, scale = int(parts[0]), int(parts[2])
        if not 1 <= size <= MAX_SIZE or not 1 <= scale <= MAX_SCALE:
            return None, f"size {size} / scale {scale} outside 1..{MAX_SIZE} / 1..{MAX_SCALE}"
        files.setdefault((size, scale), []).append((parts[3], content))
    if not files:
        return None, "no normal.light image"
    size, scale = max(files)
    layers = files[(size, scale)]
    if len(layers) != 1:
        return None, f"{len(layers)} layers at {size}@{scale}"
    name, content = layers[0]
    if not PLAIN_LAYER.match(name):
        return None, f"layer {name!r} is not a plain `<prior>.webp` (padding, palette, colour-adjust or alpha8 fields need the palette engine)"
    if content[:4] != b"RIFF" or content[8:12] != b"WEBP":
        return None, f"layer {name} is not a WebP"
    if len(content) > MAX_WEBP_BYTES:
        return None, f"layer {name} is {len(content)} bytes, over the {MAX_WEBP_BYTES} byte limit"
    if struct.unpack("<I", content[4:8])[0] + 8 != len(content):
        return None, f"layer {name}: RIFF size {struct.unpack('<I', content[4:8])[0]} does not match {len(content)} bytes"
    return (size, scale, name, content), None


def to_png(webp, px, magick):
    return subprocess.run(
        # explicit coder prefix (no format sniffing), hard resource limits, an empty environment but PATH
        [magick, "-limit", "memory", "256MiB", "-limit", "map", "256MiB", "-limit", "disk", "0", "-limit", "time", "30",
         "-limit", "width", "2048", "-limit", "height", "2048", "-limit", "area", str(MAX_PIXELS),
         "webp:-", "-resize", f"{px}x{px}>", "-strip", "-define", "png:compression-level=9", "-define", "png:exclude-chunks=date,time", "png:-"],
        input=webp, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True, timeout=60, env={"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "MAGICK_CONFIGURE_PATH": "/nonexistent"},
    ).stdout


def git_env():
    return {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1", "GIT_OPTIONAL_LOCKS": "0", "LC_ALL": "C"}


def git(up, *args):
    return subprocess.run(["git", "-C", up, *args], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=120, env=git_env()).stdout


def blob_oid(data):
    """The Git object id of a blob (sha1 object format)."""
    return hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()


class Pinned:
    """What the pinned revision's own Git objects say the input is: tree entries of the icon directory and the two licence files."""

    LICENCE_FILES = ("LICENSES/CC-BY-4.0.txt", ".reuse/dep5")

    def __init__(self, up, rev, subdir="nirvana/dsg-icons/nirvana"):
        if git(up, "rev-parse", "--show-object-format").strip() != b"sha1":
            raise SystemExit("only sha1 object-format repositories are understood")
        self.rev, self.subdir = rev, subdir
        self.tree = git(up, "rev-parse", f"{rev}:{subdir}").decode().strip()
        self.entries = {}
        for rec in git(up, "ls-tree", "-r", "-z", rev, "--", subdir).split(b"\0"):
            if not rec:
                continue
            meta, path = rec.split(b"\t", 1)
            mode, kind, oid = meta.decode().split()
            name = path.decode("utf-8")[len(subdir) + 1 :]
            if kind != "blob" or "/" in name:
                raise SystemExit(f"unexpected tree entry {path!r} in {rev}:{subdir}")
            self.entries[name] = (mode, oid)
        self.licences = {f: git(up, "rev-parse", f"{rev}:{f}").decode().strip() for f in self.LICENCE_FILES}
        self.links = {n: git(up, "cat-file", "blob", oid).decode("utf-8", "replace") for n, (m, oid) in self.entries.items() if m == "120000"}


def verify_dci(pinned, name, path, root):
    """Read `name` from the work tree and check the bytes that will be converted against the pinned Git objects.

    Returns (bytes, git_blob_oid_or_None, problems). A link is followed only to a sibling and both the link text and the target's bytes
    must match the revision. The bytes returned are the ones hashed, so nothing can change between check and use."""
    problems = []
    ent = pinned.entries.get(name)
    if ent is None:
        return read_dci(path, root), None, [f"{name}: not in {pinned.rev[:12]} (untracked)"]
    mode, oid = ent
    if os.path.islink(path):
        text = os.readlink(path)
        if mode != "120000":
            problems.append(f"{name}: a link in the work tree, a file in the revision")
        elif blob_oid(text.encode("utf-8", "surrogateescape")) != oid:
            problems.append(f"{name}: link text {text!r} differs from the revision's {pinned.links.get(name)!r}")
        raw = read_dci(path, root)
        target = os.path.basename(os.path.realpath(path))
        tent = pinned.entries.get(target)
        if tent is None or tent[0] != "100644":
            problems.append(f"{name}: link target {target!r} is not a regular file of the revision")
            return raw, None, problems
        got = blob_oid(raw)
        if got != tent[1]:
            problems.append(f"{name}: target {target} is {got[:12]}, the revision has {tent[1][:12]}")
        return raw, (got if not problems else None), problems
    raw = read_dci(path, root)
    got = blob_oid(raw)
    if mode != "100644":
        problems.append(f"{name}: a file in the work tree, mode {mode} in the revision")
    if got != oid:
        problems.append(f"{name}: modified ({got[:12]}, the revision has {oid[:12]})")
    return raw, (got if got == oid and mode == "100644" else None), problems


def read_licence(up, pinned, rel):
    """A licence file's bytes, checked against the revision (returns (bytes, problems))."""
    path = os.path.join(up, *rel.split("/"))
    real = os.path.realpath(path)
    if os.path.islink(path) or not os.path.isfile(real):
        return b"", [f"{rel}: not a regular file in the work tree"]
    with open(real, "rb") as fh:
        data = fh.read(MAX_DCI_BYTES + 1)
    got = blob_oid(data)
    return data, ([] if got == pinned.licences[rel] else [f"{rel}: modified ({got[:12]}, the revision has {pinned.licences[rel][:12]})"])


def read_dci(path, root):
    """Read one regular file of `root`. A link is followed only when its target is a regular file directly inside `root` (upstream stores
    aliases such as `app.web.airdroid.dci` as links to a sibling); the size limit applies before the read, and the open refuses a link swap."""
    real = os.path.realpath(path)
    if os.path.dirname(real) != os.path.realpath(root):
        raise Malformed("resolves outside the source directory")
    fd = os.open(real, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_size > MAX_DCI_BYTES:
            raise Malformed(f"{st.st_size} bytes exceeds the {MAX_DCI_BYTES} byte limit or is not a regular file")
        with os.fdopen(fd, "rb", closefd=False) as fh:
            return fh.read(MAX_DCI_BYTES + 1)
    finally:
        os.close(fd)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--upstream", required=True, help="git checkout of deepin-desktop-theme at the pinned revision (read only)")
    ap.add_argument("--out", required=True, help="a NEW directory; created with mode 0700, never overwritten")
    ap.add_argument("--size", type=int, default=128, choices=(64, 96, 128, 192, 256))
    ap.add_argument("--limit", type=int, default=0, help="convert only the first N icons (tests)")
    ap.add_argument("--strict", action="store_true", help="exit 1 when any icon is skipped")
    ap.add_argument("--allow-other-revision", action="store_true")
    ap.add_argument("--allow-working-tree", action="store_true",
                    help="convert even when the input differs from the revision's Git objects; the manifest then says 'working-tree, NOT revision-verified'")
    a = ap.parse_args()
    up = os.path.expanduser(a.upstream)
    rev = git(up, "rev-parse", "HEAD").decode().strip()
    if rev != PINNED and not a.allow_other_revision:
        raise SystemExit(f"upstream is at {rev}, the pinned revision is {PINNED}")
    src = os.path.join(up, "nirvana", "dsg-icons", "nirvana")
    if not os.path.isdir(src) or os.path.islink(src):
        raise SystemExit(f"{src} is not a plain directory")
    pinned = Pinned(up, rev)
    problems = []
    on_disk = sorted(os.listdir(src))
    for extra in sorted(set(on_disk) - set(pinned.entries)):
        problems.append(f"{extra}: not in {rev[:12]} (untracked)")
    if not a.limit:
        for gone in sorted(set(pinned.entries) - set(on_disk)):
            problems.append(f"{gone}: in {rev[:12]} but missing from the work tree")
    lic_bytes, lic_problems = {}, []
    for rel in Pinned.LICENCE_FILES:
        lic_bytes[rel], pr = read_licence(up, pinned, rel)
        lic_problems += pr
    names = sorted(f for f in on_disk if f.endswith(".dci"))
    if a.limit:
        names = names[: a.limit]
    # Pre-pass: read and hash every input once; the bytes kept here are the bytes converted (no check-then-use gap).
    inputs = {}
    for f in names:
        icon = f[: -len(".dci")]
        if not ICON_NAME.match(icon):
            continue
        try:
            inputs[f] = verify_dci(pinned, f, os.path.join(src, f), src)
        except (Malformed, OSError) as e:
            inputs[f] = (None, None, [], f"refused: {e}")
            continue
        problems += inputs[f][2]
    problems += lic_problems
    if problems and not a.allow_working_tree:
        for line in problems[:10]:
            print(f"refused: {line}", file=sys.stderr)
        raise SystemExit(f"{len(problems)} input(s) differ from {rev[:12]}'s Git objects; nothing written (--allow-working-tree converts anyway and labels the output unverified)")
    out = os.path.abspath(a.out)
    root = os.path.join(out, "nirvana")
    if os.path.lexists(out):
        raise SystemExit(f"{out} exists; refusing to overwrite (use a fresh directory)")
    # Refuse unverified input before requiring or executing conversion tools.
    magick = shutil.which("magick") or sys.exit("ImageMagick (magick) with WebP support is needed")
    version = subprocess.check_output([magick, "-version"], text=True).splitlines()[0]
    os.makedirs(out, mode=0o700)
    os.makedirs(os.path.join(root, str(a.size)))
    icons, skipped = {}, {}
    for f in names:
        icon = f[: -len(".dci")]
        if not ICON_NAME.match(icon):
            skipped[icon] = "icon name has characters outside [A-Za-z0-9._+-] or is too long"
            continue
        got = inputs[f]
        if got[0] is None:
            skipped[icon] = got[3]
            continue
        raw, blob = got[0], got[1]
        try:
            layer, why = best_layer(load_dci(raw))
        except (Malformed, ValueError) as e:
            skipped[icon] = f"refused: {e}"
            continue
        if layer is None:
            skipped[icon] = why
            continue
        size, scale, lname, webp = layer
        try:
            png = to_png(webp, a.size, magick)
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as e:
            skipped[icon] = f"ImageMagick refused the image: {getattr(e, 'stderr', b'')[:120]!r}"
            continue
        with open(os.path.join(root, str(a.size), icon + ".png"), "xb") as fh:
            fh.write(png)
        icons[icon] = {"git_blob": blob, "source_sha256": sha256(raw), "layer": f"{size}/normal.light/{scale}/{lname}", "source_pixels": size * scale, "png_sha256": sha256(png), "bytes": len(png)}
    verified = not problems
    with open(os.path.join(root, "index.theme"), "w", newline="\n") as fh:
        fh.write(
            "[Icon Theme]\nName=nirvana\nComment=Deepin default icon theme (DCI converted to PNG)\nInherits=bloom\nExample=x-directory-normal\n"
            f"\nDirectories={a.size}\n\n[{a.size}]\nSize={a.size}\nContext=Applications\nType=Scalable\nMinSize=16\nMaxSize=512\n"
        )
    with open(os.path.join(root, "LICENSE-CC-BY-4.0.txt"), "wb") as fh:
        fh.write(lic_bytes["LICENSES/CC-BY-4.0.txt"])
    with open(os.path.join(root, "REUSE-dep5.txt"), "wb") as fh:
        fh.write(lic_bytes[".reuse/dep5"])
    with open(os.path.join(root, "NOTICE.txt"), "w", newline="\n") as fh:
        fh.write(
            "nirvana icon theme, (c) UnionTech Software Technology Co., Ltd., CC-BY-4.0 (deepin-desktop-theme, "
            f"https://github.com/linuxdeepin/deepin-desktop-theme at {rev}, nirvana/dsg-icons/nirvana; REUSE-dep5.txt is that repository's licence statement).\n"
            f"Modified: each icon's WebP layer was decoded and shrunk to at most {a.size} px as a PNG (never enlarged); nothing else was changed.\n"
            + ("" if verified else "WARNING: the input was a working tree that differs from the revision's Git objects; this output is NOT revision-verified.\n")
        )
    manifest = {
        "format": 2, "upstream": "https://github.com/linuxdeepin/deepin-desktop-theme", "revision": rev, "license": "CC-BY-4.0",
        "copyright": "UnionTech Software Technology Co., Ltd.", "size": a.size, "converter": "build_nirvana.py", "imagemagick": version,
        "input_verification": {
            "status": "git-objects" if verified else "working-tree, NOT revision-verified",
            "detail": ("every input file (and both licence files) hashed as a Git blob and equal to the object in the pinned revision's tree; no untracked file"
                       if verified else "the input differs from the pinned revision's Git objects; see problems"),
            "tree": pinned.tree, "subdir": pinned.subdir, "tree_entries": len(pinned.entries), "licence_blobs": pinned.licences,
            "problems": problems[:200], "problem_count": len(problems),
        },
        "note": "png_sha256 values hold for the ImageMagick build named above; git_blob / source_sha256 are the stable pins of the input",
        "icons": dict(sorted(icons.items())), "skipped": dict(sorted(skipped.items())),
    }
    with open(os.path.join(out, "MANIFEST.json"), "w", encoding="utf-8", newline="\n") as fh:
        json.dump(manifest, fh, indent=1)
        fh.write("\n")
    print(f"{len(icons)} icons, {len(skipped)} skipped, {sum(i['bytes'] for i in icons.values()) // 1024} KiB; input: {manifest['input_verification']['status']}")
    for icon, why in sorted(skipped.items()):
        print(f"  skipped {icon}: {why}", file=sys.stderr)
    if (skipped and a.strict) or (problems and not verified and a.strict):
        sys.exit(1)


if __name__ == "__main__":
    main()
