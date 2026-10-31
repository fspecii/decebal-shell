#!/usr/bin/env python3
"""EVIDENCE ONLY: a bounded Xcursor *reader* for the preview sheet and the evidence checks.

This is NOT an installer, builder or fetcher and must not be used as one. Installing and building icon/cursor packs is done by the
shared, reviewed decebal installer (`decthemectl assets install`, src/desktop_assets) from `asset-packs.toml`. This module only parses
already-installed Xcursor files, with hard limits, so the evidence scripts never trust a file's own sizes.

  xcursor_read.py info XCURSOR_FILE
"""
import pathlib, struct, sys

MAGIC = b"Xcur"; IMAGE = 0xFFFD0002
MAX_FILE = 16 * 1024 * 1024; MAX_CHUNKS = 512; MAX_SIDE = 256


def read(path):
    p = pathlib.Path(path)
    if p.stat().st_size > MAX_FILE: raise ValueError("file too large")
    b = p.read_bytes()
    if len(b) < 16 or b[:4] != MAGIC: raise ValueError("not Xcursor")
    hdr, ver, n = struct.unpack_from("<III", b, 4)
    if hdr != 16 or n > MAX_CHUNKS or 16 + 12 * n > len(b): raise ValueError("bad header")
    out = []
    for i in range(n):
        typ, sub, pos = struct.unpack_from("<III", b, 16 + 12 * i)
        if typ != IMAGE: continue
        if pos + 36 > len(b): raise ValueError("chunk outside file")
        _, _, nom, _, w, h, hx, hy, delay = struct.unpack_from("<IIIIIIIII", b, pos)
        if not (0 < w <= MAX_SIDE and 0 < h <= MAX_SIDE) or nom != sub or not (0 <= hx < w and 0 <= hy < h) or pos + 36 + 4 * w * h > len(b):
            raise ValueError(f"bad image chunk {i}")
        out.append({"nominal": nom, "w": w, "h": h, "hot": (hx, hy), "delay": delay, "argb": b[pos + 36: pos + 36 + 4 * w * h]})
    return out


if __name__ == "__main__" and len(sys.argv) == 3 and sys.argv[1] == "info":
    for r in read(sys.argv[2]): print({k: v for k, v in r.items() if k != "argb"})
