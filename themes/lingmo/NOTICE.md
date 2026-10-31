# Notice

`theme.toml` is original Decebal content (MIT, the repository licence). It contains only numbers and colour values measured
from the LingmoOS repositories listed in `docs/desktop-ports/lingmo/sources.json` at the commits recorded there:

| Repository | Commit | Licence evidence |
| --- | --- | --- |
| `LingmoOS/lingmo-compositor` | `1cdaf0a86596ef3732e23d2450318728a180877b` | `LICENSE` is BSD 3-Clause (Copyright 2026 Reion Wong); its README says GPL-3.0-or-later; ported Chromium and sommelier code carries `LICENSE.chromium` / `LICENSE.sommelier` |
| `LingmoOS/lingmo-sdk` | `752d2b24b398b11b995cbb82bfba0b8c161df7bd` | `LICENSE` is BSD 3-Clause; source headers say GPL-3.0-or-later |

The two upstream repositories contradict themselves, so this package takes **no source code, shader, SVG path data or
artwork** from them: a palette value or a pixel metric is a fact about a design, and each is re-authored here as TOML.
"Lingmo" and "LingmoOS" are the upstream project's names; this theme does not imply endorsement.

No icon, cursor, font, logo or wallpaper is bundled. The Lingmo icon theme (GPL-3.0-or-later, a Colloid derivative) and cursor
themes (licence not granted in their repository) are described in `assets/desktop-ports/lingmo/asset-packs.toml` (icons, for the shared asset installer) and
`docs/desktop-ports/lingmo/reference-recipes.toml` (cursors, a user-initiated import), and installed only at the user's request. No wallpaper ships with the package.
