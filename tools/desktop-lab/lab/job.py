"""Run one bounded command inside a private desktop session."""

from __future__ import annotations

from datetime import datetime, timezone
import json
from pathlib import Path
import secrets
import shutil
import tempfile
import time

from .containment import (
    contained_argv,
    note_namespace_init,
    probe_containment,
    verify_drain,
    write_supervisor,
)
from .display import XvfbSlot, capture_png, start_session_bus, start_wayland, start_xvfb
from .fingerprint import changed, fingerprint, sha256_file
from .isolation import IsolatedEnv, host_snapshot
from .lifetime import (
    CoordinatorStop,
    SlotGuard,
    begin_cleanup,
    lock_release_allowed,
    pop_coordinator_signals,
    push_coordinator_signals,
    sync_guard,
    tracks_have_survivors,
)
from .policy import (
    LabError,
    PolicyError,
    basename,
    check_argv,
    check_name,
    check_output,
    check_timeout,
    coordination,
    ensure_deadline,
    live_slot_path,
)
from .procman import Owned, become_subreaper, clear_subreaper, kill_marked, public_process
from .report import redact_text, write_report
from .slot import acquire_build_slot, slot_metadata

_DEFAULT_STUBS = {
    "xclip": 127,
    "wl-copy": 127,
    "wl-paste": 127,
    "xsel": 127,
    "xdg-open": 127,
    "systemctl": 127,
    "loginctl": 127,
    "reboot": 127,
    "shutdown": 127,
    "poweroff": 127,
    "halt": 127,
    "hyprctl": 127,
    "swaymsg": 127,
    "gsettings": 127,
    "omarchy-theme-set": 127,
}
_SIGKILL = {-9, 137}
_JOB_KEYS = {
    "name",
    "backend",
    "timeout_s",
    "screenshot",
    "command",
    "stubs",
    "stub_defaults",
    "feature",
    "commit",
    "bins",
    "build",
    "source_root",
    "source_paths",
    "exclude",
    "screen",
    "path_extra",
    "tools",
    "session_bus",
    "deadline",
    "allow_after_deadline",
    "allow_heavy",
    "allow_unreviewed",
    "sample_interval_s",
    "cwd",
    "slot",
}


def validate_job(spec: dict) -> dict:
    if not isinstance(spec, dict):
        raise PolicyError("job spec must be an object")
    if "host_context" in spec or "inherit_host" in spec:
        raise PolicyError("refusing host context; private XDG_RUNTIME_DIR alone is not isolation")
    unknown = sorted(set(spec) - _JOB_KEYS)
    if unknown:
        raise PolicyError(f"unknown job field {unknown[0]}")
    spec = dict(spec)
    spec["name"] = check_name(spec.get("name", "job"))
    backend = spec.get("backend", "none")
    if backend not in {"xvfb", "wayland", "none"}:
        raise PolicyError("backend must be xvfb, wayland, or none")
    spec["backend"] = backend
    spec["timeout_s"] = check_timeout(spec.get("timeout_s", 30))
    spec["screenshot"] = bool(spec.get("screenshot", False))
    check_argv(
        spec.get("command"),
        allow_heavy=bool(spec.get("allow_heavy", False)),
        allow_unreviewed=bool(spec.get("allow_unreviewed", False)),
    )
    if spec["screenshot"] and backend != "xvfb":
        raise PolicyError("a screenshot requires an owned Xvfb; Wayland pixels are not invented")
    if spec.get("build") is not None:
        _validate_build(spec["build"], allow_heavy=bool(spec.get("allow_heavy", False)))
    if "tools" in spec and not isinstance(spec["tools"], dict):
        raise PolicyError("tools must be an object")
    for extra in spec.get("path_extra") or []:
        path = Path(extra)
        if not path.is_absolute() or not path.is_dir():
            raise PolicyError("path_extra entries must be absolute directories")
        if str(path).startswith("/run/user/"):
            raise PolicyError("path_extra must not point at the live user runtime")
    if "slot" in spec:
        slot_path = spec["slot"]
        if not isinstance(slot_path, str) or not Path(slot_path).is_absolute():
            raise PolicyError("build slot must be an absolute path")
        if live_slot_path(slot_path):
            raise PolicyError("refusing a live build slot")
        if basename(spec["command"][0]) == "flock":
            raise PolicyError("the slot field already applies flock --close")
    return spec


def _validate_build(build: dict, *, allow_heavy: bool) -> None:
    if not isinstance(build, dict):
        raise PolicyError("build must be an object")
    for key in build:
        if key not in {"argv", "features", "timeout_s"}:
            raise PolicyError(f"unknown build field {key}")
    check_argv(build.get("argv"), allow_heavy=allow_heavy, allow_unreviewed=False)
    if not isinstance(build.get("features", []), list):
        raise PolicyError("features must be a list")
    check_timeout(build.get("timeout_s", 30))


def run_job(spec: dict, *, out: Path, now: datetime | None = None) -> dict:
    spec = validate_job(spec)
    deadline = ensure_deadline(spec, now)
    out = check_output(out)
    out.mkdir(mode=0o700, parents=True, exist_ok=True)
    started = datetime.now(timezone.utc).replace(microsecond=0)
    report = {
        "schema": "decebal.desktop-lab.report.v1",
        "name": spec["name"],
        "feature": spec.get("feature"),
        "commit": spec.get("commit"),
        "started_at": started.isoformat(),
        "deadline_utc": deadline.isoformat(),
        "backend": spec["backend"],
        "command": spec["command"],
        "job_passed": False,
        "full_acceptance": False,
        "timed_out": False,
        "hardware_acceptance": False,
        "gpu": None,
        "coordination": coordination(),
        "limits": [
            "job_passed means the owned command and requested checks finished; it is not full acceptance",
            "process RSS/CPU/FD/thread samples are not physical CPU, GPU, or input proof",
            "this lane starts no VM and owns at most one Xvfb",
            "a private XDG_RUNTIME_DIR alone is not isolation",
            "plain flock is refused because the child would inherit the lock fd",
            "the build slot stays locked until the contained process tree is verified dead",
            "an unverified drain does not release the build slot",
            "each contained track is registered pending before spawn on an invocation-unique drain path, and the slot stays locked until that track has its own invocation-bound verified drain or its own complete known-start init death; a drain left in the output directory by an earlier run is not proof",
            "coordinator SIGTERM and SIGHUP stop the owned tree; SIGKILL cascades through parent-death and the slot guardian",
            "a host /proc marker scan is not containment and is not complete cleanup",
            "SIGKILL and exit 137 stay an unknown cause unless coordinator provenance is supplied",
            "private Xvfb uses an explicit display >= 700 and proves both listening socket inodes belong to the spawned server before input",
            "bare -displayfd is unused; foreign X sockets and locks are not unlinked",
        ],
        "compiler_failure": False,
        "result_class": "pending",
        "errors": [],
        "processes": [],
        "screenshots": [],
        "recorder": [],
        "recorder_is_not_protocol_success": True,
        "binaries": [],
        "build": None,
        "source": None,
    }
    owned = Owned()
    slot = None
    logs = []
    token = ""
    subreaper_set = False
    guard: SlotGuard | None = None
    host = host_snapshot()
    root = Path(tempfile.mkdtemp(prefix="desktop-lab-"))
    signal_state = push_coordinator_signals()

    try:
        iso = IsolatedEnv(root)
        token = secrets.token_hex(16)
        iso.env["DECEBAL_DESKTOP_LAB_JOB"] = token
        subreaper_set = True
        reaper_ok = become_subreaper()
        if not reaper_ok:
            report["errors"].append("job subreaper could not be enabled")
        _prepare_path(iso, spec, root)
        containment_ok = _attach_containment(report)
        if isinstance(report.get("containment"), dict):
            report["containment"]["invocation"] = token
        supervisor = root / "containment-supervisor.py"
        if containment_ok:
            write_supervisor(supervisor)
        if spec.get("slot") and containment_ok:
            slot_path = Path(spec["slot"])
            build_lock_fd = acquire_build_slot(slot_path)
            report["build_slot"] = slot_metadata(build_lock_fd, slot_path, spec["command"])
            report["build_slot"]["released"] = False
            if not report["build_slot"]["close_on_exec"]:
                report["errors"].append("build slot lock fd is not CLOEXEC")
                build_failed_lock = True
            else:
                build_failed_lock = False
            guard = SlotGuard()
            guard.start(build_lock_fd, out)
            owned.guard = guard
            report["build_slot"]["guardian_pid"] = guard.pid
            report["build_slot"]["guardian_start"] = guard.starttime
        else:
            build_failed_lock = False
        if spec["backend"] == "wayland":
            iso.env["XDG_SESSION_TYPE"] = "wayland"
        else:
            iso.env["XDG_SESSION_TYPE"] = "x11"
        if spec.get("source_paths"):
            report["source"] = {
                "before": fingerprint(Path(spec["source_root"]), spec["source_paths"], spec.get("exclude"))
            }
        build_failed = build_failed_lock or not reaper_ok or not containment_ok
        if spec.get("build") and reaper_ok and containment_ok:
            report["build"] = _run_build(spec, iso, owned, out, logs, supervisor, token)
            built = report["build"]
            if built["timed_out"]:
                report["errors"].append("build command exceeded timeout")
                build_failed = True
            elif built["exit_code"] in _SIGKILL:
                _mark_sigkill(report)
                build_failed = True
            elif built["exit_code"] != 0:
                report["errors"].append("build command failed")
                build_failed = True
            if not built["features_match"]:
                report["errors"].append("declared features do not match the build command")
                build_failed = True
        if containment_ok and spec.get("session_bus", True):
            log = _log(out, logs, "session-bus")
            iso.session_bus = start_session_bus(
                owned, iso.paths["XDG_RUNTIME_DIR"], binary=_tool(spec, "dbus_daemon", "/usr/bin/dbus-daemon"), log=log, env=iso.env
            )
            iso.session_bus_started = True
            iso.env["DBUS_SESSION_BUS_ADDRESS"] = iso.session_bus
        elif containment_ok:
            missing = root / "no-session-bus"
            iso.session_bus = f"unix:path={missing}"
            iso.env["DBUS_SESSION_BUS_ADDRESS"] = iso.session_bus
        if containment_ok and spec["backend"] == "xvfb":
            slot = XvfbSlot(Path(spec["tools"]["xvfb_lock"]) if spec.get("tools", {}).get("xvfb_lock") else None)
            slot.acquire()
            log = _log(out, logs, "xvfb")
            _track, display = start_xvfb(
                owned,
                binary=_tool(spec, "xvfb", "/usr/bin/Xvfb"),
                screen=spec.get("screen", "1280x720x24"),
                log=log,
                env=iso.env,
            )
            report["display_transport"] = _track["display_transport"]
            iso.env["DISPLAY"] = display
            iso.env.pop("WAYLAND_DISPLAY", None)
            iso.env["DECEBAL_BACKEND"] = "x11"
        elif containment_ok and spec["backend"] == "wayland":
            log = _log(out, logs, "wayland")
            name = start_wayland(
                owned, iso.paths["XDG_RUNTIME_DIR"], binary=_tool(spec, "sway", "/usr/bin/sway"), log=log, env=iso.env
            )
            iso.env["WAYLAND_DISPLAY"] = name
            iso.env.pop("DISPLAY", None)
            iso.env["DECEBAL_BACKEND"] = "wayland"
        elif containment_ok:
            iso.env.pop("DISPLAY", None)
            iso.env.pop("WAYLAND_DISPLAY", None)
        if containment_ok:
            iso.assert_private(host, backend=spec["backend"])
            host.clear()
            report["isolation"] = iso.public()
        if not build_failed:
            command_log = _log(out, logs, "command")
            command = _spawn_contained(
                owned,
                spec["command"],
                env=iso.env,
                name="command",
                cwd=Path(spec["cwd"]) if spec.get("cwd") else None,
                stdout=command_log,
                stderr=command_log,
                supervisor=supervisor,
                out=out,
                invocation=token,
            )
            _watch(owned, command, spec)
            if command["exit_code"] is None:
                report["timed_out"] = True
                owned.stop_one(command, 1.5)
                report["errors"].append("command exceeded timeout")
            else:
                _note_exit(report, command["exit_code"])
            if spec["screenshot"]:
                image = out / "screenshot.png"
                report["screenshots"].append(
                    capture_png(_tool(spec, "import", "/usr/bin/import"), image, iso.env)
                )
        report["binaries"] = _hash_bins(spec)
        if spec.get("source_paths"):
            after = fingerprint(Path(spec["source_root"]), spec["source_paths"], spec.get("exclude"))
            report["source"]["after"] = after
            report["source"]["changed"] = changed(report["source"]["before"], after)
            report["source"]["frozen"] = report["source"]["changed"] == [] and report["source"]["before"]["sha256"] == after["sha256"]
            if not report["source"]["frozen"]:
                report["errors"].append("source changed during the job")
        else:
            report["limits"].append("no source paths were named, so this job is not source-frozen")
        report["recorder"] = _read_recorder(root / "recorder.jsonl")
        missing_bins = [item["path"] for item in report["binaries"] if not item["exists"]]
        if missing_bins:
            report["errors"].append("a requested binary is missing")
        report["job_passed"] = not report["errors"] and not report["timed_out"]
    except CoordinatorStop as exc:
        report["errors"].append(f"coordinator received signal {exc.signum} and stopped the owned tree")
        report["coordinator_signal"] = exc.signum
        report["job_passed"] = False
        report.setdefault("isolation", None)
    except LabError as exc:
        report["errors"].append(redact_text(str(exc)))
        report["error_code"] = exc.code
        report["job_passed"] = False
        report.setdefault("isolation", None)
    except OSError as exc:
        report["errors"].append(redact_text(str(exc)))
        report["job_passed"] = False
        report.setdefault("isolation", None)
    finally:
        begin_cleanup()
        try:
            owned.stop_all()
            _finish_containment(report, owned)
            report["escaped_children_signaled"] = kill_marked(token) if token else []
            _release_slot_if_drained(report, owned, guard)
            if subreaper_set:
                clear_subreaper()
            if slot is not None:
                slot.release()
            for handle in logs:
                handle.close()
            _redact_logs(out)
            report["processes"] = [public_process(track) for track in owned.tracks]
            report["subreaper"] = subreaper_set and "job subreaper could not be enabled" not in report["errors"]
            if report.get("result_class") == "pending":
                report["result_class"] = "ok" if report.get("job_passed") else "failed"
            shutil.rmtree(root, ignore_errors=True)
            report["finished_at"] = datetime.now(timezone.utc).replace(microsecond=0).isoformat()
            write_report(out, report)
        finally:
            pop_coordinator_signals(signal_state)
    return report


def _note_exit(report: dict, code: int) -> None:
    if code in _SIGKILL:
        _mark_sigkill(report)
        return
    if code != 0:
        report["errors"].append(f"command exited {code}")


def _mark_sigkill(report: dict) -> None:
    """SIGKILL and exit 137 have no coordinator provenance in this process."""

    report["kill_cause"] = "unknown"
    report["result_class"] = "killed"
    report["compiler_failure"] = False
    report["errors"].append(
        "command ended with SIGKILL or exit 137; cause is unknown without coordinator provenance"
    )


def _prepare_path(iso: IsolatedEnv, spec: dict, root: Path) -> None:
    bin_dir = root / "bin"
    bin_dir.mkdir(mode=0o700)
    mapping = dict(_DEFAULT_STUBS)
    for stub in spec.get("stubs") or []:
        if not isinstance(stub, dict) or "/" in stub.get("name", "") or not stub.get("name"):
            raise PolicyError("stub name must be a basename")
        if isinstance(stub.get("exit"), bool) or not isinstance(stub.get("exit"), int):
            raise PolicyError("stub exit must be an integer")
        if stub["name"] in _DEFAULT_STUBS and stub["exit"] == 0:
            raise PolicyError("safety stubs must not report success")
        mapping[stub["name"]] = stub["exit"]
    _write_recorder(bin_dir / "desktop-lab-recorder.py")
    (bin_dir / "stub-exits.json").write_text(json.dumps(mapping))
    for name in mapping:
        (bin_dir / name).symlink_to(bin_dir / "desktop-lab-recorder.py")
    iso.env["DECEBAL_DESKTOP_LAB_RECORDER"] = str(root / "recorder.jsonl")
    iso.env["DECEBAL_DESKTOP_LAB_STUB_MAP"] = str(bin_dir / "stub-exits.json")
    parts = [str(bin_dir)]
    parts.extend(spec.get("path_extra") or [])
    parts.extend(["/usr/bin", "/bin"])
    iso.set_path(parts)


def _write_recorder(path: Path) -> None:
    path.write_text(
        "#!/usr/bin/env python3\n"
        "import json, os, sys\n"
        "from pathlib import Path\n"
        "name = Path(sys.argv[0]).name\n"
        "mapping = json.loads(Path(os.environ['DECEBAL_DESKTOP_LAB_STUB_MAP']).read_text())\n"
        "row = {'command': name, 'argv': sys.argv[1:]}\n"
        "with open(os.environ['DECEBAL_DESKTOP_LAB_RECORDER'], 'a', encoding='utf-8') as handle:\n"
        "    handle.write(json.dumps(row) + '\\n')\n"
        "raise SystemExit(int(mapping.get(name, 127)))\n"
    )
    path.chmod(0o755)


def _run_build(spec: dict, iso: IsolatedEnv, owned: Owned, out: Path, logs: list, supervisor: Path, invocation: str) -> dict:
    build = spec["build"]
    present, found = features_in_argv(build["argv"])
    declared = list(build.get("features", []))
    mismatch = (present and declared != found) or (declared and not present)
    log = _log(out, logs, "build")
    track = _spawn_contained(
        owned,
        build["argv"],
        env=iso.env,
        name="build",
        cwd=Path(spec["source_root"]) if spec.get("source_root") else None,
        stdout=log,
        stderr=log,
        supervisor=supervisor,
        out=out,
        invocation=invocation,
    )
    _watch(owned, track, {"timeout_s": check_timeout(build.get("timeout_s", spec["timeout_s"])), "sample_interval_s": 0.5})
    timed_out = track["exit_code"] is None
    if timed_out:
        owned.stop_one(track, 1.5)
    return {
        "argv": build["argv"],
        "features": declared,
        "features_in_argv": found if present else None,
        "features_match": not mismatch,
        "exit_code": track["exit_code"],
        "timed_out": timed_out,
    }


def features_in_argv(argv: list[str]) -> tuple[bool, list[str]]:
    found: list[str] = []
    present = False
    index = 0
    while index < len(argv):
        arg = argv[index]
        if arg == "--features" and index + 1 < len(argv):
            present = True
            found.extend(_split_features(argv[index + 1]))
            index += 2
            continue
        if arg.startswith("--features="):
            present = True
            found.extend(_split_features(arg.split("=", 1)[1]))
        index += 1
    return present, found


def _split_features(text: str) -> list[str]:
    return [part for part in text.replace(",", " ").split() if part]


def _hash_bins(spec: dict) -> list[dict]:
    root = Path(spec["source_root"]).resolve() if spec.get("source_root") else None
    rows = []
    for item in spec.get("bins") or []:
        path = Path(item)
        if not path.is_absolute():
            if root is None:
                raise PolicyError("relative binaries need source_root")
            path = root / path
        if not path.is_file():
            rows.append({"path": str(path), "exists": False, "sha256": None, "bytes": None})
            continue
        rows.append({"path": str(path), "exists": True, "sha256": sha256_file(path), "bytes": path.stat().st_size})
    return rows


def _watch(owned: Owned, track: dict, spec: dict) -> None:
    deadline = time.monotonic() + spec["timeout_s"]
    interval = min(max(float(spec.get("sample_interval_s") or 0.5), 0.1), 2.0)
    _note_init(owned, track)
    _sample_alive(owned)
    while track["process"].poll() is None:
        _note_init(owned, track)
        if time.monotonic() >= deadline:
            return
        _wait_slice(track, min(interval, max(0.0, deadline - time.monotonic())))
        if len(track["samples"]) < 20:
            _sample_alive(owned)
    track["process"].wait(timeout=1)
    track["exit_code"] = track["process"].returncode


def _note_init(owned: Owned, track: dict) -> None:
    if track.get("drain_file"):
        note_namespace_init(track)
        sync_guard(owned.guard, owned.tracks)


def _invocation_drain(out: Path, invocation: str, name: str) -> Path:
    if not invocation or "/" in invocation or invocation in {".", ".."}:
        raise PolicyError("containment invocation must be a single path segment")
    directory = out / "invocations" / invocation
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    return directory / f"containment-{name}.json"


def _spawn_contained(
    owned: Owned,
    user_argv: list[str],
    *,
    env: dict[str, str],
    name: str,
    cwd: Path | None,
    stdout,
    stderr,
    supervisor: Path,
    out: Path,
    invocation: str,
) -> dict:
    drain = _invocation_drain(out, invocation, name)
    if owned.guard is not None:
        owned.guard.reserve_track(name, str(drain), invocation)
    argv = contained_argv(user_argv, drain_file=drain, supervisor=supervisor, invocation=invocation)
    track = owned.spawn(argv, env=env, name=name, cwd=cwd, stdout=stdout, stderr=stderr)
    track["drain_file"] = str(drain)
    track["invocation"] = invocation
    note_namespace_init(track)
    sync_guard(owned.guard, owned.tracks)
    return track


def _attach_containment(report: dict) -> bool:
    try:
        info = probe_containment()
    except PolicyError as exc:
        report["errors"].append(str(exc))
        report["error_code"] = exc.code
        report["containment"] = {
            "kind": "unsupported",
            "claims_marker_scan_is_sufficient": False,
            "verified_drain": False,
        }
        return False
    report["containment"] = {
        "kind": info["kind"],
        "argv_prefix": info["argv_prefix"],
        "kill_child": info["kill_child"],
        "claims_marker_scan_is_sufficient": False,
        "verified_drain": False,
    }
    return True


def _release_slot_if_drained(report: dict, owned: Owned, guard: SlotGuard | None) -> None:
    if guard is None or guard.fd is None:
        return
    info = report.get("containment")
    verified = isinstance(info, dict) and bool(info.get("verified_drain"))
    allowed = lock_release_allowed(
        armed=guard.armed,
        verified_drain=verified,
        survivors=tracks_have_survivors(owned.tracks),
    )
    released = guard.finish(allowed)
    slot = report.get("build_slot")
    if isinstance(slot, dict):
        slot["released"] = released
        slot["guardian_pid"] = guard.pid
        slot["guardian_start"] = guard.starttime
    if not released:
        report["errors"].append("build slot remains locked; the owned tree was not verified dead")
        report["job_passed"] = False


def _finish_containment(report: dict, owned: Owned) -> None:
    info = report.get("containment")
    if not isinstance(info, dict) or info.get("kind") != "pid-namespace":
        return
    saw = False
    verified = True
    for track in owned.tracks:
        drain = track.get("drain_file")
        if not drain:
            continue
        saw = True
        inits = track.get("namespace_init") or []
        if isinstance(inits, int):
            inits = [inits]
        if not verify_drain(Path(drain), inits, track.get("namespace_init_start") or {}, track.get("invocation")):
            verified = False
    info["claims_marker_scan_is_sufficient"] = False
    info["verified_drain"] = saw and verified
    if saw and not verified:
        report["errors"].append("containment drain was not verified")
        report["job_passed"] = False


def _sample_alive(owned: Owned) -> None:
    for item in owned.tracks:
        if item["process"].poll() is None and len(item["samples"]) < 20:
            owned.sample(item)


def _wait_slice(track: dict, seconds: float) -> None:
    fd = track.get("pidfd")
    if fd is not None and seconds > 0:
        import select

        select.select([fd], [], [], seconds)
    elif seconds > 0:
        time.sleep(seconds)


def _tool(spec: dict, key: str, default: str) -> str:
    tools = spec.get("tools") or {}
    if not isinstance(tools, dict):
        raise PolicyError("tools must be an object")
    return str(tools.get(key, default))


def _redact_logs(out: Path) -> None:
    directory = out / "logs"
    if not directory.is_dir():
        return
    for path in directory.glob("*.log"):
        path.write_text(redact_text(path.read_text(encoding="utf-8", errors="replace")))


def _log(out: Path, logs: list, name: str):
    directory = out / "logs"
    directory.mkdir(mode=0o700, exist_ok=True)
    handle = (directory / f"{name}.log").open("w")
    logs.append(handle)
    return handle


def _read_recorder(path: Path) -> list[dict]:
    if not path.exists():
        return []
    rows = []
    for line in path.read_text().splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            rows.append({"error": "unreadable recorder line"})
            continue
        rows.append({"command": row.get("command"), "argv": row.get("argv")})
    return rows
