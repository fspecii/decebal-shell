# Handoff: exact port-only file set and current catalogue delta

Generated at branch commit `e498d5b` against root `c55c4cd` (root's checked-out tree when this was written); the file list is `git diff --name-only 905c059 HEAD` on this branch, where `905c059` is the
last root commit merged here, so it is the port's own work only. **No file outside the five port-owned prefixes is touched** (`src/`, `Cargo.toml`, `Makefile`, `data/`, `docs/*.md` of root: none).

## Port-only files (+ this file; 207: 75 listed, 132 native PNG captures under `docs/desktop-ports/openkylin/native/`)

Prefixes: `themes/openkylin*/`, `assets/desktop-ports/openkylin/`, `docs/desktop-ports/openkylin/`, `tests/openkylin_port.py`, `tests/openkylin_desktop_port.rs`, `tests/e2e/openkylin*.py`.
`tests/openkylin_port.py::HarnessDisplays` **reads and executes** `tests/e2e/openkylin-native.py`, which loads `tests/e2e/openkylin_fixture.py`: hand these over together
(`openkylin-decoration-proof.py` and `openkylin-preserve.py` are the other two e2e scripts; they need binaries and are not run by the offline suite).

```
assets/desktop-ports/openkylin/asset-packs.toml
assets/desktop-ports/openkylin/derived/summary.json
assets/desktop-ports/openkylin/derived/tokens-Classic-dark.json
assets/desktop-ports/openkylin/derived/tokens-Classic-light.json
assets/desktop-ports/openkylin/derived/tokens-HeYin-dark.json
assets/desktop-ports/openkylin/derived/tokens-HeYin-light.json
assets/desktop-ports/openkylin/derived/tokens-Light-Seeking-dark.json
assets/desktop-ports/openkylin/derived/tokens-Light-Seeking-light.json
assets/desktop-ports/openkylin/derived/tokens-v11-dark.json
assets/desktop-ports/openkylin/derived/tokens-v11-light.json
assets/desktop-ports/openkylin/sources.json
assets/desktop-ports/openkylin/tools/build_packages.py
assets/desktop-ports/openkylin/tools/cursor_sheet.py
assets/desktop-ports/openkylin/tools/cursor_table.py
assets/desktop-ports/openkylin/tools/derive_palette.py
assets/desktop-ports/openkylin/tools/make_asset_catalog.py
assets/desktop-ports/openkylin/tools/make_wallpapers.py
assets/desktop-ports/openkylin/tools/merge_catalogue.py
assets/desktop-ports/openkylin/tools/native_summary.py
assets/desktop-ports/openkylin/tools/package_docs.py
assets/desktop-ports/openkylin/tools/pin_sources.py
assets/desktop-ports/openkylin/tools/resolve_tokens.py
assets/desktop-ports/openkylin/tools/wallpaper_picks.json
assets/desktop-ports/openkylin/tools/wh_fetch.py
assets/desktop-ports/openkylin/tools/wh_search.py
assets/desktop-ports/openkylin/tools/xcursor_read.py
assets/desktop-ports/openkylin/wallpapers.json
assets/desktop-ports/openkylin/widgets/ukui-time-12.toml
assets/desktop-ports/openkylin/widgets/ukui-time-24.toml
docs/desktop-ports/openkylin/README.md
docs/desktop-ports/openkylin/icons-cursors.md
docs/desktop-ports/openkylin/integration-contract.md
docs/desktop-ports/openkylin/native/README.md
docs/desktop-ports/openkylin/native/decoration-proof.json
docs/desktop-ports/openkylin/native/native-checks.json
docs/desktop-ports/openkylin/native/native-controlcenter.json
docs/desktop-ports/openkylin/native/native-widget.json
docs/desktop-ports/openkylin/palette.md
docs/desktop-ports/openkylin/registration-notes.md
tests/e2e/openkylin-decoration-proof.py
tests/e2e/openkylin-native.py
tests/e2e/openkylin-preserve.py
tests/e2e/openkylin_fixture.py
tests/openkylin_desktop_port.rs
tests/openkylin_port.py
themes/openkylin-classic/ATTRIBUTION.md
themes/openkylin-classic/LICENSE.md
themes/openkylin-classic/README.md
themes/openkylin-classic/theme.toml
themes/openkylin-classic/widgets/ukui-time-12.toml
themes/openkylin-classic/widgets/ukui-time-24.toml
themes/openkylin-heyin/ATTRIBUTION.md
themes/openkylin-heyin/LICENSE.md
themes/openkylin-heyin/README.md
themes/openkylin-heyin/theme.toml
themes/openkylin-heyin/widgets/ukui-time-12.toml
themes/openkylin-heyin/widgets/ukui-time-24.toml
themes/openkylin-islands/ATTRIBUTION.md
themes/openkylin-islands/LICENSE.md
themes/openkylin-islands/README.md
themes/openkylin-islands/theme.toml
themes/openkylin-islands/widgets/ukui-time-12.toml
themes/openkylin-islands/widgets/ukui-time-24.toml
themes/openkylin-light-seeking/ATTRIBUTION.md
themes/openkylin-light-seeking/LICENSE.md
themes/openkylin-light-seeking/README.md
themes/openkylin-light-seeking/theme.toml
themes/openkylin-light-seeking/widgets/ukui-time-12.toml
themes/openkylin-light-seeking/widgets/ukui-time-24.toml
themes/openkylin/ATTRIBUTION.md
themes/openkylin/LICENSE.md
themes/openkylin/README.md
themes/openkylin/theme.toml
themes/openkylin/widgets/ukui-time-12.toml
themes/openkylin/widgets/ukui-time-24.toml
```

## Catalogue delta (`data/wallhaven-curated.json`, root has 24 themes / 46 images)

Run by root: `python3 -I assets/desktop-ports/openkylin/tools/merge_catalogue.py data/wallhaven-curated.json OUT` (refuses a clash). Delta: **+5 `themes` records, +10 `images` records,
+1 `scope.openkylin_integration` string**; result 29 themes / 56 images. No existing record changes. Checked against root's current file: no theme or image id
collides. After integration `tests/openkylin_port.py` requires the catalogue's records for these ids to be deep-equal to `assets/desktop-ports/openkylin/wallpapers.json`.

| theme | light image | dark image |
|---|---|---|
| `openkylin` | `m3lme1` | `4vwjr3` |
| `openkylin-islands` | `wq2dwr` | `o5p21m` |
| `openkylin-heyin` | `28v3mm` | `kxy19m` |
| `openkylin-classic` | `3ld9wv` | `pokqd3` |
| `openkylin-light-seeking` | `7p92ov` | `1k9vjw` |

| image | resolution | full-res sha256 | bytes |
|---|---|---|---|
| `m3lme1` | 3840×2160 | `b33471fb6fee4fc3…` | 3673 KiB |
| `4vwjr3` | 2560×1440 | `408cdc8d5eea1cec…` | 376 KiB |
| `wq2dwr` | 7680×4320 | `e2c8c13cc54f2a3c…` | 8339 KiB |
| `o5p21m` | 5478×3081 | `7156c0d9f18ad846…` | 9920 KiB |
| `28v3mm` | 5120×2880 | `fcabd1c689d3d1bd…` | 6456 KiB |
| `kxy19m` | 3840×2160 | `abba7c84234957bb…` | 4375 KiB |
| `3ld9wv` | 3840×2160 | `292cc37e67801d2f…` | 13373 KiB |
| `pokqd3` | 6144×3456 | `d9092c143181b610…` | 15001 KiB |
| `7p92ov` | 3840×2160 | `2ec777158e9f10c2…` | 3147 KiB |
| `1k9vjw` | 7680×4320 | `15fdc28270a70ec9…` | 14293 KiB |

Authorship and licence of all ten are unknown (null); nothing is bundled.

## Asset catalogue

`assets/desktop-ports/openkylin/asset-packs.toml`: 2 sources (licence blobs pinned with `license_oids`), 6 packs, 5 recommendations; to be listed where root lists asset-pack families. The icon packs'
`from moka` provenance is recorded as unresolved.
