#!/usr/bin/env python3
"""UKUI shortcut panel: every control reaches its backend, through the real bar, against private recording stubs; no live desktop.
Usage: python3 tests/e2e/openkylin-cc-actions.py LIB_TEST_BINARY EVIDENCE_DIR

Same isolation as tests/e2e/gnome-cc-native.py (whose stub table is reused): a PATH holding only recording stubs, a private HOME/XDG, no DISPLAY, no session bus,
one test thread. Controls only reach the dry-run hook, so the stubs may see status probes and never a setter; the script fails if one does.
"""
import hashlib, importlib.util, json, os, pathlib, shutil, subprocess, sys, tempfile

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("gnome_cc_native", HERE / "gnome-cc-native.py")
g = importlib.util.module_from_spec(spec); spec.loader.exec_module(g)


def main():
    binary = pathlib.Path(sys.argv[1]).resolve(); evidence = pathlib.Path(sys.argv[2]).resolve(); evidence.mkdir(parents=True, exist_ok=False)
    with tempfile.TemporaryDirectory(prefix="ukui-cc-actions-") as private:
        root = pathlib.Path(private)
        for sub in ("home", "config", "cache", "state", "data", "run", "bin", "themes"): (root / sub).mkdir(mode=0o700)
        calls = root / "calls.log"; calls.write_text("")
        for name, body in g.STUBS.items():
            stub = root / "bin" / name
            stub.write_text(f'#!/bin/sh\necho "{name} $*" >> "$GNOME_CC_CALLS"\n{body}\n'); stub.chmod(0o755)
        for tool in ("fc-list", "fc-match"):
            real = shutil.which(tool); assert real, f"{tool} is required to find fonts"
            (root / "bin" / tool).symlink_to(real)
        env = {"PATH": str(root / "bin"), "HOME": str(root / "home"), "XDG_CONFIG_HOME": str(root / "config"), "XDG_CACHE_HOME": str(root / "cache"), "XDG_STATE_HOME": str(root / "state"),
               "XDG_DATA_HOME": str(root / "data"), "XDG_RUNTIME_DIR": str(root / "run"), "DECEBAL_CONFIG": str(root / "config"), "DECEBAL_THEMES": str(root / "themes"),
               "DECEBAL_SYSTEM_THEMES": str(root / "themes"), "DECEBAL_SYSTEM_AUTOMATIC": "0", "DBUS_SESSION_BUS_ADDRESS": f"unix:path={private}/no-bus",
               "DBUS_SYSTEM_BUS_ADDRESS": f"unix:path={private}/no-system", "GNOME_CC_PRIVATE_SERVICES": "1", "GNOME_CC_CALLS": str(calls), "TMPDIR": str(root / "cache")}
        r = subprocess.run([str(binary), "menubar::ukui_cc_tests", "controlcenter::ukui", "--test-threads=1"], env=env, text=True, capture_output=True, timeout=600)
        (evidence / "test-output.txt").write_text(r.stdout + r.stderr); print(r.stdout[-1800:])
        if r.returncode != 0: print(r.stderr[-2000:]); sys.exit(r.returncode)
        seen = calls.read_text(); (evidence / "stub-service-calls.log").write_text(seen)
        for setter in g.SETTERS: assert setter not in seen, f"setter reached a stub service (dry-run bypass): {setter!r}\n{seen}"
    (evidence / "report.json").write_text(json.dumps({"binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "tests_run_with_actions": True}, indent=2))


main()
