"""Source-frozen build and test gate.

A passing gate hashes the named source before and after the build and tests.
It does not mean full desktop, hardware, Wayland, or release acceptance.
"""

from __future__ import annotations

from datetime import datetime, timezone
from pathlib import Path

from .fingerprint import changed, fingerprint
from .job import features_in_argv, run_job, validate_job
from .policy import PolicyError, check_argv, check_name, check_output, check_timeout, coordination, ensure_deadline
from .report import write_report

_KEYS = {
    "name",
    "deadline",
    "allow_after_deadline",
    "source_root",
    "source_paths",
    "exclude",
    "build",
    "bins",
    "tests",
    "allow_heavy",
    "allow_unreviewed",
    "trailer_since",
    "repo",
}


def run_gate(spec: dict, *, out: Path, now: datetime | None = None) -> dict:
    if not isinstance(spec, dict):
        raise PolicyError("gate spec must be an object")
    unknown = sorted(set(spec) - _KEYS)
    if unknown:
        raise PolicyError(f"unknown gate field {unknown[0]}")
    name = check_name(spec.get("name", "gate"))
    deadline = ensure_deadline(spec, now)
    if not spec.get("source_paths"):
        raise PolicyError("a source-frozen gate needs source_paths")
    root = Path(spec.get("source_root", ".")).resolve()
    allow_heavy = bool(spec.get("allow_heavy", False))
    build = spec.get("build")
    feature_error = _feature_error(build, allow_heavy=allow_heavy)
    for test in spec.get("tests") or []:
        if not isinstance(test, dict):
            raise PolicyError("each gate test must be an object")
        if test.get("build"):
            raise PolicyError("nesting a build inside a gate test is refused")
        validate_job({**test, "allow_heavy": allow_heavy, "allow_unreviewed": spec.get("allow_unreviewed", False)})
    out = check_output(out)
    out.mkdir(mode=0o700, parents=True, exist_ok=True)
    report = {
        "schema": "decebal.desktop-lab.gate.v1",
        "name": name,
        "started_at": datetime.now(timezone.utc).replace(microsecond=0).isoformat(),
        "deadline_utc": deadline.isoformat(),
        "full_acceptance": False,
        "gate_passed": False,
        "hardware_acceptance": False,
        "coordination": coordination(),
        "errors": [],
        "limits": [
            "gate_passed means the named source was unchanged and the listed commands exited 0",
            "it is not frozen release proof unless the build command and binary hashes are the release ones",
            "it is not physical CPU, GPU, Wayland, or input acceptance",
        ],
        "build": None,
        "tests": [],
    }
    before = fingerprint(root, spec["source_paths"], spec.get("exclude"))
    report["source"] = {"before": before}
    if feature_error:
        report["errors"].append(feature_error)
        report["build"] = {
            "argv": build.get("argv"),
            "features": list(build.get("features", [])),
            "features_match": False,
            "exit_code": None,
            "binaries": [],
        }
    elif build is not None:
        built = run_job(_build_job(name, spec, build, root, deadline, allow_heavy), out=out / "build", now=now)
        present, found = features_in_argv(build["argv"])
        report["build"] = {
            "argv": build["argv"],
            "features": list(build.get("features", [])),
            "features_in_argv": found if present else None,
            "exit_code": _command_exit(built),
            "binaries": built["binaries"],
            "job_passed": built["job_passed"],
            "full_acceptance": False,
            "report": str((out / "build" / "report.json")),
        }
        if not built["job_passed"]:
            report["errors"].append("build failed")
    elif spec.get("bins"):
        probed = run_job(_probe_job(spec, root, deadline), out=out / "bins", now=now)
        report["binaries"] = probed["binaries"]
        if not probed["job_passed"]:
            report["errors"].append("binary manifest is incomplete")
    if not report["errors"]:
        for test in spec.get("tests") or []:
            child_spec = dict(test)
            child_spec.setdefault("backend", "none")
            child_spec.setdefault("session_bus", False)
            child_spec["allow_heavy"] = allow_heavy
            child_spec["allow_unreviewed"] = spec.get("allow_unreviewed", False)
            child_spec["deadline"] = spec.get("deadline", deadline.isoformat())
            child_spec["allow_after_deadline"] = spec.get("allow_after_deadline", False)
            child = run_job(child_spec, out=out / "tests" / child_spec["name"], now=now)
            report["tests"].append(
                {
                    "name": child_spec["name"],
                    "job_passed": child["job_passed"],
                    "full_acceptance": False,
                    "exit_code": _command_exit(child),
                    "timed_out": child["timed_out"],
                    "errors": child["errors"],
                    "report": str(out / "tests" / child_spec["name"] / "report.json"),
                }
            )
            if not child["job_passed"]:
                report["errors"].append(f"test {child_spec['name']} failed")
    after = fingerprint(root, spec["source_paths"], spec.get("exclude"))
    report["source"]["after"] = after
    report["source"]["changed"] = changed(before, after)
    report["source"]["frozen"] = report["source"]["changed"] == [] and before["sha256"] == after["sha256"]
    if not report["source"]["frozen"]:
        report["errors"].append("source changed during the gate")
    if spec.get("trailer_since"):
        from .records import check_commits

        trailer_errors = check_commits(Path(spec.get("repo") or root), spec["trailer_since"])
        report["trailers"] = {"since": spec["trailer_since"], "errors": trailer_errors, "history_rewritten": False}
        report["errors"].extend(trailer_errors)
    report["gate_passed"] = not report["errors"]
    report["finished_at"] = datetime.now(timezone.utc).replace(microsecond=0).isoformat()
    write_report(out, report)
    return report


def _feature_error(build: dict | None, *, allow_heavy: bool) -> str | None:
    if build is None:
        return None
    if not isinstance(build, dict):
        raise PolicyError("build must be an object")
    check_argv(build.get("argv"), allow_heavy=allow_heavy, allow_unreviewed=False)
    check_timeout(build.get("timeout_s", 30))
    if not isinstance(build.get("features", []), list):
        raise PolicyError("features must be a list")
    present, found = features_in_argv(build["argv"])
    declared = list(build.get("features", []))
    if (declared and not present) or (present and declared != found):
        return "declared features do not match the build command"
    return None


def _build_job(name: str, spec: dict, build: dict, root: Path, deadline, allow_heavy: bool) -> dict:
    del name
    return {
        "name": "build",
        "backend": "none",
        "session_bus": False,
        "stub_defaults": True,
        "timeout_s": build.get("timeout_s", 30),
        "command": build["argv"],
        "bins": spec.get("bins") or [],
        "source_root": str(root),
        "cwd": str(root),
        "allow_heavy": allow_heavy,
        "deadline": spec.get("deadline", deadline.isoformat()),
        "allow_after_deadline": spec.get("allow_after_deadline", False),
    }


def _probe_job(spec: dict, root: Path, deadline) -> dict:
    return {
        "name": "bins",
        "backend": "none",
        "session_bus": False,
        "stub_defaults": False,
        "timeout_s": 5,
        "command": ["/usr/bin/true"],
        "bins": spec["bins"],
        "source_root": str(root),
        "deadline": spec.get("deadline", deadline.isoformat()),
        "allow_after_deadline": True,
    }


def _command_exit(report: dict):
    for process in report.get("processes") or []:
        if process["name"] == "command":
            return process["exit_code"]
    return None
