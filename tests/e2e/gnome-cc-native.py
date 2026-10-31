#!/usr/bin/env python3
"""GNOME quick-settings look: native painter and action tests against private fake services; no live desktop.
Usage: python3 tests/e2e/gnome-cc-native.py LIB_TEST_BINARY EVIDENCE_DIR

LIB_TEST_BINARY is the compiled `decebal` library test executable (`cargo test --lib --no-run`). It is run with a PATH that holds
only recording stub services, a private HOME/XDG, no DISPLAY / Wayland / session bus, and `GNOME_CC_PRIVATE_SERVICES` set so that
the probe-dependent action tests run. Controls only ever reach the existing dry-run hook (`sys::set_dry_run`), so the stubs must see
status probes and never a setter: the script fails if a setter shows up in their call log.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

# status probes the stubs answer (the real tools are never reached)
STUBS = {
    "decnotify": 'case "$*" in "dnd status") echo off;; *) ;; esac',
    "omarchy": 'case "$*" in "toggle nightlight --status") echo \'{"enabled":false,"temperature":null}\';; *) ;; esac',
    "brightnessctl": 'case "$*" in "-m -c backlight") echo "intel_backlight,backlight,400,40%,1000";; *) ;; esac',
    "wpctl": 'case "$1" in get-volume) echo "Volume: 0.50";; *) ;; esac',
    "pactl": "exit 1",
    "nmcli": "exit 1",
    "bluetoothctl": "exit 1",
    "playerctl": "exit 1",
    "makoctl": "exit 1",
    "dunstctl": "exit 1",
    "decutil": "exit 0",
    "decsettings": "exit 0",
    "desktop-style": "exit 0",
    "nm-connection-editor": "exit 0",
}
SETTERS = ["radio wifi", "power on", "power off", "toggle nightlight\n", "dnd toggle", "set-volume", "brightnessctl -q", "play-pause", "idle-inhibit toggle", "gamemode toggle", "record toggle", "set mode"]


def main():
    binary = Path(sys.argv[1]).resolve()
    evidence = Path(sys.argv[2]).resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="gnome-cc-native-") as private:
        root = Path(private)
        for sub in ["home", "config", "cache", "state", "data", "run", "bin", "themes"]:
            (root / sub).mkdir(mode=0o700)
        calls = root / "calls.log"
        calls.write_text("")
        for name, body in STUBS.items():
            stub = root / "bin" / name
            stub.write_text(f'#!/bin/sh\necho "{name} $*" >> "$GNOME_CC_CALLS"\n{body}\n')
            stub.chmod(0o755)
        # Font discovery needs the read-only `fc-list` / `fc-match` (never a setter); every other tool is a recording stub.
        for tool in ("fc-list", "fc-match"):
            real = shutil.which(tool)
            assert real, f"{tool} is required to find fonts"
            (root / "bin" / tool).symlink_to(real)
        env = {
            "PATH": str(root / "bin"), "HOME": str(root / "home"), "XDG_CONFIG_HOME": str(root / "config"), "XDG_CACHE_HOME": str(root / "cache"),
            "XDG_STATE_HOME": str(root / "state"), "XDG_DATA_HOME": str(root / "data"), "XDG_RUNTIME_DIR": str(root / "run"),
            "DECEBAL_CONFIG": str(root / "config"), "DECEBAL_THEMES": str(root / "themes"), "DECEBAL_SYSTEM_THEMES": str(root / "themes"),
            "DECEBAL_SYSTEM_AUTOMATIC": "0", "DBUS_SESSION_BUS_ADDRESS": f"unix:path={private}/no-bus", "DBUS_SYSTEM_BUS_ADDRESS": f"unix:path={private}/no-system",
            "GNOME_CC_PRIVATE_SERVICES": "1", "GNOME_CC_CALLS": str(calls), "GNOME_CC_EVIDENCE": str(evidence), "TMPDIR": str(root / "cache"),
        }
        log = evidence / "test-output.txt"
        result = subprocess.run([str(binary), "controlcenter::gnome", "menubar::gnome_cc_tests", "--test-threads=1"], env=env, text=True, capture_output=True, timeout=600)
        log.write_text(result.stdout + result.stderr)
        print(result.stdout[-1500:])
        if result.returncode != 0:
            print(result.stderr[-2000:])
            sys.exit(result.returncode)
        seen = calls.read_text()
        (evidence / "stub-service-calls.log").write_text(seen)
        for setter in SETTERS:
            assert setter not in seen, f"setter reached a stub service (dry-run bypass): {setter!r}\n{seen}"
        assert "decnotify dnd status" in seen and "brightnessctl -m -c backlight" in seen, "status probes were answered by the private stubs"
    report = {"binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "pngs": {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(evidence.glob("*.png"))}}
    (evidence / "report.json").write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
