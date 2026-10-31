# Deepin window decoration: measured spec, the rdwm styles, and how the shell picks one

Nothing here edits rdwm or the shared engine. Source of every number: deepin-kwin `664808264f5a49c306834565d0e0c631a8feda27`,
`src/plugins/kdecorations/deepin-chameleon/deepin/{light,dark}/decoration.json` and the 78 SVG glyph files beside them (CC0-1.0 per that
repository's `REUSE.toml`; they are not copied into this repository any more: rdwm draws its own, and the glyph sheet
[evidence/chameleon-glyphs-light-dark.png](evidence/chameleon-glyphs-light-dark.png) is a rendering of the upstream SVGs kept as
measurement evidence). The plugin's C++ (GPL-3.0-or-later) was read for behaviour only.

## Measured numbers (`decoration.json`, window type 1001 "ordinary window")

| item | light | dark |
|---|---|---|
| titlebar height | 50 px | 50 px |
| titlebar background | `white` | `#101010` |
| title | Source Han Sans SC 14 px, centred | same |
| title colour, active / inactive | `#303030` / `#969696` | `#8c8c8c` / `#656565` |
| window border | 1 px `#e0e0e0` | 1 px `#4d000000` (black at 30 %) |
| buttons | minimise, maximise / unmaximise, close; each 50 x 50 px, flat cell, no background in the normal state | same |
| button states | `normal`, `hover`, `press`, `disabled` | same |
| glyph | 1 px strokes, centred: minimise 14 x 1, maximise 12 x 10, unmaximise 13 x 11, close 11 x 11; `#000` at 0.7 (normal, hover), `#0081ff` pressed, 0.4 disabled | `#fff` at 0.7, same pressed colour |
| hover / press plate | 50 x 50, black at 0.05 (hover), `#0081ff` at 0.1 (press) | white at 0.05, same press |
| shadow (ordinary window) | radius 50, offset (0, 22), `#80000000` active / `#40000000` inactive | same |
| dialog (1002) | shadow 30 / (0, 12) / `#50000000`; menu button at (10, 10) | same |
| right-click menu (1004) / drop-down (1005) / tool box (1006) | corner radius 18 / 0 / 8, blur 30 | same |

**Corner radius.** The JSON says `"0,0"` for an ordinary window: the rounding comes from the toolkit setting
`dtk-window-radius`. dde-services `69fcd01` (`src/plugin-qt/xsettings/misc/org.deepin.XSettings.json`) ships **8**; DTK's QML fallback when
the setting is unset is 12 (`FlowStyle.qml`, `menu.radius`), and the global theme nirvana carries `WindowRadius=12`. The first draft of this
port used 12 from the fallback; the manifest now uses 8, the value a stock system really has. (rdwm's measured styles use 8 too.)

## What exists in rdwm

rdwm commit `952457b` (branch `feat/claude-asian-desktop-decorations`, in root's review when this was written) adds the `deepin` and
`deepin-dark` styles, an independent native implementation of the table above (no upstream code or SVG path data), with per-focus
shadows and a 220 px minimum width; `rdwmctl schema` lists them with `family = "deepin"` and `variant = "light" | "dark"`. Its numbers agree
with the table here (50 px bar, 50 x 50 cells, text colours, borders, `#0081ff` held glyph, the 50/22 shadow). I cross-checked the title
and button numbers, not its pixels: no rdwm built from that commit was run for this port.

## Selection: the mode-specific window-style contract

The manifest uses the shared contract (Lingmo branch `ca418f2`, `docs/desktop-ports/lingmo/decoration-style-modes.md`), with no Deepin id in engine code:

```toml
[decoration]
style = "adwaita"                  # older key, for engines without the mode keys
style_light = "deepin"
style_dark = "deepin-dark"
fallback_style_light = "adwaita-light"
fallback_style_dark = "adwaita"
```

* The engine proves the wanted style against the **running** rdwm's own `schema` reply, never the installed CLI's offline table. No rdwm running: the
  explicit target `rdwmctl schema --offline` answers, labelled as offline proof. An unknown or transient answer stops the apply before anything is written.
  The fallback is used only if the same source lists it, and Settings then shows a persistent warning.
* No `height` or `radius`: a theme value would override the native geometry (deepin 50 px, radius 8) of whichever style ends up in use. `title_align`, `buttons`
  and `button_order` are left out for the same reason.
* Needs `min_shell_version = "0.3.0"` (set). Until the engine with the contract is merged its validator rejects the new keys as unknown;
  `tests/deepin_port.rs` gates the engine-dependent assertions on `schema::field("decoration.style_light")` and mirrors the rules for every engine,
  `tests/deepin_decoration_modes.rs` calls the real `decoration_compat::wanted` for both modes and compiles only with the engine.
* The fallbacks are not Chameleon (their glyph strokes, hover plates and shadows differ); they exist so an older rdwm still gets a sensible
  right-hand-controls title bar, with the warning saying why.

## Not carried over

The menu button (32 x 32 at the left end of a window) and the snap/split flyout glyphs have no Decebal equivalent. Deepin applications that
draw their own DTK title bar (`titleBar.height` 50 in `FlowStyle.qml`) are unaffected by any of this.
