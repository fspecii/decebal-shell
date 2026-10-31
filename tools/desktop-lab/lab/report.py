"""JSON and concise Markdown reports. Excerpts drop secret-looking lines."""

from __future__ import annotations

import json
from pathlib import Path
import re

_SECRET_LINE = re.compile(
    r"(?i)(secret|token|password|credential|api[_-]?key|authorization|cookie|private[_-]?key|BEGIN [A-Z ]*PRIVATE KEY)"
)


def redact_text(text: str) -> str:
    lines = []
    for line in text.splitlines():
        lines.append("[redacted]" if _SECRET_LINE.search(line) else line)
    return "\n".join(lines)


def write_report(out: Path, report: dict) -> None:
    out.mkdir(mode=0o700, parents=True, exist_ok=True)
    (out / "report.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    (out / "report.md").write_text(render_markdown(report))


def render_markdown(report: dict) -> str:
    lines = [
        f"# desktop-lab {report.get('name', 'report')}",
        "",
        f"full_acceptance: {str(report.get('full_acceptance', False)).lower()}",
        f"job_passed: {str(report.get('job_passed', False)).lower()}",
        f"hardware_acceptance: false",
    ]
    if report.get("result_class"):
        lines.append(f"result_class: {report['result_class']}")
    if report.get("kill_cause"):
        lines.append(f"kill_cause: {report['kill_cause']}")
    transport = report.get("display_transport")
    if isinstance(transport, dict):
        inode_note = ""
        if transport.get("inodes_owned_by_server"):
            inode_note = (
                f" filesystem_inode: {transport.get('filesystem_inode')}"
                f" abstract_inode: {transport.get('abstract_inode')}"
                f" inodes_owned_by_server: true"
            )
        lines.append(
            "display_transport: "
            f":{transport.get('number')} "
            f"filesystem_socket: {str(bool(transport.get('filesystem_socket'))).lower()} "
            f"abstract_socket: {str(bool(transport.get('abstract_socket'))).lower()} "
            f"displayfd_probe: {str(bool(transport.get('displayfd_probe'))).lower()}"
            f"{inode_note}"
        )
    containment = report.get("containment")
    if isinstance(containment, dict):
        lines.append(
            "containment: "
            f"{containment.get('kind')} "
            f"marker_scan_sufficient: {str(containment.get('claims_marker_scan_is_sufficient', False)).lower()}"
        )
    if report.get("errors"):
        lines.append("errors:")
        for error in report["errors"]:
            lines.append(f"- {redact_text(str(error))}")
    if report.get("limits"):
        lines.append("limits:")
        for limit in report["limits"]:
            lines.append(f"- {limit}")
    source = report.get("source")
    if isinstance(source, dict) and source.get("before"):
        lines.append(
            f"source_frozen: {str(source.get('frozen', False)).lower()} {source['before'].get('sha256', '')}"
        )
    build = report.get("build")
    if isinstance(build, dict):
        lines.append(f"build_exit: {build.get('exit_code')}")
        lines.append(f"features: {build.get('features')}")
    for binary in report.get("binaries") or []:
        lines.append(f"binary: {binary.get('path')} {binary.get('sha256')}")
    for image in report.get("screenshots") or []:
        lines.append(f"screenshot: {image.get('path')} {image.get('sha256')}")
    lines.append("")
    return "\n".join(lines)
