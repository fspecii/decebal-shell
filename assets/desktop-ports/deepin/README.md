# Deepin asset sources (nothing licensed is stored here)

This folder is the Deepin family's declarative part of the shared asset-pack contract (`docs/desktop-asset-packs.md`): **pins, not copies**.
The icon and cursor themes are large and GPL / CC-BY licensed, so the repository carries only where they come from; the shell's installer
fetches or imports them, verifies every byte against a git tree id, keeps the upstream licence next to the files and never runs anything
from the upstream tree.

| file | what |
|---|---|
| [`asset-packs.toml`](asset-packs.toml) | the companion catalogue: source `deepin-icon-theme` @ `5d11ba4e` (GPL-3.0-or-later), packs `deepin-bloom` and `deepin-bloom-dark` (icons + Xcursor cursors, tree ids pinned), and `[[recommend]]` for the theme `deepin` |
| [`wallpapers/wallhaven-deepin.json`](wallpapers/wallhaven-deepin.json) | metadata-only Wallhaven fragment in the schema of `data/wallhaven-curated.json` (images, `themes.deepin`, `rejected_recommendations`) for the root to merge: two real photographs (dark graphite `ex5m3w`, light pale blue `r2k6km`) with full-resolution hashes, centre-crop reviews, unknown author and licence kept unknown; the earlier CGI picks are kept under `historical_candidates`. No image is stored or fetched by the package; nothing central is edited here |
| [`nirvana/MANIFEST.json`](nirvana/MANIFEST.json) | the input manifest of the maintainer conversion of Deepin's default icon theme: Git blob and sha256 of each of 1,435 converted DCI files, the layer used, output sha256, the revision, the verification status (`git-objects`) and the four refusals |
| [`tools/build_nirvana.py`](tools/build_nirvana.py), [`tools/test_build_nirvana.py`](tools/test_build_nirvana.py) | the standalone DCI reader/converter and its self-test (never run automatically): [DCI-CONVERSION](../../../docs/desktop-ports/deepin/DCI-CONVERSION.md) |
| [`tools/capture_panel.sh`](tools/capture_panel.sh), [`tools/capture_matrix.sh`](tools/capture_matrix.sh), [`tools/capture_interaction.sh`](tools/capture_interaction.sh) | private native evidence on an owned high X display (:700 to :999); see the evidence section of the port README |

## Using the packs

```sh
DECEBAL_ASSET_CATALOGS=assets/desktop-ports/deepin decthemectl assets list
decthemectl assets install deepin-bloom            # downloads the pinned GitHub archive, or:
decthemectl assets install deepin-bloom --source-dir ~/path/to/deepin-icon-theme    # a local clone at the pinned commit
decthemectl assets use icons deepin-bloom           # optional: applying the theme never selects an icon or cursor theme
```

## Licence notes

* Bloom (`bloom/`, `bloom-dark/`, `cursors-src/`): UnionTech Software Technology Co., Ltd., GPL-3.0-or-later (`.reuse/dep5` of the pinned
  revision). Individual third-party application logos inside `apps/` remain their projects' property; upstream does not enumerate them.
  The installer keeps `LICENSE`, `LICENSES/GPL-3.0-or-later.txt` and `.reuse/dep5` with the installed theme.
* nirvana (`deepin-desktop-theme`): CC-BY-4.0, UnionTech; a converted copy carries the licence text and a notice that the images were resized.
* Nothing here is linked into, compiled with or loaded as code by the MIT modules; the theme manifest names no icon or cursor theme.
