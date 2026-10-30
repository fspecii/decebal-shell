#!/usr/bin/env python3
"""Install the optional Hyprland session with its configured runtime prefix.

PREFIX is a literal absolute Unix prefix, restricted to portable path characters
because it is also embedded in PATH, shell commands and desktop/Hyprland config.
DESTDIR is staging only and is never embedded in runtime files.
"""
import os
from pathlib import Path
import re
import sys


def install(prefix: str, destdir: str) -> None:
    # Validate before creating anything. In particular, do not feed make values
    # through shell or sed expressions. A colon cannot be represented in PATH.
    if not re.fullmatch(r"/[A-Za-z0-9_./+\-]*", prefix):
        raise ValueError("PREFIX must be absolute and contain only letters, digits, /, _, ., + or - (no whitespace or shell metacharacters)")
    if any(component in (".", "..") for component in prefix.split("/")):
        raise ValueError("PREFIX must be absolute without . or .. path components")
    if destdir and not Path(destdir).is_absolute():
        raise ValueError("DESTDIR must be absolute when set")
    prefix = prefix.rstrip("/")
    source = Path(__file__).resolve().parent
    base = Path(destdir or "/") / prefix.lstrip("/")
    files = [
        (source / "decebal-session", "bin/decebal-session", 0o755),
        (source.parent / "compat/mac-compat", "bin/macshell-session", 0o755),
        (source / "omarchy-launch-shell", "lib/decebal/shim/omarchy-launch-shell", 0o755),
        (source / "omarchy-shell", "lib/decebal/shim/omarchy-shell", 0o755),
        (source / "hyprland-decebal.desktop", "share/wayland-sessions/hyprland-decebal.desktop", 0o644),
    ]
    files += [(source / name, "share/decebal/" + name, 0o644)
              for name in ("hypridle.conf", "hyprlock.conf", "mako.conf")]
    files += [(source.parent / name, "share/decebal/" + name, 0o644)
              for name in ("hyprland-layers.lua", "hyprland-dechaven.lua", "hyprland-declaunch.lua")]
    # Read the complete input set before writing any destination.
    prepared = [(relative, mode, path.read_text().replace("/usr/local", prefix))
                for path, relative, mode in files]
    for relative, mode, text in prepared:
        target = base / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
        target.chmod(mode)


if __name__ == "__main__":
    try:
        install(os.environ.get("PREFIX", "/usr/local"), os.environ.get("DESTDIR", ""))
    except (OSError, ValueError) as error:
        sys.exit(f"install-session: {error}")
