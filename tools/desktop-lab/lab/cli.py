"""Command line for the private desktop-lab harness."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys

from .fingerprint import fingerprint
from .gate import run_gate
from .job import run_job
from .lifetime import push_coordinator_signals
from .policy import LabError, PolicyError, SPRINT_DEADLINE, coordination
from .records import (
    check_commits,
    check_message,
    classify_panel,
    classify_settings,
    index_commits,
    preview_contract,
    read_control,
)
from .report import write_report
from .slot import incident_record

_BASELINE = Path(__file__).resolve().parents[1] / "BASELINE"


def main(argv: list[str] | None = None) -> int:
    push_coordinator_signals()
    parser = argparse.ArgumentParser(prog="desktop-lab", description="Private desktop test harness. Exit 0 is not full acceptance.")
    sub = parser.add_subparsers(dest="command", required=True)

    run = sub.add_parser("run", help="run one isolated command")
    run.add_argument("--spec", type=Path)
    run.add_argument("--out", type=Path, required=True)
    run.add_argument("--name", default="job")
    run.add_argument("--backend", choices=("xvfb", "wayland", "none"), default="none")
    run.add_argument("--timeout", type=float, default=30)
    run.add_argument("--screenshot", action="store_true")
    run.add_argument("--session-bus", action="store_true", default=False)
    run.add_argument("argv", nargs=argparse.REMAINDER)
    run.set_defaults(func=cmd_run)

    gate = sub.add_parser("gate", help="source-frozen build and test gate")
    gate.add_argument("--spec", type=Path, required=True)
    gate.add_argument("--out", type=Path, required=True)
    gate.set_defaults(func=cmd_gate)

    commits = sub.add_parser("commits", help="check new candidate commit messages")
    commits_sub = commits.add_subparsers(dest="commits_command", required=True)
    check = commits_sub.add_parser("check", help="read commits after a revision; do not rewrite them")
    check.add_argument("--repo", type=Path, default=Path("."))
    check.add_argument("--since")
    check.set_defaults(func=cmd_commits_check)
    message = commits_sub.add_parser("check-message", help="check one message without creating a commit")
    message.add_argument("--file", type=Path, required=True)
    message.set_defaults(func=cmd_check_message)

    index = sub.add_parser("index", help="index commit features and lab evidence")
    index.add_argument("--repo", type=Path, default=Path("."))
    index.add_argument("--since", required=True)
    index.add_argument("--evidence", type=Path)
    index.add_argument("--out", type=Path, required=True)
    index.set_defaults(func=cmd_index)

    catalog = sub.add_parser("catalog", help="read sprint contracts without executing them")
    catalog.add_argument("--out", type=Path, required=True)
    catalog.add_argument("--panel", type=Path)
    catalog.add_argument("--settings", type=Path)
    catalog.add_argument("--settings-bins", type=Path)
    catalog.add_argument("--settings-inputs", type=Path)
    catalog.add_argument("--preview", type=Path)
    catalog.add_argument("--control", type=Path)
    catalog.set_defaults(func=cmd_catalog)

    sprint = sub.add_parser("sprint", help="read deadline and ceilings from the control file")
    sprint.add_argument("--control", type=Path, required=True)
    sprint.add_argument("--out", type=Path, required=True)
    sprint.set_defaults(func=cmd_sprint)

    finger = sub.add_parser("fingerprint", help="hash named source paths")
    finger.add_argument("--root", type=Path, required=True)
    finger.add_argument("--path", action="append", required=True)
    finger.add_argument("--out", type=Path, required=True)
    finger.set_defaults(func=cmd_fingerprint)

    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except LabError as exc:
        print(exc, file=sys.stderr)
        return exc.code


def cmd_run(args) -> int:
    if args.spec and args.argv:
        raise PolicyError("pass a spec or a command, not both")
    if args.spec:
        spec = _load(args.spec)
    else:
        command = list(args.argv)
        if command[:1] == ["--"]:
            command = command[1:]
        spec = {
            "name": args.name,
            "backend": args.backend,
            "timeout_s": args.timeout,
            "screenshot": args.screenshot,
            "session_bus": args.session_bus,
            "command": command,
        }
    report = run_job(spec, out=args.out)
    _announce(args.out, report.get("job_passed"))
    return int(report.get("error_code") or (0 if report.get("job_passed") else 1))


def cmd_gate(args) -> int:
    report = run_gate(_load(args.spec), out=args.out)
    _announce(args.out, report.get("gate_passed"))
    return 0 if report.get("gate_passed") else 1


def cmd_commits_check(args) -> int:
    since = args.since or _baseline()
    errors = check_commits(args.repo.resolve(), since)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        print("history was not rewritten", file=sys.stderr)
        return 1
    print(f"no forbidden trailers after {since}")
    return 0


def cmd_check_message(args) -> int:
    problem = check_message(args.file.read_text(encoding="utf-8"))
    if problem:
        print(problem, file=sys.stderr)
        return 1
    print("message has no Co-authored-by trailer")
    return 0


def cmd_index(args) -> int:
    report = index_commits(args.repo.resolve(), args.since, args.evidence)
    write_report(args.out, report)
    _announce(args.out, None)
    return 0


def cmd_catalog(args) -> int:
    report = coordination()
    report["executed_unreviewed"] = False
    report["deadline_utc"] = SPRINT_DEADLINE
    report["sections"] = {}
    if args.panel:
        report["sections"]["panel"] = classify_panel(_load(args.panel))
    if args.settings:
        report["sections"]["settings_stress"] = classify_settings(_load(args.settings), args.settings_bins)
    if args.preview:
        report["sections"]["preview_contract"] = preview_contract(args.preview, args.settings_inputs)
    if args.control:
        report["sections"]["sprint_control"] = read_control(args.control)
    report["sections"]["build_slot_incident"] = incident_record()
    report["full_acceptance"] = False
    write_report(args.out, report)
    _announce(args.out, None)
    return 0


def cmd_sprint(args) -> int:
    report = coordination()
    report["control"] = read_control(args.control)
    report["full_acceptance"] = False
    write_report(args.out, report)
    _announce(args.out, None)
    return 0


def cmd_fingerprint(args) -> int:
    report = fingerprint(args.root, args.path)
    report["full_acceptance"] = False
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(f"fingerprint: {args.out}")
    print("full_acceptance: false")
    return 0


def _load(path: Path) -> dict:
    document = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(document, dict):
        raise PolicyError("JSON spec must be an object")
    return document


def _baseline() -> str:
    if not _BASELINE.is_file():
        raise PolicyError("BASELINE is missing; pass --since. This command does not rewrite history")
    text = _BASELINE.read_text(encoding="utf-8").strip()
    if len(text) < 7:
        raise PolicyError("BASELINE is not a revision; pass --since")
    return text


def _announce(out: Path, passed: bool | None) -> None:
    print(f"report: {out / 'report.json'}")
    print("full_acceptance: false")
    if passed is not None:
        print(f"passed: {str(bool(passed)).lower()}")
