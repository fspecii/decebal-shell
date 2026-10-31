#!/usr/bin/env python3
"""Proof of the mode-aware window style against REAL old and new rdwm builds, in a private fixture (nothing live is touched).

Usage: python3 tests/e2e/openkylin-decoration-proof.py DECTHEMECTL NEW_RDWM_DIR OLD_RDWM_DIR OUT_JSON
DECTHEMECTL must implement the mode-specific window-style contract (decoration.style_light/style_dark/fallback_style_*). *_RDWM_DIR hold `rdwm` and `rdwmctl`
(read-only; the new one lists the measured `openkylin`/`openkylin-dark` styles, the old one does not). For each package x mode x window manager: a private Xvfb on a
reserved display >= 700 (ownership proven in both socket namespaces, see openkylin_fixture.py), a private 0700 XDG_RUNTIME_DIR (rdwm creates its socket there; the
user's own socket is never opened), the real rdwm started inside it, then the real `decthemectl apply`. Recorded: the apply's exit code and message, the style in the
rdwm config, the style the RUNNING rdwm reports, and the persistent decoration_compat record. Expected: the new WM gets the pair for the mode and no record; the old WM
gets the proven fallback, exit 0, and a persistent record naming the wanted pair.
"""
import hashlib, importlib.util, json, os, pathlib, subprocess, sys, tempfile, time

ROOT = pathlib.Path(__file__).resolve().parents[2]
_s = importlib.util.spec_from_file_location("openkylin_fixture", pathlib.Path(__file__).with_name("openkylin_fixture.py")); fx = importlib.util.module_from_spec(_s); _s.loader.exec_module(fx)
DT, NEW, OLD, OUT = (pathlib.Path(a).resolve() for a in sys.argv[1:5])
sha = lambda p: hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
WANT = {"light": ("openkylin", "adwaita-light"), "dark": ("openkylin-dark", "adwaita")}
used, results = set(), []


def case(package, mode, wm_name, wmdir):
    with tempfile.TemporaryDirectory(prefix="okd-") as td:
        w = pathlib.Path(td)
        for d in ("home/.config/rdwm", "tmp", "state", "run", "dcfg"): (w / d).mkdir(parents=True)
        (w / "run").chmod(0o700)
        n = fx.free_display(used)
        env = dict(PATH=f"{wmdir}:/usr/bin:/bin", LANG="C.UTF-8", HOME=str(w / "home"), XDG_CONFIG_HOME=str(w / "home/.config"), XDG_DATA_HOME=str(w / "home/.local/share"),
                   XDG_CACHE_HOME=str(w / "home/.cache"), XDG_STATE_HOME=str(w / "state"), XDG_RUNTIME_DIR=str(w / "run"), TMPDIR=str(w / "tmp"), DISPLAY=f":{n}",
                   RDWM_SOCK=str(w / "run/rdwm.sock"), DECEBAL_CONFIG=str(w / "dcfg"), DECEBAL_THEMES=str(ROOT / "themes"), DECEBAL_SYSTEM_THEMES=str(w / "home"),
                   DECEBAL_PALETTES=str(ROOT / "palettes"), DECEBAL_SYSTEM_AUTOMATIC="0", DBUS_SESSION_BUS_ADDRESS="unix:path=/nonexistent")
        procs = []
        try:
            procs.append(fx.start_xvfb(n, "1280x720", env))
            assert env["DISPLAY"] == f":{n}"
            procs.append(subprocess.Popen([str(wmdir / "rdwm")], env=env, stdout=(w / "rdwm.log").open("w"), stderr=subprocess.STDOUT))
            for _ in range(80):
                if (w / "run/rdwm.sock").is_socket(): break
                time.sleep(0.25)
            assert (w / "run/rdwm.sock").is_socket(), f"rdwm did not create its private socket (see its log)"
            run = lambda *a: subprocess.run(a, env=env, text=True, capture_output=True)
            assert run(str(DT), "set", "mode", mode).returncode == 0
            ap = run(str(DT), "theme", "apply", package)
            cfg = w / "home/.config/rdwm/config.toml"
            style = next((l.split("=", 1)[1].strip().strip('"') for l in cfg.read_text().splitlines() if l.strip().startswith("style")), None) if cfg.exists() else None
            radius = next((l.split("=", 1)[1].strip() for l in cfg.read_text().splitlines() if l.strip().startswith("radius")), None) if cfg.exists() else None
            sch = run(str(wmdir / "rdwmctl"), "schema")
            listed = None
            try:
                k = [x for x in json.loads(sch.stdout)["keys"] if x["section"] == "decoration" and x["key"] == "style"][0]; listed = len(k["choices"])
            except Exception: pass
            conf = run(str(wmdir / "rdwmctl"), "config")
            try: now = json.loads(conf.stdout); now = now.get("config", now)["decoration"]["style"]
            except Exception: now = "unknown"
            try: rec = json.loads((w / "dcfg/decebal.json").read_text()).get("decoration_compat")
            except Exception: rec = None
            r = dict(package=package, mode=mode, wm=wm_name, apply_exit=ap.returncode, apply_message=(ap.stdout + ap.stderr).strip()[:400], config_style=style, config_radius=radius,
                     wm_lists_styles=listed, wm_reports_style=now, record=rec)
            want, fall = WANT[mode]
            if wm_name == "new":
                assert (ap.returncode, style, now, rec) == (0, want, want, None), r
            else:
                assert ap.returncode == 0 and style == fall and now == fall, r
                assert rec and rec.get("desired") == want and rec.get("applied") == fall, r
            if package == "openkylin-classic": assert radius == "0", r
            return r
        finally:
            for p in reversed(procs):
                p.terminate()
                try: p.wait(timeout=8)
                except subprocess.TimeoutExpired: p.kill()


for package in ("openkylin", "openkylin-classic"):
    for mode in ("light", "dark"):
        for name, d in (("new", NEW), ("old", OLD)):
            results.append(case(package, mode, name, d)); print(package, mode, name, results[-1]["config_style"], results[-1]["wm_reports_style"], results[-1]["record"] is not None)
OUT.write_text(json.dumps(dict(
    decthemectl=dict(path=str(DT), sha256=sha(DT)), rdwm_new=dict(dir=str(NEW), rdwm=sha(NEW / "rdwm"), rdwmctl=sha(NEW / "rdwmctl")), rdwm_old=dict(dir=str(OLD), rdwm=sha(OLD / "rdwm"), rdwmctl=sha(OLD / "rdwmctl")),
    results=results), indent=1))
print("PASS", len(results), "applies against real old and new rdwm")
