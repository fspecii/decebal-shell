"""Sprint limits, argv refusals, and the shared lane-report shell.

This lane does not start VMs, publish commits, restart a window manager, or
turn a passed JSON file into full acceptance.
"""

from __future__ import annotations

from datetime import datetime, timezone
import os
from pathlib import Path
import re

SPRINT_DEADLINE = "2026-10-31T11:36:32+00:00"
LANE = "debug"
VM_OWNER_LANE = "vm"
# Briefed by the sprint owner. control.json carries max_vms only.
VM_VCPU = 2
VM_MEMORY_MIB = 1024
MAX_VMS = 2
MAX_XVFB = 1
MAX_TIMEOUT_S = 600
# Private Xvfb fixtures stay in this window. Display :0 can keep its abstract
# socket after the filesystem X0 path is gone; the cause of that is unconfirmed.
MIN_PRIVATE_DISPLAY = 700
MAX_PRIVATE_DISPLAY = 999

# Root asked this lane not to execute these fixtures.
UNREVIEWED_MARKERS = (
    "settings-preview-stress.py",
    "settings-current-stress/run.py",
    "settings-current-stress/tabs.py",
    "panel-wm-acceptance",
    "panel-wm-popup",
)

HEAVY_BASENAMES = {"cargo", "rustc"}
VM_BASENAMES = {
    "qemu",
    "qemu-kvm",
    "qemu-system-x86_64",
    "qemu-system-aarch64",
    "virt-install",
    "virsh",
    "VBoxManage",
    "limactl",
    "machinectl",
}
PUBLISH_BASENAMES = {"claude", "grok", "codex", "t3", "gh"}
HOST_CONTROL_BASENAMES = {
    "systemctl",
    "loginctl",
    "reboot",
    "shutdown",
    "poweroff",
    "halt",
    "hyprctl",
}
GIT_MUTATIONS = {"push", "rebase", "reset", "filter-branch", "filter-repo", "clean", "amend"}
SECRET_KEY = re.compile(
    r"(?i)(secret|token|password|credential|api[_-]?key|authorization|cookie|private[_-]?key|access[_-]?key)"
)
SECRET_PREFIXES = ("SSH_", "AWS_", "GITHUB_", "GH_", "OPENAI_", "ANTHROPIC_", "XAI_", "T3_")
TRAILER_LINE = re.compile(r"(?im)^[ \t]*co-authored-by[ \t]*:(.*)$")
CLAUDE_CODE = re.compile(r"(?i)claude\s+code")


class LabError(Exception):
    code = 1


class PolicyError(LabError):
    code = 2


class ToolMissing(LabError):
    code = 3


def parse_time(value: str) -> datetime:
    text = value.strip().replace("Z", "+00:00")
    try:
        parsed = datetime.fromisoformat(text)
    except ValueError as exc:
        raise PolicyError(f"unusable timestamp {value}") from exc
    if parsed.tzinfo is None:
        raise PolicyError(f"timestamp needs a timezone: {value}")
    return parsed.astimezone(timezone.utc)


def ensure_deadline(spec: dict, now: datetime | None = None) -> datetime:
    raw = spec.get("deadline") or SPRINT_DEADLINE
    deadline = parse_time(raw)
    moment = now or datetime.now(timezone.utc)
    if moment.tzinfo is None:
        raise PolicyError("clock value needs a timezone")
    moment = moment.astimezone(timezone.utc)
    if moment > deadline and not spec.get("allow_after_deadline"):
        raise PolicyError(f"past sprint deadline {deadline.isoformat()}")
    return deadline


# Root retired the leaked inode. These paths stay untouched: no open, flock, write, or signal.
LIVE_SLOT_PATHS = (
    "/home/ambsd/.cache/decebal-build-slots/slot1.lock",
    "/home/ambsd/.cache/decebal-build-slots/slot2.lock",
    "/home/ambsd/.cache/decebal-build-slots/slot1-retired-0200.lock",
)
_LIVE_SLOT_NAMES = frozenset(Path(path).name for path in LIVE_SLOT_PATHS)


def basename(argv0: str) -> str:
    return Path(argv0).name


def live_slot_path(value: object) -> bool:
    """True for the leaked, retired, or in-use coordinator slots. Does not touch them."""

    if not isinstance(value, str) or not value:
        return False
    path = Path(value).expanduser()
    text = str(path)
    if text in LIVE_SLOT_PATHS:
        return True
    return path.name in _LIVE_SLOT_NAMES and path.parent.name == "decebal-build-slots"


def check_argv(argv: list[str], *, allow_heavy: bool = False, allow_unreviewed: bool = False) -> None:
    if not argv or not all(isinstance(part, str) and part for part in argv):
        raise PolicyError("command must be a non-empty list of strings")
    names = [basename(part) for part in argv]
    name = names[0]
    if any(item in VM_BASENAMES for item in names):
        raise PolicyError("this lane does not start VMs; the vm lane owns that ceiling")
    if any(item in PUBLISH_BASENAMES or item in HOST_CONTROL_BASENAMES for item in names):
        refused = next(item for item in names if item in PUBLISH_BASENAMES or item in HOST_CONTROL_BASENAMES)
        raise PolicyError(f"refusing to run {refused}")
    if name == "git" and any(part in GIT_MUTATIONS for part in argv[1:]):
        raise PolicyError("refusing a history rewrite or publish git command")
    if any(item in HEAVY_BASENAMES for item in names) and not allow_heavy:
        raise PolicyError("refusing a heavy compiler; set allow_heavy only when root asks")
    if name == "flock" and "--close" not in argv[1:]:
        raise PolicyError("plain flock passes the lock fd to the child; use flock --close")
    if any(live_slot_path(part) for part in argv):
        raise PolicyError("refusing a live build slot")
    joined = " ".join(argv)
    if not allow_unreviewed and any(marker in joined for marker in UNREVIEWED_MARKERS):
        raise PolicyError("refusing an unreviewed fixture; read its contract instead of executing it")


def secret_key(name: str) -> bool:
    return name.startswith(SECRET_PREFIXES) or bool(SECRET_KEY.search(name))


def forbidden_trailer(message: str) -> str | None:
    """Return the offending trailer value, or None.

    Sprint policy rejects every Co-authored-by trailer on a new candidate.
    Claude Code is named explicitly when that is the identity. Existing
    history is not rewritten; callers choose the commit range.
    """

    for value in TRAILER_LINE.findall(message):
        text = value.strip()
        if CLAUDE_CODE.search(text):
            return f"Claude Code trailer: {text}"
        return f"Co-authored-by trailer: {text}"
    return None


def coordination() -> dict:
    return {
        "schema": "decebal.sprint.lane-report.v1",
        "lane": LANE,
        "deadline_utc": SPRINT_DEADLINE,
        "full_acceptance": False,
        "vm": {
            "owner_lane": VM_OWNER_LANE,
            "max_vms": MAX_VMS,
            "vcpu": VM_VCPU,
            "memory_mib": VM_MEMORY_MIB,
            "vcpu_source": "sprint brief; control.json has max_vms only",
            "memory_source": "sprint brief; control.json has max_vms only",
            "started_by_this_lane": False,
        },
        "xvfb": {"max_owned": MAX_XVFB},
        "publishes": False,
        "calls_model_apis": False,
    }


def parse_display_number(value: str | None) -> int | None:
    if not value or not value.startswith(":"):
        return None
    number = value[1:].split(".", 1)[0]
    if number.isdigit():
        return int(number)
    return None


def host_display_number() -> int | None:
    return parse_display_number(os.environ.get("DISPLAY", ""))


def check_timeout(value: object) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise PolicyError("timeout_s must be a number")
    if not 0 < value <= MAX_TIMEOUT_S:
        raise PolicyError(f"timeout_s must be inside (0, {MAX_TIMEOUT_S}]")
    return float(value)


def check_name(value: object) -> str:
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9_.-]{1,80}", value):
        raise PolicyError("name must be a short token")
    return value


def check_output(path: Path) -> Path:
    resolved = path.expanduser().resolve()
    if resolved == Path("/") or resolved == Path.home().resolve():
        raise PolicyError("refusing that output directory")
    return resolved
