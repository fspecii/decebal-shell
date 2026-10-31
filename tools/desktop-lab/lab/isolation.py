"""Private HOME, XDG, session bus, and a nonexistent system bus.

Patterns copied from the existing fixtures, not from the live session:
tests/e2e/env.sh, tests/e2e_spot.sh, tests/wm_tracker_lifetime.rs,
tests/file-reveal.py, and tests/e2e/desktop-frame-settings.py.
The child environment is an allowlist. Host secrets are not copied.
"""

from __future__ import annotations

import os
from pathlib import Path
import stat

from .policy import MIN_PRIVATE_DISPLAY, PolicyError, parse_display_number, secret_key

_COPY = ("LANG", "LC_ALL", "LC_CTYPE", "TZ", "USER", "LOGNAME", "TERM")
_SCRUB_EXACT = {
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "SESSION_MANAGER",
    "HOME",
    "XDG_RUNTIME_DIR",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "XDG_CACHE_HOME",
    "XDG_CONFIG_DIRS",
    "XDG_DATA_DIRS",
    "DBUS_SESSION_BUS_ADDRESS",
    "DBUS_SYSTEM_BUS_ADDRESS",
}
_SCRUB_PREFIXES = ("RDWM_", "GTK_", "GDK_", "GSK_", "QT_")
_LIVE_DISPLAYS = {":0", ":0.0", ":1", ":1.0"}
_DIRS = (
    ("HOME", "home"),
    ("XDG_CONFIG_HOME", "config"),
    ("XDG_DATA_HOME", "data"),
    ("XDG_STATE_HOME", "state"),
    ("XDG_CACHE_HOME", "cache"),
    ("XDG_RUNTIME_DIR", "run"),
    ("XDG_CONFIG_DIRS", "empty-config"),
    ("XDG_DATA_DIRS", "empty-data"),
)


class IsolatedEnv:
    def __init__(self, root: Path):
        self.root = root
        self.root.mkdir(mode=0o700, exist_ok=True)
        self.root.chmod(0o700)
        self.paths: dict[str, Path] = {}
        for key, name in _DIRS:
            path = root / name
            path.mkdir(mode=0o700)
            path.chmod(0o700)
            self.paths[key] = path
        tmp = root / "tmp"
        tmp.mkdir(mode=0o700)
        tmp.chmod(0o700)
        self.system_bus_path = root / "no-system-bus"
        if self.system_bus_path.exists():
            raise PolicyError("system bus placeholder must not exist")
        self.system_bus = f"unix:path={self.system_bus_path}"
        self.session_bus: str | None = None
        self.session_bus_started = False
        self.env: dict[str, str] = {}
        self.host_home_reused = False
        self.host_display_reused = False
        self.host_bus_reused = False
        for key in _COPY:
            if key in os.environ:
                self.env[key] = os.environ[key]
        self.env.update(
            {
                "HOME": str(self.paths["HOME"]),
                "XDG_CONFIG_HOME": str(self.paths["XDG_CONFIG_HOME"]),
                "XDG_DATA_HOME": str(self.paths["XDG_DATA_HOME"]),
                "XDG_STATE_HOME": str(self.paths["XDG_STATE_HOME"]),
                "XDG_CACHE_HOME": str(self.paths["XDG_CACHE_HOME"]),
                "XDG_RUNTIME_DIR": str(self.paths["XDG_RUNTIME_DIR"]),
                "XDG_CONFIG_DIRS": str(self.paths["XDG_CONFIG_DIRS"]),
                "XDG_DATA_DIRS": str(self.paths["XDG_DATA_DIRS"]),
                "DECEBAL_CONFIG": str(self.paths["XDG_CONFIG_HOME"]),
                "DECEBAL_SYSTEM_AUTOMATIC": "0",
                "NO_AT_BRIDGE": "1",
                "TMPDIR": str(tmp),
                "DBUS_SYSTEM_BUS_ADDRESS": self.system_bus,
                "PATH": "/usr/bin:/bin",
            }
        )

    def set_path(self, parts: list[str]) -> None:
        for part in parts:
            if not Path(part).is_absolute():
                raise PolicyError("PATH entries must be absolute")
        self.env["PATH"] = ":".join(parts)

    def public(self) -> dict:
        runtime = self.paths["XDG_RUNTIME_DIR"]
        mode = stat.S_IMODE(runtime.stat().st_mode)
        return {
            "home": str(self.paths["HOME"]),
            "xdg_runtime_dir": str(runtime),
            "runtime_mode": mode,
            "session_bus": self.session_bus,
            "session_bus_started": self.session_bus_started,
            "system_bus": self.system_bus,
            "system_bus_exists": self.system_bus_path.exists(),
            "display": self.env.get("DISPLAY"),
            "wayland_display": self.env.get("WAYLAND_DISPLAY"),
            "path": self.env["PATH"],
            "env_names": sorted(self.env),
            "live_display_copied": False,
            "live_wayland_copied": False,
            "host_home_reused": self.host_home_reused,
            "host_display_reused": self.host_display_reused,
            "host_bus_reused": self.host_bus_reused,
            "runtime_dir_alone_is_isolation": False,
        }

    def assert_private(self, host: dict, *, backend: str) -> None:
        """Raise when a private runtime dir is the only thing that changed.

        Host values are compared and then dropped. They are not written to the report.
        """

        env = self.env
        values = host.get("values") or {}
        home = env.get("HOME", "")
        if not home or not _inside(self.root, home):
            raise PolicyError("refusing host HOME; private XDG_RUNTIME_DIR alone is not isolation")
        if values.get("HOME") and home == values["HOME"]:
            raise PolicyError("refusing host HOME; private XDG_RUNTIME_DIR alone is not isolation")
        runtime = env.get("XDG_RUNTIME_DIR", "")
        if values.get("XDG_RUNTIME_DIR") and runtime == values["XDG_RUNTIME_DIR"]:
            raise PolicyError("refusing host XDG_RUNTIME_DIR; private XDG_RUNTIME_DIR alone is not isolation")
        for key, path in self.paths.items():
            if env.get(key) != str(path) or not _inside(self.root, str(path)):
                raise PolicyError(f"{key} is not a private directory")
        mode = stat.S_IMODE(self.paths["XDG_RUNTIME_DIR"].stat().st_mode)
        if mode != 0o700:
            raise PolicyError("XDG_RUNTIME_DIR must be mode 0700")
        for key in ("XAUTHORITY", "SESSION_MANAGER"):
            if key in env:
                raise PolicyError(f"refusing {key}; private XDG_RUNTIME_DIR alone is not isolation")
        for key in env:
            if key.startswith(_SCRUB_PREFIXES):
                raise PolicyError(f"refusing {key}; private XDG_RUNTIME_DIR alone is not isolation")
            if secret_key(key):
                raise PolicyError("refusing a secret in the child environment")
        display = env.get("DISPLAY")
        wayland = env.get("WAYLAND_DISPLAY")
        if display in _LIVE_DISPLAYS:
            raise PolicyError("refusing live DISPLAY; private XDG_RUNTIME_DIR alone is not isolation")
        if values.get("DISPLAY") and display == values["DISPLAY"]:
            raise PolicyError("refusing host DISPLAY; private XDG_RUNTIME_DIR alone is not isolation")
        if values.get("WAYLAND_DISPLAY") and wayland == values["WAYLAND_DISPLAY"]:
            raise PolicyError("refusing host WAYLAND_DISPLAY; private XDG_RUNTIME_DIR alone is not isolation")
        if backend == "none" and (display or wayland):
            raise PolicyError("unit job mode must scrub DISPLAY and WAYLAND_DISPLAY")
        if backend == "xvfb":
            number = parse_display_number(display)
            if wayland or number is None or number < MIN_PRIVATE_DISPLAY:
                raise PolicyError("an owned Xvfb must publish only an explicit display number >= 700")
        if backend == "wayland" and (display or not wayland):
            raise PolicyError("a private compositor job must not keep a host display")
        self._assert_bus(env.get("DBUS_SESSION_BUS_ADDRESS", ""), values.get("DBUS_SESSION_BUS_ADDRESS"), session=True)
        self._assert_bus(env.get("DBUS_SYSTEM_BUS_ADDRESS", ""), values.get("DBUS_SYSTEM_BUS_ADDRESS"), session=False)
        if self.system_bus_path.exists():
            raise PolicyError("system bus placeholder must not exist")

    def _assert_bus(self, address: str, host_value: str | None, *, session: bool) -> None:
        label = "session bus" if session else "system bus"
        if host_value and address == host_value:
            raise PolicyError(f"refusing host {label}; private XDG_RUNTIME_DIR alone is not isolation")
        if not address.startswith("unix:path="):
            raise PolicyError(f"{label} must be a unix path")
        path = address.removeprefix("unix:path=")
        if not _inside(self.root, path):
            raise PolicyError(f"{label} is outside the private root")
        if not session and Path(path).exists():
            raise PolicyError("system bus placeholder must not exist")


def host_snapshot() -> dict:
    """Host session values used for a one-shot comparison. Not report material."""

    values = {}
    for key, value in os.environ.items():
        if key in _SCRUB_EXACT or key.startswith(_SCRUB_PREFIXES):
            values[key] = value
    return {"values": values}


def _inside(root: Path, path: str) -> bool:
    try:
        target = Path(path).resolve()
        base = root.resolve()
    except OSError:
        return False
    return target == base or base in target.parents
