"""Commit trailers, evidence index, and read-only sprint contracts.

Checking a trailer never rewrites history. Reading a passed JSON file never
promotes it to full acceptance. Unreviewed stress and panel fixtures are not
executed.
"""

from __future__ import annotations

import json
from pathlib import Path
import shutil
import subprocess
import tempfile

from .fingerprint import sha256_file
from .policy import (
    MAX_VMS,
    SPRINT_DEADLINE,
    LabError,
    PolicyError,
    forbidden_trailer,
    parse_time,
)

_GIT_READS = {"log", "rev-parse", "diff-tree", "show", "rev-list"}
_PREVIEW_MARKERS = (
    "SETTINGS_TAB_SECONDS",
    "SETTINGS_BIN",
    "SETTINGS_EVIDENCE_DIR",
    "no-session-bus",
    "no-system-bus",
    "DECEBAL_SYSTEM_AUTOMATIC",
)
_PREVIEW_TABS = ("theme", "palettes", "customise", "apps")


def git_read(repo: Path, args: list[str], timeout: float = 30) -> str:
    if not args or args[0] not in _GIT_READS:
        raise PolicyError("refusing a git command that is not a read")
    home = Path(tempfile.mkdtemp(prefix="desktop-lab-git-"))
    try:
        proc = subprocess.run(
            ["git", "-C", str(repo), *args],
            env={
                "PATH": "/usr/bin:/bin",
                "HOME": str(home),
                "GIT_CONFIG_NOSYSTEM": "1",
                "GIT_CONFIG_GLOBAL": "/dev/null",
                "GIT_CONFIG_SYSTEM": "/dev/null",
            },
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            check=False,
        )
    finally:
        shutil.rmtree(home, ignore_errors=True)
    if proc.returncode != 0:
        detail = proc.stderr.strip().splitlines()
        raise PolicyError(detail[-1] if detail else "git read failed")
    return proc.stdout


def check_message(message: str) -> str | None:
    return forbidden_trailer(message)


def check_commits(repo: Path, since: str) -> list[str]:
    """Return trailer errors for commits after `since`. The range is not rewritten."""

    if not isinstance(since, str) or not since or since.startswith("-"):
        raise PolicyError("since must be a revision, not an option")
    raw = git_read(repo, ["log", f"{since}..HEAD", "--format=%H%x1f%B%x1e"])
    errors = []
    for record in raw.split("\x1e"):
        if "\x1f" not in record:
            continue
        sha, message = record.split("\x1f", 1)
        sha = sha.strip()
        problem = forbidden_trailer(message)
        if problem:
            errors.append(f"{sha} {problem}")
    return errors


def index_commits(repo: Path, since: str, evidence: Path | None = None) -> dict:
    raw = git_read(repo, ["log", "--reverse", f"{since}..HEAD", "--format=%H%x1f%s%x1f%b%x1e"])
    reports = _evidence(evidence) if evidence else []
    commits = []
    for record in raw.split("\x1e"):
        if record.count("\x1f") < 2:
            continue
        sha, subject, body = record.split("\x1f", 2)
        sha = sha.strip()
        files = [
            line
            for line in git_read(repo, ["diff-tree", "--root", "--no-commit-id", "--name-only", "-r", sha]).splitlines()
            if line
        ]
        features = feature_names(files)
        problem = forbidden_trailer(f"{subject}\n{body}")
        matched = []
        for item in reports:
            if item.get("commit") == sha or (item.get("feature") and item.get("feature") in features):
                matched.append(
                    {
                        "path": item["path"],
                        "feature": item.get("feature"),
                        "job_passed": item.get("job_passed"),
                        "full_acceptance": False,
                    }
                )
        commits.append(
            {
                "sha": sha,
                "subject": subject.strip(),
                "features": features,
                "files": files,
                "forbidden_trailer": problem,
                "evidence": matched,
                "full_acceptance": False,
            }
        )
    return {
        "schema": "decebal.desktop-lab.evidence-index.v1",
        "since": since,
        "full_acceptance": False,
        "history_rewritten": False,
        "commits": commits,
    }


def feature_names(paths: list[str]) -> list[str]:
    found = []
    for path in paths:
        parts = path.split("/")
        if not parts or parts[0] == "":
            continue
        if path.startswith("tools/desktop-lab/"):
            found.append("desktop-lab")
        elif parts[0] == "src" and len(parts) > 1:
            if parts[1] == "bin" and len(parts) > 2:
                found.append("bin:" + parts[2].removesuffix(".rs"))
            else:
                found.append(parts[1].removesuffix(".rs"))
        elif parts[0] == "tests":
            found.append("tests:" + "/".join(parts[1:]))
        elif parts[0] == "docs":
            found.append("docs:" + parts[-1])
        else:
            found.append(parts[0])
    return sorted(set(found))


def classify_record(document: dict, *, kind: str) -> dict:
    if not isinstance(document, dict):
        raise PolicyError("evidence must be a JSON object")
    fingerprint = document.get("source_fingerprint") if isinstance(document.get("source_fingerprint"), dict) else None
    frozen = False
    if document.get("source_frozen") is True and fingerprint is not None:
        before = fingerprint.get("before")
        after = fingerprint.get("after")
        frozen = isinstance(before, str) and before == after and len(before) == 64
    binding = document.get("source_binding")
    if isinstance(binding, str) and ("not independently frozen" in binding or "no independent" in binding):
        frozen = False
    return {
        "kind": kind,
        "json_passed": document.get("passed") is True,
        "json_passed_is_not_full_acceptance": True,
        "full_acceptance": False,
        "source_frozen": frozen,
        "acceptance": "source_frozen_job" if frozen else "candidate_runtime",
        "hardware_acceptance": False,
        "recorded_hardware_claim_ignored": document.get("hardware_acceptance") is True,
        "executed_by_desktop_lab": False,
        "errors": [],
    }


def classify_panel(document: dict) -> dict:
    base = classify_record(document, kind="panel")
    base["source_binding"] = document.get("source_binding")
    base["recorded_limits"] = list(document.get("limits") or [])
    base["binaries_recorded"] = document.get("binaries")
    base["binaries_rehashed"] = False
    base["source_frozen"] = False
    base["acceptance"] = "candidate_runtime"
    try:
        work = int(document["workarea_cases"])
        short = int(document["short_all_applet_scale2_cases"])
        popups = document["popup_directions"]
        if not isinstance(popups, dict):
            raise TypeError("popup_directions")
        popup_true = sum(1 for value in popups.values() if value is True)
        base.update(
            workarea_cases=work,
            short_cases=short,
            workarea_total=work + short,
            popup_count=len(popups),
            popup_true=popup_true,
            counts_match_contract=work + short == 25 and len(popups) == 8 and popup_true == 8,
        )
    except (KeyError, TypeError, ValueError) as exc:
        base["errors"].append(f"panel summary is missing counts: {exc}")
        base["counts_match_contract"] = False
    base["limits"] = list(base["recorded_limits"]) + ["desktop-lab did not re-run this fixture"]
    return base


def classify_settings(document: dict, bin_dir: Path | None = None) -> dict:
    base = classify_record(document, kind="settings-stress")
    base["source_frozen"] = False
    base["acceptance"] = "candidate_runtime"
    base["historical_crash_cause"] = document.get("historical_crash_cause")
    base["unexpected_exit_observed"] = document.get("unexpected_exit_observed")
    base["recorded_limits"] = list(document.get("limits") or [])
    claimed = {}
    pinned = document.get("binaries_unchanged_and_pinned") or {}
    if isinstance(pinned, dict):
        for name, info in pinned.items():
            if isinstance(info, dict):
                claimed[name] = info.get("expected")
    base["claimed_binary_sha256"] = claimed
    base["binaries_rehashed"] = False
    if bin_dir is not None:
        rows = {}
        for name, digest in claimed.items():
            path = bin_dir / name
            if not path.is_file():
                rows[name] = {"exists": False, "matches_claimed": False}
                continue
            actual = sha256_file(path)
            rows[name] = {"exists": True, "sha256": actual, "matches_claimed": actual == digest}
        base["rehash"] = rows
        base["binaries_rehashed"] = True
        base["rehash_note"] = "read-only hashes of the named copies; not a new build and not full acceptance"
    base["limits"] = list(base["recorded_limits"]) + [
        "passed JSON is a bounded observation, not full acceptance",
        "desktop-lab did not re-run this stress",
    ]
    return base


def preview_contract(script: Path, inputs: Path | None = None) -> dict:
    text = script.read_text(encoding="utf-8")
    missing = [item for item in _PREVIEW_MARKERS if item not in text]
    digest = sha256_file(script)
    reviewed = None
    matches = None
    if inputs is not None:
        payload = json.loads(inputs.read_text(encoding="utf-8"))
        reviewed = (payload.get("reviewed_fixture_hashes") or {}).get("tests/e2e/settings-preview-stress.py")
        matches = reviewed == digest if isinstance(reviewed, str) else False
    return {
        "executed": False,
        "full_acceptance": False,
        "hardware_acceptance": False,
        "script": str(script),
        "sha256": digest,
        "tabs": [tab for tab in _PREVIEW_TABS if tab in text],
        "missing_markers": missing,
        "contract_ok": not missing and all(tab in text for tab in _PREVIEW_TABS),
        "reviewed_hash": reviewed,
        "reviewed_hash_matches": matches,
        "root_reported_switches": {
            "count": 40,
            "verified_by_this_lane": False,
            "note": "Root reported a SETTINGS_TAB_SECONDS run passed 40 switches. This command does not execute the script.",
        },
        "limits": [
            "contract read only; the stress script was not executed",
            "a passing result would still not be full acceptance or a source-frozen release",
        ],
    }


def read_control(path: Path) -> dict:
    document = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(document, dict):
        raise PolicyError("control file must be a JSON object")
    deadline = document.get("deadline_utc")
    matches = False
    if isinstance(deadline, str):
        try:
            matches = parse_time(deadline) == parse_time(SPRINT_DEADLINE)
        except LabError:
            matches = False
    policy = document.get("policy") if isinstance(document.get("policy"), dict) else {}
    return {
        "deadline_utc": deadline if isinstance(deadline, str) else None,
        "deadline_matches_sprint": matches,
        "max_vms": document.get("max_vms"),
        "max_vms_matches_brief": document.get("max_vms") == MAX_VMS,
        "no_coauthor_trailers": policy.get("no_coauthor_trailers") is True,
        "threads_ignored": "threads" in document,
        "full_acceptance": False,
        "vcpu": 2,
        "memory_mib": 1024,
        "vcpu_source": "sprint brief; not taken from thread records",
        "memory_source": "sprint brief; not taken from thread records",
    }


def _evidence(root: Path) -> list[dict]:
    rows = []
    if not root.exists():
        return rows
    for path in root.rglob("report.json"):
        if len(path.relative_to(root).parts) > 4:
            continue
        try:
            document = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            continue
        if isinstance(document, dict):
            rows.append(
                {
                    "path": str(path),
                    "commit": document.get("commit"),
                    "feature": document.get("feature"),
                    "job_passed": document.get("job_passed"),
                }
            )
    return rows
