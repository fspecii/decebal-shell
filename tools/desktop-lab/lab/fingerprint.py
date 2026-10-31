"""Exact file fingerprints for a source-frozen gate.

The caller names the paths. Nothing here walks the repository by default,
and a matching pair is a freeze check, not full desktop acceptance.
"""

from __future__ import annotations

import hashlib
from pathlib import Path

from .policy import PolicyError

_SKIP_DIRS = {".git", "__pycache__", "target"}


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _excluded(relative: str, exclude: list[str]) -> bool:
    for item in exclude:
        if relative == item or relative.startswith(item.rstrip("/") + "/"):
            return True
    return False


def fingerprint(root: Path, paths: list[str], exclude: list[str] | None = None) -> dict:
    if not paths:
        raise PolicyError("a source fingerprint needs at least one path")
    root = root.resolve()
    excluded = exclude or []
    files: list[Path] = []
    for relative in paths:
        if not isinstance(relative, str) or relative.startswith("/") or ".." in Path(relative).parts:
            raise PolicyError(f"source path must stay inside the root: {relative}")
        target = (root / relative).resolve()
        if root != target and root not in target.parents:
            raise PolicyError(f"source path escapes the root: {relative}")
        if target.is_file():
            files.append(target)
        elif target.is_dir():
            for dirpath, dirnames, filenames in os_walk(target):
                dirnames[:] = sorted(name for name in dirnames if name not in _SKIP_DIRS)
                for name in sorted(filenames):
                    files.append(Path(dirpath) / name)
        else:
            raise PolicyError(f"source path is missing: {relative}")
    entries = []
    for file in sorted(set(files)):
        relative = file.relative_to(root).as_posix()
        if _excluded(relative, excluded):
            continue
        mode = file.stat().st_mode & 0o777
        entries.append({"path": relative, "sha256": sha256_file(file), "size": file.stat().st_size, "mode": mode})
    payload = "\n".join(f"{item['path']}\0{item['sha256']}\0{item['size']}\0{item['mode']}" for item in entries)
    return {
        "algorithm": "sha256",
        "sha256": hashlib.sha256(payload.encode()).hexdigest(),
        "count": len(entries),
        "files": entries,
    }


def changed(before: dict, after: dict) -> list[str]:
    left = {item["path"]: (item["sha256"], item["size"], item["mode"]) for item in before["files"]}
    right = {item["path"]: (item["sha256"], item["size"], item["mode"]) for item in after["files"]}
    return [path for path in sorted(set(left) | set(right)) if left.get(path) != right.get(path)]


def os_walk(path: Path):
    import os

    return os.walk(path)
