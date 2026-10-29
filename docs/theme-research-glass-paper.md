# Orbit Glass and Paper Signal

Research checked 2026-10-29. Reddit vote counts below are indexed snapshots, not fixed ratings. These are original desktop designs inspired by the compositional choices in popular posts, with original wallpaper artwork. No Reddit image, logo, dotfile, or application screenshot is included in either package.

## References inspected

- [[i3-gaps] Glassmorphism here I come](https://www.reddit.com/r/unixporn/comments/tc3kd0/) — 2022-03-11, indexed +2,317 votes. The actual screenshot uses large translucent rounded terminal surfaces against bold geometric wallpaper and leaves extensive negative space. Orbit adopts that separation of geometric backdrop and quiet foreground, using its own orbital construction.
- [[KDE] A Very Glassy Rice](https://www.reddit.com/r/unixporn/comments/1nvj0ks/) — 2025-10-01, indexed +853 votes. The actual screenshot uses a narrow floating top capsule and anchored lower navigation. Orbit adopts floating navigation and restrained edge definition. Its panels remain substantially opaque for legibility; the renderer does not provide the reference's refraction/godray effects.
- [[DWM] Minimalism](https://www.reddit.com/r/unixporn/comments/1nwcjxc/) — 2025-10-02, indexed +449 votes. The actual screenshot uses a warm paper field, flat rectangular windows and ample breathing room. Paper Signal adapts that editorial restraint with a functional thin panel and original geometric paper artwork.

## Distinct desktop structures

Orbit Glass uses a 28px floating bar, compact 46px magnifying Mac dock, rounded Arc window decoration, Inter type with Rubik display fallback, and 10px equal inner/outer gaps. Its desktop wallpaper has translucent orbital ribbons on the left and a quiet area on the right. Its restrained panel opacity is supported by existing renderer knobs; no unsupported blur or refraction setting is declared.

Paper Signal uses a 22px flat bar, no dock, 26px Minimal decoration with 2px corners, Adwaita Sans typography with monospace window titles with installed-family fallbacks, 12px equal inner/outer gaps and low shadow strength. Its original wallpaper uses ledger lines, a cut circle, folded geometric planes and fine contour lines. It has a light default wallpaper and a graphite companion.

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


## Orbit 2.0 production revision

The original Orbit package relied too heavily on color/wallpaper variation and reused a conventional top bar. Version 2.0 gives the desktop a distinct working structure: a right-side vertical navigation rail, a compact 42-pixel nonmagnifying centered launcher dock, 28-pixel Minimal window decoration with controls on the right, Adwaita Sans typography with monospace window titles, and 16/20-pixel inner/outer gaps. These differences remain when the same named palette is applied to Orbit and NextKDE. Orbit no longer includes theme widget placements; the global widget library owns those choices.

The wallpaper was redrawn from original vector geometry as an eclipse field: a restrained shaded sphere, atmospheric rim and trajectory lines, leaving the right work area quiet. It contains no application UI. Both dark/light complete vector sources are embedded in the package's ARTWORK.md, with MIT PNG renders. No upstream artwork, Reddit screenshot or shader implementation was copied.

### Primary glass-material research

NextKde/NextKde commit `3fa1d7006869bbd3404a05d362c6e6028e0fe3da` was inspected directly, including `shared/qml/controls/shaders/liquid_convex.frag`, `liquid_compact_surface.frag`, `NOTICE.md`, `AppearanceTokens.qml` and `DeskWidgetCard.qml`. The shader notice explicitly retains NextKDE ownership/GPL-3.0. Its convex model cites an MIT upstream demonstration but that does not relicense NextKDE's implementation. Research informs independently implemented material concepts only.

The reference's glass is more than transparency:

- The surface samples the actual backdrop in screen-aligned coordinates, so texture does not slide with a moved/scaled panel.
- A curved edge lens displaces the backdrop along the rounded-rectangle normal, while the central field stays undistorted.
- Background-only blur precedes tint/readability treatment. Foreground icons and text stay crisp.
- Thin directional highlights describe the material's thickness. Broad opaque outlines and flat RGB borders do not substitute for that edge response.
- A luminance-aware scrim preserves readable ink over bright/dark scenery; extremely subtle grain prevents sterile flat fills without becoming visible noise.
- Antialiased geometric clipping preserves the actual surface contour and excludes transparent backing-canvas margins.

Root integration owns the shared material renderer. The requested API separates tint/strength, blur, refraction strength, edge width/highlight, readability and grain rather than treating one alpha knob as a complete material. It must apply consistently to the right rail, launcher, popovers and global widget cards, independent of theme IDs. The theme package cannot provide true backdrop sampling/refraction by itself.

### Actual private desktop checks

Orbit2.0 package validation passes. An actual private 1920×1080 Xvfb + rdwm + Picom XRender/Gaussian scene was captured and inspected with the right rail, live shared analog/resource cards, original eclipse field and Settings window. The final structural proof is `/tmp/nextkde-matrix-orbit-production-dark-final.fQREDq/root.png`. It uses actual `theme apply` after selecting mode, preserving the manifest's Minimal decoration; running legacy color/mode apply afterward would override package decoration extras and misrepresent the theme.

This screenshot demonstrates the new structure and current translucent material, not completed liquid-refraction support. Fresh final screenshots must follow the root shared-material integration. The previously documented static contrast checks do not establish real glass legibility over arbitrary wallpaper. The package now declares explicit dark/light wallpaper keys. Full apply chooses the matching variant; mode changes follow package-owned art while retaining user custom wallpaper.


Orbit requests a distinct native material profile: 7-pixel refraction, 10-pixel bezel, 0.25 highlight, 0.003 grain and 5-pixel blur. These use the root shared renderer, not an Orbit-only paint path. Material geometry is generated from the same actual rail-pill/dock layout used to draw the foreground, with no glass in gaps between rail groups or unused popup canvas. The original eclipse artwork is not evidence of the renderer; an actual textured-backdrop test and final runtime screenshots are still required after integration.


The final native Orbit2.0 captures were generated and visually inspected on private 1920×1080 rdwm/Picom desktops: dark `/tmp/nextkde-matrix-orbit-native-dark.xNDiXn/root.png`, light `/tmp/nextkde-matrix-orbit-native-light.1RmXV1/root.png`. Light mode selected its matching wallpaper automatically through the new package transaction, without a manual wallpaper command. The actual 42-pixel launcher and corresponding compact plate/reserve now follow the theme rather than static 47/71-pixel dock constants. The package's main screenshot uses the actual dark native capture. Final package validation passes with SHA-256 `a5804d35c9c6a2b4c128c9fb7a3f67a220deb75d57f48d98008a5c051e01b587`.
