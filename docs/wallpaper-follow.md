# Follow theme wallpaper

Wallpaper stays independent by default. Turn on **Wallpaper → Follow theme
wallpaper** to use a matching curated Wallhaven photograph when applying a
complete desktop theme. Turning it on also prepares the photo for the theme and
mode currently selected. Changing light/dark mode in Settings picks its matching
variant. Choosing a separate palette keeps that palette; it does not cancel a
photo already being prepared for the same desktop design.

When **Match wallpaper colours** is enabled, a completed photo also refreshes its
stored colour cache from the current source. A fixed image or colour source stays
fixed; turning colour matching off leaves it untouched. The refresh preserves
palette, radius and other matching preferences, and does not request another
photo when the extracted brightness changes the light/dark variant. If the photo
was applied but colour refresh fails, Settings reports **Wallpaper applied;
colour refresh failed**. The photo stays applied; fix the colour source or use
`decthemectl dynamic refresh` to retry the colour update.

Photos are prepared in the background. A previously reviewed original can be
reused offline only if its bytes match the catalogue's SHA-256 and image bounds.
Other cached downloads need the worker's previous successful validation receipt.
New downloads use the same SFW/photography metadata and image checks as the
Wallhaven picker. Cache reuse is labelled as reviewed/previously validated;
it does not claim a new API metadata check.

If Wallhaven is offline, returns403, changes a safety classification, or the
download/config check fails, the theme remains applied and the current wallpaper
is kept. Settings shows the failure once as a persistent notice, including when
the theme gallery was scrolled down. Wallpaper also keeps the error and a Retry
button. No generated substitute is used. Custom/legacy themes without a curated
photograph report that limitation instead of guessing a default.

Manual Wallpaper choices cancel a pending photo. A newer theme or mode
supersedes older work, so late responses cannot replace the user's newer choice.
A plain palette/accent/radius change is preserved while the matching photo is
prepared. Turning the switch off keeps the current image and leaves future
wallpaper choices independent. Theme preview/undo preserve the independent
wallpaper preference; previews never start downloads.

This follows static rdwm wallpapers. If a live video wallpaper is active, turn it
off first; the worker reports the conflict rather than quietly applying a hidden
photo behind the video. Other Wayland wallpaper providers and automatic OS
light/dark transitions are not yet integrated; reapply the theme to resolve a
new automatic mode.

```
decthemectl wallpaper follow on
decthemectl wallpaper follow off
decthemectl wallpaper follow retry
decthemectl wallpaper follow status --json
```

Developer validation (private paths, fake rdwm IPC and loopback HTTP only):

```
cargo test --lib wallpaper_follow
PROFILE=debug python3 tests/e2e/wallpaper-follow.py
```

A reviewable original-cache import is available for an already curated asset
folder. It is a dry-run unless `--apply` is supplied. Unknown bytes, mismatched
dimensions, symlinks and differing destination files are skipped. Photos are
copied to the user's independent background folder, never into source themes.

```
decthemectl --import-reviewed-wallpapers /path/to/reviewed/originals
# Inspect the JSON manifest before copying exact reviewed bytes:
decthemectl --import-reviewed-wallpapers /path/to/reviewed/originals --apply
```
