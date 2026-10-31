# Settings preview pressure

Private Xvfb run of the immutable `1ec25dfe0f4d2c45f748c581665c739e38cb3fb1` binaries. The harness is `tests/e2e/preview-pressure.py`. It does not build Rust, does not edit preview or Settings code, and does not change host mounts or quotas.

This is one owned X server per case. It is not a desktop-environment benchmark. `online_cpus` from the recorder is informational. CPU below is percent of one core.

## Command

`PREVIEW_PRESSURE_EVIDENCE` must be a directory that does not exist yet. The script creates it with an exclusive `mkdir`. If the path already exists, including a symlink, the script exits 2 and does not write, delete, or replace anything inside it. The default path `/var/tmp/decebal-preview-pressure-20261031` is the first run and is now refused. Leave it in place.

```sh
PREVIEW_PRESSURE_EVIDENCE=/var/tmp/new-preview-pressure-evidence \
  python3 tests/e2e/preview-pressure.py
```

`PREVIEW_PRESSURE_ARTIFACT` is optional. When set, it must be an absolute directory that contains `bin/` and `SHA256SUMS`. `decsettings` and `decthemectl` must match the pinned digests below, both in that manifest and on disk. The theme tree is the sibling `<artifact-name>.assets/share/decebal/themes`. Unset, the harness uses the 1ec artifact under `/var/tmp/decebal-release-engineering-20261031/artifacts/`.

Safety fixtures, with no Settings launch:

```sh
python3 tests/e2e/preview-pressure-safety.py
```

Exit 0 means every case stayed in the observed class. Exit 1 is a product fault and writes `min-repro.json` under the new evidence directory. Exit 2 is an orchestration fault (existing evidence path, hash mismatch, X server the harness does not own, or a host tool failure). A hash mismatch writes `min-repro.json` only after the new directory has been created.

## Artifacts

Bins, mode `0555`, checked against the pinned `SHA256SUMS` before launch:

| Binary | SHA256 |
| --- | --- |
| `decsettings` | `23a9f3fb4ad2e1a150acef9b0a96731cd7a8ea10359dde6319bfade7772ff98c` |
| `decthemectl` | `566d00be02a3ed6059ac28d2bc288b03bfc66018e4f51298ef2e0eb1731891b3` |

`DECEBAL_THEME_PREVIEW_BIN` is that `decthemectl`. `FONTCONFIG_FILE` lists only these directories under the sibling `.assets/share/decebal/themes` tree: `dux-lavender/fonts`, `gnome-adwaita/fonts`, `gnome-catppuccin-dock/fonts`, `sweeteh/fonts`. The fixture copies ten small themes into a private directory (`nord`, `dracula`, `solarized`, `graphite`, `high-contrast`, `macos`, `windows11`, `caelestia`, `mountain-neon-dot`, `glyph-rice`). Wallpaper is a local 32×18 PNG. `curl`, `wget`, and `aria2c` on `PATH` are stubs that exit 1.

Each case uses `tests/e2e/private_x11.py`: one Xvfb whose `/tmp/.X11-unix` socket is owned by that PID, private `HOME` / XDG / runtime, and a private bus path. Inherited `DISPLAY`, `WAYLAND_DISPLAY`, and `XAUTHORITY` are removed. Cleanup opens a pidfd, then signals a scanned child only when that pid's live parent is still the owned settings PID. A foreign parent is not signaled.

## Recorded run

`/var/tmp/decebal-preview-pressure-20261031-r2/summary.json`, exit 0, on 2026-10-31. No `min-repro.json`. The earlier directory `/var/tmp/decebal-preview-pressure-20261031` was not modified. Preview PNGs the renderer wrote are 1920×1200. None of the sampled files were 1920×1080.

### Rapid Themes / Wallpaper / Appearance

Six sidebar switches, alternating Appearance and Wallpaper, each followed by the Themes tab when Appearance was reached. All six landed on the requested pane. Latencies: 0.169s, then 0.106s five times. The window stayed mapped. Frames went from 8 to 20. Theme-card controls stayed at 103 on Appearance. Preview PNGs went 0, 0, 1, 2, 2, 2. The last Appearance sample had `theme_previews_painted` 2. The run did not see the thumbnail count fall to zero, and it did not see Settings exit on its own.

Window capture `rapid_nav/rapid.png` is 1100×740.

Recorder exit 5 because `decthemectl` exited before two intervals (CPU status `no_measured_intervals`). Pid 2428264 `decsettings` was measured: 9.0011% of one core (min 1.9996, max 16.0067), RSS 17141760, PSS 15354880. Pid 2428371 `decthemectl` last RSS 82137088, PSS 79949824.

### Warm cache

Cold first PNG at 0.312s. Four files, all 1920×1200 (107533, 39916, 54000, and 39976 bytes). After a stop and a second Appearance launch, all four kept the same inode, mtime, and size. Time until an existing cache file was visible on the warm launch: 0.003s. Warm sample: pane Appearance, 4 previews painted, 103 cards.

Recorder exit 5 for the same reason. Pid 2428480 `decsettings`: 22.993% of one core (min 21.9939, max 23.992), RSS 24596480, PSS 23052288. Pid 2428549 `decthemectl` had already exited: last RSS 27312128, PSS 25165824, no CPU interval.

Captures `warm_cache/cold.png` and `warm_cache/warm.png` are 1100×740.

### Cache not writable

Private `cache/decebal` and `theme-previews` were mode `0555`, then restored to `0700`. This is EACCES on a private directory. ENOSPC was not exercised. Over 7.709s the window stayed mapped, frames reached 6, painted previews stayed 0, and zero PNGs were written. Cards: 103. Capture `cache_eacces/eacces.png` is the binary's default 940×680 (this case does not resize).

Recorder pid 2428742 `decsettings`: 0.0% of one core (min 0.0, max 0.0, status ok), RSS 14196736, PSS 12111872.

### Cache seed

Forty local 4×2 PNGs were written into the private cache before launch. That is a seed so later files can be counted. It is not a memory test and not a disk-full test. After 7.719s the directory held 44 PNGs. The four new files are 1920×1200. The window stayed mapped, frames reached 6, and 4 previews were painted. ENOSPC was not exercised.

The recorder ran after that wait. Pid 2429033 `decsettings`: 0.0% of one core (min 0.0, max 0.0, status ok), RSS 16228352, PSS 14657536. The renderer was no longer alive.

### Close with a queued preview

Preview work was visible (painted cards or a cache PNG). The harness then sent `SIGTERM` through the settings pidfd (pid 2429516) and its child (pid 2429540). This is not a user closing the window from the GUI. Exit status -15. `SIGKILL` was not used. The window was gone. No child remained.

## Limits

The fixture is ten themes, not the full artifact tree. Samples are one second. Renderer CPU percent was not measured in this run: the worker exited before two recorder intervals. RSS and PSS above are last values for the named PIDs only. The close case is a pidfd `SIGTERM`, not a GUI close. The forty tiny PNGs are a cache seed, not memory or disk pressure. ENOSPC was not exercised.
