# Orbit Glass and Paper Signal

Research checked 2026-10-29. Reddit vote counts below are indexed snapshots, not fixed ratings. These are original desktop designs inspired by the compositional choices in popular posts, with original wallpaper artwork. No Reddit image, logo, dotfile, or application screenshot is included in either package.

## References inspected

- [[i3-gaps] Glassmorphism here I come](https://www.reddit.com/r/unixporn/comments/tc3kd0/) — 2022-03-11, indexed +2,317 votes. The actual screenshot uses large translucent rounded terminal surfaces against bold geometric wallpaper and leaves extensive negative space. Orbit adopts that separation of geometric backdrop and quiet foreground, using its own orbital construction.
- [[KDE] A Very Glassy Rice](https://www.reddit.com/r/unixporn/comments/1nvj0ks/) — 2025-10-01, indexed +853 votes. The actual screenshot uses a narrow floating top capsule and anchored lower navigation. Orbit adopts floating navigation and restrained edge definition. Its panels remain substantially opaque for legibility; the renderer does not provide the reference's refraction/godray effects.
- [[DWM] Minimalism](https://www.reddit.com/r/unixporn/comments/1nwcjxc/) — 2025-10-02, indexed +449 votes. The actual screenshot uses a warm paper field, flat rectangular windows and ample breathing room. Paper Signal adapts that editorial restraint with a functional thin panel and original geometric paper artwork.

## Distinct desktop structures

Orbit Glass uses a 28px floating bar, compact 46px magnifying Mac dock, rounded Arc window decoration, Inter type with Rubik display fallback, and 10px equal inner/outer gaps. Its desktop wallpaper has translucent orbital ribbons on the left and a quiet area on the right. Its restrained panel opacity is supported by existing renderer knobs; no unsupported blur or refraction setting is declared.

Paper Signal uses a 22px flat bar, no dock, 26px Minimal decoration with 2px corners, IBM Plex typography with installed-family fallbacks, 12px equal inner/outer gaps and low shadow strength. Its original wallpaper uses ledger lines, a cut circle, folded geometric planes and fine contour lines. It has a light default wallpaper and a graphite companion.

Both manifests omit wallpaper color matching and hardcoded window-manager border colors. They provide complete light/dark token defaults while named palettes remain free to replace all interface colors. Structure, typography and navigation differ even when the same palette is selected. Wallpaper colors are artwork, not an interface palette override.

Both packages reference shared builtins only: Orbit uses `clock-card`, `cpu-ram-ring`, `now-playing-mini`; Paper uses `clock-card`, `sysmon-graph`, `now-playing-mini`. No package-local widget definition is introduced. These are initial placement suggestions; the integrating global widget settings preserve user choices across theme changes. The shared widgets use theme tokens and can also be enabled with any other desktop theme.

## Artwork and validation

Each package contains `ARTWORK.md` with the complete original SVG source for both 3840×2160 wallpaper variants and PNG renders. SVG source lives in Markdown because the package validator intentionally accepts raster image assets rather than loose SVG files. Art contains no embedded UI, application windows, labels or widgets. All four PNG renders were visually inspected at 960×540. Real desktop screenshots should be captured after integration; none are simulated in wallpaper.

Executed from `/home/ambsd/Projects/decebal-themes-glass`:

```sh
python /tmp/create-orbit-paper.py
python /tmp/write-theme-manifests.py
DECEBAL_CONFIG=/tmp/decebal-theme-validation.json /home/ambsd/Projects/macshell/target/debug/decthemectl theme validate themes/orbit-glass
DECEBAL_CONFIG=/tmp/decebal-theme-validation.json /home/ambsd/Projects/macshell/target/debug/decthemectl theme validate themes/paper-signal
```

Both package validations passed. Temporary generation scripts are development helpers; reproducible wallpaper source is shipped in `ARTWORK.md`. A Python `tomllib`/sRGB contrast check checked text against background, surface, popup, titlebar and sidebar tokens. Minimum primary text contrast: Orbit dark 13.33, light 11.88; Paper dark 10.55, light 9.36. Minimum secondary text: 9.38/5.71 and 7.68/5.00 respectively. Accent text: 10.59/6.87 and 7.26/6.17. Faint text is reserved for nonessential details (minimum 3.94).

The current wallpaper loader chooses the first listed image; it does not automatically switch artwork with light/dark mode. Companion variants are provided for manual wallpaper selection. Font families fall back when unavailable. Source validation and static contrast checks do not replace live multi-monitor/scale/window-layout inspection. No theme was applied and no live configuration changed during package creation.
