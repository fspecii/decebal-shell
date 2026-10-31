# Plasma icon and cursor assets: recipes, private-prefix evidence and asset receipts

Contract read: `docs/desktop-asset-packs.md` on branch `feat/claude-desktop-asset-packs`, commit `76b9285b33c7664c4028e9e473532b3170ed58ac`
(committed tree only; the asset owner's working tree and engine were not read or modified). That commit contains only the document: the
installer (`src/desktop_assets/`, `decthemectl assets ...`) was **not committed**, so nothing here was run through it. The receipts come
from `tools/plasma-asset-receipt.py`, an emulation of the documented steps (stage, filter, verify tree OID and archive hash, licence copy,
content hash). They must be re-checked with `decthemectl assets verify` when the real installer lands.

Port owner file: `assets/desktop-ports/plasma/asset-packs.toml` (`family = "plasma"`). Receipts: `docs/plasma-theme-ports/asset-receipts.json`.
All build and install evidence lives under private prefixes in `~/.cache/decebal-plasma-ports/assets/`; nothing was installed into a user
or system icon folder, no theme was switched, no upstream script was run.

## Cursors: contract-compatible, receipts recorded

The Breeze repository commits **prebuilt Xcursor files** (`cursors/README.md`: "Commit changes in the theme directories to git"), so the
packs need no build step. Layan's `install.sh` only copies `dist-white/` to `Layan-white-cursors`.

| Pack id | Source commit | `path` | `tree` (git OID, verified) | install name | Archive sha256 (reproducible: downloaded twice) |
| --- | --- | --- | --- | --- | --- |
| `plasma-breeze-cursors` | breeze `75561b7f` | `cursors/Breeze/Breeze` | `ac0e5396057e0a7864d75fad624e81722b9c497d` | `breeze_cursors` | `377f44357a6e2bb0…78af6cc8f` (43,118,517 bytes) |
| `plasma-breeze-light-cursors` | breeze `75561b7f` | `cursors/Breeze_Light/Breeze_Light` | `60a2775ea3ef53b21ff0c59fe67023b87909fd91` | `Breeze_Light` | same archive |
| `plasma-layan-white-cursors` | Layan-cursors `b8c46895` | `dist-white` | `409983964b6ce7bec9227bed8a94929e604c8bcf` | `Layan-white-cursors` | `0d856f4d8b75131d…9a653d` (3,571,871 bytes) |

Verification performed (private, emulated): archive sha256 equals the catalogue; the tar's pax comment (`git get-tar-commit-id`) equals the
pinned commit for both archives; the pack folder extracted from the **archive** hashes, with git itself, to the same tree OID as
`git rev-parse <commit>:<path>` in the pinned clone; every symlink is relative and stays inside the pack; both cursor sets carry a core
pointer (`left_ptr` → `default`). Staged content hashes (documented formula, my filter): see the receipts; counts are 139 files + 68
symlinks for each Breeze set (47 scalable-cursor `metadata.json` files are skipped by the documented extension filter) and 48 files + 64
symlinks for Layan.

Licences as upstream ships them: Breeze cursors folder carries `COPYING` (GNU GPL version 2), `COPYING-ICONS` (the Breeze icon library's
LGPL-3.0-or-later notice) and `AUTHORS` (cursors by Ken Vermette); no SPDX header was found for the cursor files, so the catalogue records
`GPL-2.0-only` from the shipped text and names all three files: confirm with the Breeze maintainers before redistributing anything.
Layan-cursors: `LICENSE` is GPLv3 (GitHub reports GPL-3.0). `[[recommend]]`: `plasma-breeze` → `plasma-breeze-cursors` (Plasma's
look-and-feel `defaults` set `cursorTheme=breeze_cursors` for both Breeze Light and Dark), `plasma-layan` → `plasma-layan-white-cursors`
(Layan's defaults set `Layan-white-cursors`). The contract has one cursor recommendation per theme, so Breeze Light cursors are an
installable pack without a recommendation.

## Breeze icons: native build recipe into a private prefix (receipts), not a contract pack

`breeze-icons` needs a **build step** that the contract does not run: `index.theme` is generated from `breeze.theme.in` plus
`commonthemeinfo.theme.in`, and `breeze-dark` is `icons/` overlaid with colour-patched symbolic icons produced by upstream's own C++ tool
`tools/generate-symbolic-dark.cpp`. `tools/plasma-icon-recipe.sh` reproduces `icons/CMakeLists.txt` without ECM: it compiles that tool with
`g++` against Qt6Core (6.12.0, g++ 16.2.1), generates both indices and the dark overlay, copies the icon folders, adds the licence texts,
and writes only inside the private prefix:

```sh
tools/plasma-icon-recipe.sh ~/.cache/decebal-plasma-refs/breeze-icons ~/.cache/decebal-plasma-ports/assets/prefix-icons \
                            ~/.cache/decebal-plasma-ports/assets/work-icons      # about 25 s
```

Result at breeze-icons `e3c7154b4f243413b0cb9028484977f3c37de15c` (KF 6.32.0 development): `breeze` and `breeze-dark`, each 5,651 files and
8,442 relative symlinks (no absolute or broken link), content hashes `sha256:6ead90dc…d611c` and `sha256:7cc3bb03…55dd7`. Compared with the
installed Arch package `breeze-icons 6.31.0-1` in `/usr/share/icons`: **5,638 of 5,640 common files are byte-identical in each theme**,
including upstream's generated dark icons (for example `places/22/start-here-kde-symbolic.svg` with `#fcfcfc`, hash `b433b40c96a8…`);
`index.theme` is identical apart from the translated `Name[..]`/`Comment[..]` lines that the distribution adds; 11 files exist only in the
newer pinned source. This proves the recipe, not that the pinned source is the version the user has installed.

Install route under the contract (not executed): import the **built** folders, which the contract records as `local`:

```sh
decthemectl assets import ~/.cache/decebal-plasma-ports/assets/prefix-icons/share/icons/breeze --provides icons \
    --license LGPL-3.0-or-later --license-file LICENSES/COPYING-ICONS --name breeze      # then the same for breeze-dark
```

Most Plasma-capable systems already ship `breeze` and `breeze-dark` through their package manager; the Plasma packages only **depend on
the theme being installed** (`[icons] theme = "breeze"`, `[branding] logo = "icon:start-here-kde-symbolic"`). Where it is missing, the
panel draws the neutral `builtin:grid` logo (shared commit `b28cca5`).

## Tela icons (Layan): not installable by the contract

Tela's `install.sh` generates its colour and bright variants by script (`COLOR_VARIANTS`, `BRIGHT_VARIANTS`); the installer never runs
upstream scripts and installs only prebuilt trees, so there is no pack. A native recipe like the Breeze one would have to reproduce
`install.sh`; it was **not attempted** and no Tela receipt exists. Layan keeps `[icons] fallback = "breeze"`.

## What the asset owner / root still has to provide

1. The committed installer (`decthemectl assets install/verify/import`) and `catalog check`, to re-run this file through the real validator
   (the catalogue was written to the documented rules by hand and checked by `tests/plasma_theme_ports.rs`).
2. Registration of the `plasma` catalogue in `src/desktop_assets/builtin.rs` (root, when the port lands).
3. A decision on icon packs that need a generation step (Breeze, Tela): either accept `import` of locally built output, or add a recipe
   mechanism to the contract. No engine or contract file was changed here.
