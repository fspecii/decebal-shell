# Sweeteh for Decebal

Port of [Aarav2709/Sweeteh](https://github.com/Aarav2709/Sweeteh), pinned at
`b7c0bdaa6aca54c7b45e341341b4b34408bb9c5a`, inspected 2026-10-29. Source inspected:
README/LICENSE, config/Defaults.qml, theme/{Theme,ColorSet,Colors}.qml,
theme/fallback.json, components/Depth.qml, modules/bar and modules/desktop,
services/AppTheme.qml, hypr/hyprland.lua, dots/kitty, fish and starship. Frames at
10, 45 and 100 seconds of the actual upstream Sweeteh.mp4 were viewed. Neither
installer nor any upstream executable was run.

## What is ported

The explicit blue **fallback palette** in upstream fallback.json supplies both
modes' Material roles. This is a real upstream configuration, not a guess from a
single recolored demo frame. The demo also shows dynamic red/green schemes.
Rounded 26 px desktop tiles sit on a real 4 px accent ledge (native safe widget
geometry), with fine light rims. Shell popup radius is 24 px, the engine maximum.
Google Sans Flex is requested for UI/title/display; installed Inter/Noto are
fallbacks. The unmodified upstream OFL font is bundled for optional manual setup.
Upstream's 5/20 gaps, 2 px border and 10 px window rounding map into rdwm. Adwaita
captions keep close/minimise/maximise available; native Hyprland clients in the
reference often have no titlebar, which is intentionally not copied.

Eight optional safe widgets: analog clock and weather at upper right, wide
calendar below, MPRIS media and system load below that, plus workspace, center
clock/launcher and status islands along the top. Positions follow the source's
right-hand tile grouping and island silhouette. Calendar uses actual local date;
it does not invent a week grid. Weather uses the existing shared provider;
missing weather says No data. System values/history and media metadata/progress
are real shared sources, with explicit unavailable states. Transport controls
only appear for a real MPRIS player. No lyric, place or service name is invented.

The top pieces are **widget islands**, not a fully ported dynamic island/bar.
Workspaces use shared tags/actions; center opens launcher and right opens the
dashboard. They have no system tray, exclusive screen reservation, expanding
notch, OSD stack, polkit prompt, Cava/lyrics or animated springs. Apps may cover
them. The native shared launcher/control center/notifications remain available;
those interfaces do not acquire Sweeteh's QML layouts. No dock is enabled.
The seed targets 1920 × 1080; smaller/scaled screens require layout adjustment.

## Wallpaper, font, icons and cursor

See ATTRIBUTION.md for exact hashes, primary URLs and license separation. Both
unaltered photographs actually ship in the pinned upstream assets directory.
Dark uses Julia Ly's rain texture; light uses Jack B's night-sky landscape. These
are two existing wallpaper alternatives, not a claim that the latter is daylight.
The red flower image and animated abyss seen in the demo are **not copied**:
the former is not a checked-in asset and the latter lacks per-asset provenance.
Theme-owned artwork follows mode changes; a user's custom wallpaper is retained.

Upstream recolors installed Adwaita folders into a generated Sweeteh icon theme.
This package selects installed **Adwaita**, with hicolor fallback and symbolic
tinting; it does not claim custom folder recoloring or bundle external icons.
The package engine cannot safely generate/register icon themes from SVGs yet.
Upstream only requests cursor size 24 and names no cursor family; keep the user's
cursor. There is no theme-package cursor selector, so none is invented.

The bundled `fonts/GoogleSansFlex.ttf` is retained with its OFL license for
upstream attribution. Its variable outlines are currently unsupported by the
native text renderer; do not register it for this shell. Decebal falls back to
an installed readable font when an outline cannot be drawn. Theme application
does not install fonts. Material Symbols are not bundled; widgets reuse native
symbolic glyphs with matching semantic colors.

## Terminal and application styling

Existing Decebal application-theme adapters provide matching GTK 3 / GTK 4 /
libadwaita semantic CSS, Qt configuration where supported, Xresources and native
st/kitty/foot/alacritty/ghostty colors. Inspect the normal All apps/system theme
plan before opting in. Existing GTK windows can need reopening. Qt runtime
fidelity is not verified here. Automatic application theming was disabled for
all port tests; no user files or dconf values were modified.

The existing terminal adapter writes a managed block into kitty.conf, preserving
unrelated options. Optional manual upstream-like settings are `font_family
JetBrains Mono Nerd Font`, `font_size 11.0`, `background_opacity 0.85`,
`cursor_shape beam`, `cursor_trail 1` and `window_margin_width 21.75`, if supported
by the installed kitty version. They are documented, not applied. Fish/starship
are optional independent programs: upstream's exact prompt uses end-4-derived
ANSI slots 232–255 and extra generated fish colors; Decebal's standard 16-color
adapter does not implement those slots, prompt files or upstream ColorMath
harmonization. Do not copy the prompt and claim parity. No Vesktop CSS import,
Zed extension, upstream shell hooks or automatic updates are installed.

The default package pins the real blue fallback for a reproducible baseline.
Users can subsequently enable Decebal's existing Match wallpaper or choose any
palette; its extraction differs from Sweeteh/Matugen's wallpaper-HSL wash.

## Evidence, safety and limits

Native headless sample/empty renders and four labelled compositions are in
`docs/theme-ports/sweeteh/`. These are not desktop screenshots and fixed sample
values do not describe this machine. Actual definitions and manifest anchors
are rendered; no screenshot pixels or simulated UI chrome are painted into art.
All widgets are optional; customized layouts retain Keep / Replace / Cancel.
The known buttons-side hint warning is expected; Adwaita determines the right
side and the engine explicitly enables buttons. Production PAM is unchanged.

Reproduce from the repository with private config and
`DECEBAL_SYSTEM_AUTOMATIC=0`: run `cargo test --test theme_ports` with
`DECEBAL_THEME_EVIDENCE="$PWD/docs/theme-ports"`, then
`python tests/theme-ports-compose.py sweeteh`. CLI validation and reproducible
pack tests do not install/apply anything to the desktop. Follow-up shared engine
work: real reserved island bar, generated folder recoloring, registered package
fonts, true week grid and wallpaper-hue/terminal harmonization.
