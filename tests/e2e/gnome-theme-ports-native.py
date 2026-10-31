#!/usr/bin/env python3
"""Real native panel/dock/control-centre captures of the GNOME ports on a private Xvfb.
Usage: python3 tests/e2e/gnome-theme-ports-native.py BIN_DIR THEMES_DIR EVIDENCE_DIR [WALLPAPER_DIR]

Applies each package with `decthemectl theme apply --no-rdwm` into a private config directory, runs the real decbar (and decdock when
the package enables it) and `decutil control-center toggle`, then captures the root window. Private HOME/XDG/bus, external probes
stubbed to fail (so absent hardware stays absent, nothing is faked), no window manager, no live display, no input injection.
WALLPAPER_DIR (optional, evidence only, never committed) holds `<wallhaven-id>.jpg` files named in
tests/fixtures/gnome-theme-ports/wallhaven-fragment.json and is painted on the private root window.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

from PIL import Image, ImageChops

HERE = Path(__file__).resolve().parents[2]
def loadable_themes(bin_dir, themes, scratch):
    """The themes folder to point decbar/decthemectl at. The packages need decebal 0.3.0 (the release with the GNOME look); on an older
    build the real folder is refused, so a copy with the floor lowered to this build is used (only theme.toml differs)."""
    probe = subprocess.run([str(bin_dir / "decthemectl"), "theme", "validate", str(themes / "gnome-adwaita")], capture_output=True, text=True, env={"PATH": "/usr/bin:/bin", "HOME": str(scratch), "XDG_CONFIG_HOME": str(scratch), "DECEBAL_CONFIG": str(scratch)})
    if "needs decebal" not in probe.stdout + probe.stderr:
        return themes
    import re
    version = re.search(r"this is ([0-9.]+)", probe.stdout + probe.stderr).group(1)
    lowered = scratch / "lowered-themes"
    for pkg in PORTS:
        shutil.copytree(themes / pkg, lowered / pkg)
        manifest = lowered / pkg / "theme.toml"
        manifest.write_text(re.sub(r'min_shell_version = "[0-9.]+"', f'min_shell_version = "{version}"', manifest.read_text()))
    return lowered


PORTS = {
    # id: (dock enabled, opaque band colour per mode). The dux band paints the `background` token and the item faces the `bar` token, so
    # the dark Adwaita band is #222226 with black item faces, not the all-black Shell panel (documented framework gap).
    "gnome-adwaita": (False, {"dark": (34, 34, 38), "light": (250, 250, 251)}),
    "gnome-catppuccin-dock": (True, {}),
}


def wait(predicate, label, tries=200):
    for _ in range(tries):
        try:
            value = predicate()
            if value:
                return value
        except (OSError, ValueError, subprocess.CalledProcessError):
            pass
        time.sleep(0.05)
    raise AssertionError(label)


BACKDROP = (107, 111, 133)  # plain #6b6f85 backdrop of the committed captures


def off_solid(image):
    return sum(1 for px in image.get_flattened_data() if px != BACKDROP)


def changed(a, b):
    diff = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v else 0)
    return diff.histogram()[255]


def sweep(private):
    """Terminate every process that was started with this run's private runtime directory (`theme apply` spawns decdock detached)."""
    needle = f"XDG_RUNTIME_DIR={private}/run".encode()
    for proc in Path("/proc").iterdir():
        if proc.name.isdigit() and int(proc.name) != os.getpid():
            try:
                if needle in (proc / "environ").read_bytes().split(b"\0"):
                    os.kill(int(proc.name), 15)
            except (OSError, ValueError):
                pass


def main():
    bin_dir, themes, evidence = (Path(p).resolve() for p in sys.argv[1:4])
    wallpapers = Path(sys.argv[4]).resolve() if len(sys.argv) > 4 else None
    evidence.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="gnome-port-themes-") as keep:
        themes = loadable_themes(bin_dir, themes, Path(keep))
        return run_all(bin_dir, themes, evidence, wallpapers)


def run_all(bin_dir, themes, evidence, wallpapers):
    fragment = json.loads((HERE / "tests/fixtures/gnome-theme-ports/wallhaven-fragment.json").read_text())
    report = {"binary_sha256": {}, "captures": []}
    for name in ["decbar", "decdock", "decutil", "decthemectl"]:
        report["binary_sha256"][name] = hashlib.sha256((bin_dir / name).read_bytes()).hexdigest()
    for theme_id, (has_dock, opaque) in PORTS.items():
        for mode in ["dark", "light"]:
            with tempfile.TemporaryDirectory(prefix="gnome-port-native-") as private:
                root = Path(private)
                for sub in ["home", "config", "cache", "state", "data", "run", "bin", "empty-themes"]:
                    (root / sub).mkdir(mode=0o700)
                for app in ["curl", "nmcli", "pactl", "wpctl", "playerctl", "bluetoothctl", "brightnessctl", "rdwmctl"]:
                    stub = root / "bin" / app
                    stub.write_text("#!/bin/sh\nexit 1\n")
                    stub.chmod(0o755)
                display = next(f":{n}" for n in range(352, 400) if not Path(f"/tmp/.X11-unix/X{n}").exists() and not Path(f"/tmp/.X{n}-lock").exists())
                env = dict(
                    os.environ,
                    HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"),
                    XDG_STATE_HOME=str(root / "state"), XDG_DATA_HOME=str(root / "data"), XDG_RUNTIME_DIR=str(root / "run"),
                    DECEBAL_CONFIG=str(root / "config"), DECEBAL_THEMES=str(themes), DECEBAL_SYSTEM_THEMES=str(root / "empty-themes"),
                    DECEBAL_BACKEND="x11", DECEBAL_SYSTEM_AUTOMATIC="0", DISPLAY=display,
                    DBUS_SESSION_BUS_ADDRESS=f"unix:path={private}/no-bus", DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={private}/no-system",
                    RDWMCTL=str(root / "bin/rdwmctl"), PATH=f'{root / "bin"}:{os.environ["PATH"]}',
                )
                for key in ["WAYLAND_DISPLAY", "RDWM_SOCK", "PULSE_SERVER"]:
                    env.pop(key, None)
                log = (evidence / f"{theme_id}-{mode}.log").open("w")
                server = subprocess.Popen(["Xvfb", display, "-screen", "0", "1920x1080x24", "-nolisten", "tcp"], env=env, stdout=log, stderr=log)
                children = []
                try:
                    def run(*args, timeout=30, check=True):
                        return subprocess.run(args, env=env, text=True, capture_output=True, check=check, timeout=timeout).stdout.strip()
                    wait(lambda: server.poll() is None and run("xdotool", "getdisplaygeometry"), "private Xvfb ready")
                    wallpaper = None
                    solid = True
                    if wallpapers:
                        wallpaper = wallpapers / f'{fragment["themes"][theme_id][mode]["image_id"]}.jpg'
                    backdrop = root / "backdrop.png"
                    if wallpaper and wallpaper.exists():
                        solid = False
                        source = Image.open(wallpaper).convert("RGB")
                        scale = max(1920 / source.width, 1080 / source.height)  # fill: cover and centre-crop
                        source = source.resize((round(source.width * scale), round(source.height * scale)), Image.LANCZOS)
                        left, top = (source.width - 1920) // 2, (source.height - 1080) // 2
                        source.crop((left, top, left + 1920, top + 1080)).save(backdrop)
                    else:
                        Image.new("RGB", (1920, 1080), BACKDROP).save(backdrop)
                    # A real backdrop window (not the root pixmap): the compositor paints windows reliably, the root only when a pixmap is published.
                    children.append(subprocess.Popen(["display", "-backdrop", "-title", "gnome-port-backdrop", str(backdrop)], env=env, stdout=log, stderr=log))
                    time.sleep(0.7)
                    # `theme apply` starts decdock itself when the package enables the dock, after the backdrop window exists so the dock maps above it.
                    run(str(bin_dir / "decthemectl"), "theme", "apply", theme_id, "--no-rdwm")
                    cfg_path = root / "config/decebal.json"
                    cfg = json.loads(cfg_path.read_text())
                    cfg["theme"]["mode"] = mode
                    cfg_path.write_text(json.dumps(cfg))
                    # Private XRender compositor (the bar canvas is transparent and only blends under a compositor), with the repository's
                    # scoped blur rule for surfaces that request glass.
                    (root / "picom.conf").write_text('backend = "xrender";\nvsync = false;\nuse-damage = false;\nshadow = false;\nfading = false;\n'
                                                     'blur: { method = "gaussian"; size = 12; deviation = 5.0; };\n'
                                                     'rules = ( { blur-background = false; }, { match = "_DECEBAL_BLUR@ = 1"; blur-background = true; } );\n')
                    children.append(subprocess.Popen(["picom", "--config", str(root / "picom.conf")], env=env, stdout=log, stderr=log))
                    time.sleep(1.0)
                    children.append(subprocess.Popen([str(bin_dir / "decbar")], env=env, stdout=log, stderr=log))
                    wait(lambda: run("xdotool", "search", "--class", "decbar", check=False) or run("xdotool", "search", "--name", "decebal-bar", check=False), "bar mapped")
                    time.sleep(2.0)
                    if has_dock:
                        # `theme apply` started decdock itself (single instance per private runtime dir); wait until it has painted.
                        bottom = (560, 900, 1360, 1080)

                        def dock_drawn():
                            run("import", "-window", "root", str(evidence / "probe.png"))
                            return off_solid(Image.open(evidence / "probe.png").convert("RGB").crop(bottom)) > 3000

                        if solid:
                            wait(dock_drawn, "dock painted", tries=200)
                            (evidence / "probe.png").unlink()
                        else:
                            time.sleep(3.0)
                    time.sleep(1.0)
                    shot = evidence / f"{theme_id}-{mode}-desktop.png"
                    run("import", "-window", "root", str(shot))
                    image = Image.open(shot).convert("RGB")
                    band = image.getpixel((600, 16))
                    below = image.getpixel((600, 60))
                    if mode in opaque:
                        assert band == opaque[mode], f"{theme_id} {mode}: the panel band is the pinned panel colour, got {band}"
                    # panel height is 32 px: the last band row is y=31, the first desktop row y=32
                    column = [image.getpixel((600, y)) for y in range(0, 48)]
                    edge = next((y for y in range(1, 48) if column[y] != column[0]), None)
                    assert edge == 32, f"{theme_id} {mode}: panel thickness {edge}"
                    entry = {"theme": theme_id, "mode": mode, "desktop": shot.name, "sha256": hashlib.sha256(shot.read_bytes()).hexdigest(), "band_pixel": band, "panel_thickness": edge,
                             "wallpaper": str(wallpaper.name) if wallpaper and wallpaper.exists() else None}
                    # Control centre: the native popup is a long-lived process; capture it, then end it.
                    cc = subprocess.Popen([str(bin_dir / "decutil"), "control-center", "toggle"], env=env, stdout=log, stderr=log)
                    children.append(cc)
                    time.sleep(2.0)
                    shot_cc = evidence / f"{theme_id}-{mode}-control-center.png"
                    run("import", "-window", "root", str(shot_cc))
                    entry["control_center"] = shot_cc.name
                    entry["control_center_sha256"] = hashlib.sha256(shot_cc.read_bytes()).hexdigest()
                    entry["control_center_changed_pixels"] = changed(image, Image.open(shot_cc).convert("RGB"))
                    bottom = (560, 900, 1360, 1080)
                    # Dock presence is asserted against the plain backdrop only; the wallpaper runs are evidence captures.
                    entry["dock_pixels_off_backdrop"] = off_solid(image.crop(bottom)) if solid else None
                    if solid and has_dock:
                        assert entry["dock_pixels_off_backdrop"] > 3000, f"{theme_id} {mode}: dock painted in the bottom centre"
                    elif solid:
                        assert entry["dock_pixels_off_backdrop"] == 0, f"{theme_id} {mode}: dock is disabled, backdrop untouched"
                    assert entry["control_center_changed_pixels"] > 5000, f"{theme_id} {mode}: control centre opened"
                    report["captures"].append(entry)
                finally:
                    sweep(root)
                    for child in children:
                        child.terminate()
                    for child in children:
                        try:
                            child.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            child.kill()
                    server.terminate()
                    server.wait(timeout=10)
                    log.close()
    (evidence / "report.json").write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
