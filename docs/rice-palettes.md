# Researched rice palettes — 20 additions

These palettes are colour sources for every Decebal desktop design. They do not install a desktop theme, wallpaper, icon pack or plugin. The existing 13 IDs remain unchanged; these 20 IDs bring the built-in catalogue to 33.

## Research and attribution

The batch covers established editor/terminal rice families rather than a claimed popularity ranking. Nineteen use the authored [Tinted Theming Base16/Base24 scheme data](https://github.com/tinted-theming/schemes/tree/dd2a1379533372ad79560bb4392570782a8a0a9d), pinned at `dd2a1379533372ad79560bb4392570782a8a0a9d`. These are Base16/Base24 adaptations, not claims of first-party full desktop-theme fidelity. GitHub data at that pin is generated upstream from Primer primitives v11.10.0. Nightfox/Dayfox use their [original palette modules](https://github.com/EdenEast/nightfox.nvim/tree/4dacd3f0185a2227bdf3b6c0975a8f0bf87cac9a/lua/nightfox/palette) at `4dacd3f0185a2227bdf3b6c0975a8f0bf87cac9a`.

The sources were inspected locally and their primary repositories browsed. Each variant has its exact source URL, SHA-256, author, license, source colours and every Decebal-role-to-source-key mapping in [palette-sources.json](palette-sources.json). Source licence notices are reproduced in [palette-sources-LICENSES.txt](palette-sources-LICENSES.txt). No third-party artwork, code, installation script or screenshot is copied.

Fifteen families have source dark/light pairs. Spacemacs, Monokai, OceanicNext, Zenburn and Vesper have no paired light file in the inspected source; their light modes are explicitly labelled **Decebal adaptations**, reusing the dark source’s neutral colours in light roles, with the status readability adaptations described below. They must not be presented as official upstream light schemes.

## Catalogue

| ID | Source family / author | Dark source | Light source |
| --- | --- | --- | --- |
| `ayu` | Ayu — Tinted Theming (https://github.com/tinted-theming), Ayu Theme (https://github.com/ayu-theme) | [base16/ayu-dark.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/ayu-dark.yaml) | [base16/ayu-light.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/ayu-light.yaml) |
| `one-dark` | One Dark / One Light — Lalit Magant (http://github.com/tilal6991) | [base16/onedark.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/onedark.yaml) | [base16/one-light.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/one-light.yaml) |
| `horizon` | Horizon — Michaël Ball (http://github.com/michael-ball/) | [base16/horizon-dark.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/horizon-dark.yaml) | [base16/horizon-light.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/horizon-light.yaml) |
| `material` | Material — Nate Peterson | [base16/material.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/material.yaml) | [base16/material-lighter.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/material-lighter.yaml) |
| `papercolor` | PaperColor — Jon Leopard (http://github.com/jonleopard), Tinted Theming (https://github.com/tinted-theming), based on PaperColor Theme (https://github.com/NLKNguyen/papercolor-theme) | [base16/papercolor-dark.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/papercolor-dark.yaml) | [base16/papercolor-light.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/papercolor-light.yaml) |
| `edge` | Edge — cjayross (https://github.com/cjayross), Tinted Theming (https://github.com/tinted-theming) | [base16/edge-dark.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/edge-dark.yaml) | [base16/edge-light.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/edge-light.yaml) |
| `oxocarbon` | Oxocarbon — shaunsingh/IBM, Tinted Theming (https://github.com/tinted-theming) | [base16/oxocarbon-dark.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/oxocarbon-dark.yaml) | [base16/oxocarbon-light.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/oxocarbon-light.yaml) |
| `selenized` | Selenized — Jan Warchol (https://github.com/jan-warchol/selenized) / adapted to base16 by ali | [base16/selenized-dark.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/selenized-dark.yaml) | [base16/selenized-light.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/selenized-light.yaml) |
| `bluloco` | Bluloco — FredHappyface (https://github.com/fredHappyface) | [base24/bluloco-dark.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base24/bluloco-dark.yaml) | [base24/bluloco-light.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base24/bluloco-light.yaml) |
| `github` | GitHub — Tinted Theming (https://github.com/tinted-theming) | [base16/github-dark.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/github-dark.yaml) | [base16/github.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/github.yaml) |
| `tomorrow` | Tomorrow — Chris Kempson (http://chriskempson.com) | [base16/tomorrow-night.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/tomorrow-night.yaml) | [base16/tomorrow.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/tomorrow.yaml) |
| `flexoki` | Flexoki — Steph Ango (https://github.com/kepano/flexoki) | [base16/flexoki-dark.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/flexoki-dark.yaml) | [base16/flexoki-light.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/flexoki-light.yaml) |
| `atelier-forest` | Atelier Forest — Bram de Haan (http://atelierbramdehaan.nl) | [base16/atelier-forest.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/atelier-forest.yaml) | [base16/atelier-forest-light.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/atelier-forest-light.yaml) |
| `synth-midnight` | Synth Midnight — Michaël Ball (http://github.com/michael-ball/) | [base16/synth-midnight-dark.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/synth-midnight-dark.yaml) | [base16/synth-midnight-light.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/synth-midnight-light.yaml) |
| `spacemacs` | Spacemacs — Nasser Alshammari (https://github.com/nashamri/spacemacs-theme) | [base16/spacemacs.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/spacemacs.yaml) | Decebal source-colour adaptation |
| `monokai` | Monokai — Wimer Hazenberg (http://www.monokai.nl) | [base16/monokai.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/monokai.yaml) | Decebal source-colour adaptation |
| `oceanic-next` | OceanicNext — https://github.com/voronianski/oceanic-next-color-scheme | [base16/oceanicnext.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/oceanicnext.yaml) | Decebal source-colour adaptation |
| `zenburn` | Zenburn — elnawe | [base16/zenburn.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/zenburn.yaml) | Decebal source-colour adaptation |
| `vesper` | Vesper — FormalSnake (https://github.com/formalsnake) | [base16/vesper.yaml](https://github.com/tinted-theming/schemes/blob/dd2a1379533372ad79560bb4392570782a8a0a9d/base16/vesper.yaml) | Decebal source-colour adaptation |
| `nightfox` | Nightfox / Dayfox — James Simpson / EdenEast | [lua/nightfox/palette/nightfox.lua](https://github.com/EdenEast/nightfox.nvim/blob/4dacd3f0185a2227bdf3b6c0975a8f0bf87cac9a/lua/nightfox/palette/nightfox.lua) | [lua/nightfox/palette/dayfox.lua](https://github.com/EdenEast/nightfox.nvim/blob/4dacd3f0185a2227bdf3b6c0975a8f0bf87cac9a/lua/nightfox/palette/dayfox.lua) |

## Desktop role mapping and legibility

Base16/Base24 are terminal/editor colour sets, so desktop surface roles are adaptations even when the hex value is copied exactly. Ordinary variants use source `base00` as background and usually `base01` as card/popup/titlebar surface. Bluloco Dark uses its Base24 `base11` surface instead of ANSI gray. Where no source accent remains readable on both surfaces, the surface reuses the background; no blended replacement colour is manufactured. Sidebar, bar and dock share the background. Border uses the source neutral `base02`. Source light variants retain their light source background; the five labelled light adaptations use the brightest source neutral as background.

Main text is the source neutral with the strongest contrast across the surfaces. Secondary and faint text reuse source neutrals with at least 4.5:1 and 3:1 contrast respectively. Accents prefer the named source role, then another literal source colour when required for at least 3:1 contrast. This can change the upstream editor’s nominal accent choice. For example, Ayu Light uses its purple rather than low-contrast amber; the exact effective mapping is recorded below and in JSON. Semantic states retain the source red/green/yellow role (Bluloco uses its Base24 bright yellow). When its contrast is below 3:1, sRGB channels are interpolated toward the source main foreground in 0.001 steps, rounded to 8-bit values, until contrast reaches at least 3.01:1. These 41 status-role adaptations preserve the source hue direction rather than substituting an unrelated readable accent. JSON records each original hex, final hex, target source key/hex and exact mix. Status text/icons remain necessary and colour alone must not carry meaning.

`accent_text` may use an explicit Decebal black/white readability fallback if no source neutral achieves 4.5:1 on the chosen accent. These entries are marked `decebal.contrast.black/white` and **not** attributed to upstream. There are nine such foreground fallbacks. Apart from these and the explicitly documented status mixes, all supplied role values are literal source colours.

| ID | Dark background / accent | Light background / accent |
| --- | --- | --- |
| `ayu` | `#0b0e14` / `#ffb454` (base0A) | `#f8f9fa` / `#a37acc` (base0E) |
| `one-dark` | `#282c34` / `#61afef` (base0D) | `#fafafa` / `#4078f2` (base0D) |
| `horizon` | `#1c1e26` / `#e93c58` (base08) | `#fdf0ed` / `#da103f` (base0D) |
| `material` | `#263238` / `#89ddff` (base0C) | `#fafafa` / `#6182b8` (base0D) |
| `papercolor` | `#1c1c1c` / `#5fafd7` (base0D) | `#eeeeee` / `#005f87` (base0D) |
| `edge` | `#262729` / `#73b3e7` (base0D) | `#fafafa` / `#6587bf` (base0D) |
| `oxocarbon` | `#161616` / `#33b1ff` (base0D) | `#f2f4f8` / `#0f62fe` (base0D) |
| `selenized` | `#103c48` / `#4695f7` (base0D) | `#fbf3db` / `#006dce` (base0D) |
| `bluloco` | `#1e2027` / `#189ffd` (base0A) | `#f7f7f7` / `#1d44dd` (base0D) |
| `github` | `#0d1117` / `#58a6ff` (base0D) | `#ffffff` / `#0969da` (base0D) |
| `tomorrow` | `#1d1f21` / `#81a2be` (base0D) | `#ffffff` / `#4271ae` (base0D) |
| `flexoki` | `#100f0f` / `#4385be` (base0D) | `#fffcf0` / `#205ea6` (base0D) |
| `atelier-forest` | `#1b1918` / `#7b9726` (base0B) | `#f1efee` / `#df5320` (base09) |
| `synth-midnight` | `#050608` / `#ea5ce2` (base0E) | `#dddfe0` / `#b53b50` (base08) |
| `spacemacs` | `#1f2022` / `#4f97d7` (base0D) | `#f8f8f8` / `#a31db1` (base0E) |
| `monokai` | `#272822` / `#f92672` (base08) | `#f9f8f5` / `#f92672` (base08) |
| `oceanic-next` | `#1b2b34` / `#5fb3b3` (base0C) | `#d8dee9` / `#65737e` (base03) |
| `zenburn` | `#383838` / `#e0cf9f` (base0A) | `#ffffff` / `#000000` (base0F) |
| `vesper` | `#101010` / `#ffc799` (base0A) | `#d5d5d5` / `#876c4f` (base0F) |
| `nightfox` | `#192330` / `#719cd6` (blue) | `#f6f2ee` / `#2848a9` (blue) |

The constraints concern the opaque palette tokens: main/bar text ≥4.5:1 on all supplied surfaces; secondary text ≥4.5:1 on the surface; faint text and accents/status colours ≥3:1; on-accent text ≥4.5:1. They do not establish WCAG compliance for every application, transparency level, wallpaper, disabled control or downstream custom renderer. Existing native palette tests additionally check every built-in palette against every theme in both modes and preserve theme geometry, fonts and decoration structure.

## Validation

Independent Python validation parsed all 20 TOMLs / 40 modes / 760 role values, verified source-file SHA-256 hashes and every literal or derived mapping, and checked contrast across all nine opaque surfaces. Minimum main contrast is 5.58:1; secondary 4.51:1; faint 3.02:1; accent 3.02:1; each semantic state 3.01:1; on-accent 4.50:1. All 20 dark role vectors are distinct. The installed `decthemectl palette list` also parsed all 33 catalogue files. Focused native palette tests passed (5/5); formatting and whitespace checks passed. No live theme/palette selection, configuration or process is changed by this batch.
