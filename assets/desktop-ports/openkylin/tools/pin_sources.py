#!/usr/bin/env python3
"""Write assets/desktop-ports/openkylin/sources.json from the local upstream clones (read-only git queries).

Usage: python3 -I pin_sources.py UPSTREAM_DIR OUT_JSON
"""
import json, subprocess, sys, pathlib

up = pathlib.Path(sys.argv[1]).expanduser()

def git(repo, *a):
    return subprocess.run(["git", "-C", str(up / repo), *a], capture_output=True, text=True).stdout.strip()

# repo, pinned branch, role, licence evidence (path in the repo that states it), spdx as stated there
REPOS = [
 ("ukui-panel", "openkylin/huanghe", "taskbar: classic panel, island panel, top bar, task manager, tray, calendar", "", ""),
 ("ukui-globaltheme", "openkylin/huanghe", "global themes v11, Light-Seeking, HeYin, Classic: theme.conf + light/dark CSS token tables", "", ""),
 ("ukui-themes", "debian/unstable", "icon packs (default/classical/fashion), cursor themes dark-sense + blue-crystal with hotspot tables, GTK ukui-white/black", "", ""),
 ("openkylin-theme", "openkylin/nile", "newer snapshot of the same icon packs and cursors (nile)", "", ""),
 ("dmz-cursor-theme", "openkylin/yangtze", "DMZ-White / DMZ-Black cursors used by the HeYin global theme", "", "debian/copyright states CC-BY-SA-3.0 for the theme (Novell/Jakub Steiner lineage) and GPL-2+ for renderpngs.py"),
 ("qt5-ukui-platformtheme", "openkylin/huanghe", "Qt style/platform theme: widget parameters, style helper", "", ""),
 ("ukui-control-center", "openkylin/huanghe", "Settings app: client-side titlebar and window button metrics", "", ""),
 ("kwin", "openkylin/nile", "the real UKUI window decoration: kwin `src/plugins/kdecorations/ukui` (38 px header, 30 px buttons, radius 12) and its ukui-base icons", "", "plugin LICENSE is LGPL-3; kwin debian/copyright mixes GPL-2+/LGPL-2.1+3"),
 ("ukui-kwin", "debian/unstable", "KWin fork: window manager, Aurorae decoration plugin", "", ""),
 ("ukui-kwin-effects", "openkylin/yangtze", "task view / window switcher effects", "", ""),
 ("ukui-window-switch", "debian/unstable", "alt-tab window switcher", "", ""),
 ("ukui-sidebar", "4.10-dev", "notification centre / sidebar", "", ""),
 ("ukui-notification-daemon", "debian/unstable", "notification popups", "", ""),
 ("ukui-search", "openkylin/huanghe", "global search UI", "", ""),
 ("ukui-session-manager", "openkylin/huanghe", "session / logout dialog", "", ""),
 ("ukui-power-manager", "openkylin/huanghe", "power / battery", "", ""),
 ("ukui-system-monitor", "openkylin/huanghe", "process monitor", "", ""),
 ("ukui-screensaver", "debian/unstable", "lock screen", "", ""),
 ("ukui-settings-daemon", "openkylin/huanghe", "theme/font/cursor gsettings plumbing", "", ""),
 ("ukui-system-appwidget", "debian/unstable", "the only desktop widget UKUI ships: the time appwidget (448 x 162)", "", ""),
 ("ukui-quick-widgets", "debian/unstable", "panel/sidebar widgets: calendar, digital clock, netspeed (reference only)", "", ""),
 ("ukui-quick", "ai-test", "QML widget framework used by the panel", "", ""),
 ("kylin-nm", "0519", "network manager applet", "", ""),
 ("peony", "3.20.x.x", "file manager", "", ""),
 ("ukui-clock", "openkylin/nile", "clock app", "", ""),
 ("openkylin-wallpapers", "openkylin/nile", "upstream wallpapers (CC-BY-SA-3.0); inspected only, NOT shipped (policy: Wallhaven candidates)", "", "CC-BY-SA-3.0 declared for all files; attribution/share-alike apply; not shipped here"),
 ("ukui-wallpapers", "develop", "more upstream wallpapers (licence NOASSERTION on GitHub); inspected only, NOT shipped", "", "CC-BY-SA-3.0 declared; per-image authorship not verified; not shipped here"),
 ("kdecoration", "openkylin/yangtze", "KDE KDecoration API fork (no UKUI-specific decoration found)", "", ""),
]
out = {"organisation": "https://github.com/openkylin", "mirror_note": "GitHub org is a read-only mirror last pushed 2025-04; UKUI development continues on gitee.com/openkylin (not reachable via the task's entry point).", "repos": []}
for name, branch, role, lic_path, spdx in REPOS:
    rev = git(name, "rev-parse", "HEAD")
    declared = sorted({l.split(":", 1)[1].strip() for l in git(name, "show", "HEAD:debian/copyright").splitlines() if l.startswith("License:") and l.split(":", 1)[1].strip()})
    files = [f for f in git(name, "ls-tree", "--name-only", "HEAD").splitlines() if f.upper().startswith(("LICENSE", "COPYING"))]
    # git blob ids of the licence texts that exist (root LICENSE*/COPYING* files and debian/copyright), so a reader can verify the exact bytes that were declared
    blobs = {}
    for f in files + ["debian/copyright"]:
        oid = subprocess.run(["git", "-C", str(up / name), "rev-parse", "-q", "--verify", f"HEAD:{f}"], capture_output=True, text=True).stdout.strip()
        if oid: blobs[f] = oid
    out["repos"].append({
        "repo": f"https://github.com/openkylin/{name}", "branch": branch, "commit": rev,
        "commit_date": git(name, "log", "-1", "--format=%cs"),
        "role": role, "licence_declared_in_debian_copyright": declared, "licence_files_at_root": files, "licence_blobs": blobs,
        "note_on_licence": spdx,
    })
trees = {}
for repo, path in [("kwin", "src/plugins/kdecorations/ukui"), ("openkylin-theme", "icons/cursors/dark-sense"), ("openkylin-theme", "icons/cursors/blue-crystal"), ("ukui-themes", "icons/cursors/dark-sense"), ("ukui-themes", "icons/cursors/blue-crystal"),
                   ("ukui-themes", "icons/ukui-icon-theme-default"), ("ukui-themes", "icons/ukui-icon-theme-classical"), ("ukui-themes", "icons/ukui-icon-theme-fashion"),
                   ("openkylin-theme", "icons/ukui-icon-theme-default"), ("openkylin-theme", "icons/ukui-icon-theme-classical"), ("openkylin-theme", "icons/ukui-icon-theme-fashion"),
                   ("dmz-cursor-theme", "DMZ-White"), ("dmz-cursor-theme", "DMZ-Black"), ("ukui-globaltheme", "globaltheme")]:
    ls = git(repo, "ls-tree", "-r", "-l", "HEAD", path).splitlines()
    size = sum(int(l.split()[3]) for l in ls if l.split()[3].isdigit())
    trees[f"{repo}:{path}"] = {"git_tree": git(repo, "rev-parse", f"HEAD:{path}"), "files": len(ls), "bytes": size}
out["unresolved_provenance"] = [{"what": "UKUI icon packs (ukui-icon-theme-default/classical/fashion)", "evidence": "index.theme: `Comment=Icon theme for UKUI ,from moka.`",
                                  "status": "UNRESOLVED: part of the artwork may descend from the Moka icon theme; its licence and whether openkylin-theme may relicense it were neither researched nor assumed. The GPL-3 above is what debian/copyright declares, not a verification."}]
out["verified_trees"] = trees
pathlib.Path(sys.argv[2]).write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n")
print(len(out["repos"]), "repos,", len(trees), "trees")
