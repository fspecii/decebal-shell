# Deepin's default icon theme (nirvana): conversion recipe and a request to the asset-installer owner

nirvana, the icon theme Deepin 25's global theme selects (`dde-appearance` 7190e59 `Global_Theme = nirvana.light`; the XSettings default
`icon-theme-name` is `bloom`, dde-services 69fcd01), is stored as **DCI** containers in `deepin-desktop-theme` a0c79e3
(`nirvana/dsg-icons/nirvana/*.dci`, 1,439 files of which 1,435 convert, 25 MB, CC-BY-4.0 per that repository's `.reuse/dep5`). The freedesktop lookup does not read
DCI, and `asset-packs.toml` cannot express a conversion, so nirvana is **not** a `[[pack]]` today and **no converted file is committed**.

## Maintainer recipe (what exists now, reviewable, never executes upstream code)

```sh
git clone --filter=blob:none --no-checkout https://github.com/linuxdeepin/deepin-desktop-theme.git
git -C deepin-desktop-theme sparse-checkout set --no-cone /nirvana/ /LICENSES/ /.reuse/
git -C deepin-desktop-theme checkout a0c79e31467ab602d96605475dca69eb835c5d90

python3 -I assets/desktop-ports/deepin/tools/build_nirvana.py --upstream deepin-desktop-theme --out "$NEW_PRIVATE_DIR"
decthemectl assets import "$NEW_PRIVATE_DIR" --provides icons --license CC-BY-4.0 \
    --license-file nirvana/LICENSE-CC-BY-4.0.txt --license-file nirvana/REUSE-dep5.txt --path nirvana --name nirvana
```

`build_nirvana.py` is a standalone, stdlib-only CLI (plus `magick` for the WebP decode) and a **maintainer tool**: nothing runs it automatically and it is
not a native converter, which is what the section below asks the installer owner for. It refuses any revision but the pinned one and writes only into a
new directory (mode 0700, never overwritten). Every DCI file is hostile input: file-size (8 MiB), nesting (6), entries-per-directory (256), name
(UTF-8, NUL-terminated, no `/` or controls), declared-length, header-count, version, RIFF-size, size/scale-range (1..1024 / 1..4, plain decimals) and
layer-name checks run before a byte is used; a layer is plain only as `<prior>.webp` (anything with the spec's padding, palette or `.alpha8` fields
is refused); ImageMagick runs with resource limits, an empty environment and an explicit `webp:` coder, and only ever shrinks.

**Provenance.** The bytes it converts are hashed as Git blobs and compared with the objects of the pinned revision's tree (`a0c79e3`:
`nirvana/dsg-icons/nirvana`, tree `fb56b39d...`, the id the installer's catalogue would pin): a link is checked by its text and then the sibling it
points to, the two licence files (`LICENSES/CC-BY-4.0.txt`, `.reuse/dep5`) likewise, and any file the revision does not list is an untracked input.
Any difference stops the run before anything is written; `--allow-working-tree` converts anyway and the manifest and `NOTICE.txt` then say
"working-tree, NOT revision-verified". On the real checkout: status `git-objects`, 1,439 tree entries, 0 problems, a `git_blob` for every icon.
Its self-test (`python3 -I tools/test_build_nirvana.py`, run by `tests/deepin_port.rs`) feeds it truncated, mis-counted, mis-named, over-deep and
over-wide containers, malformed or out-of-range size/scale directories, palette and padded layer names, and a throwaway Git repository with a modified
tracked file, an untracked file, a retargeted or replaced link, a link leaving the directory and a modified licence file.

Measured on the pinned revision: **1,435 of 1,439** icons converted (128 px at most, 12 MB, about 80 s). Four are refused, each with its reason in
`nirvana/MANIFEST.json` (`skipped`):

* `deepin-virtualkeyboard`, `network-display-failed`, `network-display-succeed`: the layer is `1.0.webp`, i.e. palette field 0 (a foreground-tinted layer
  the palette engine would colour); an earlier draft of the tool wrongly converted these as plain images.
* `deepin-xdgicon-convert` (sha256 `d7cb5d10...3577`, 21,602 bytes): genuine upstream damage, a root directory entry `256` that **declares 31,888 bytes
  but only 21,522 remain**.

Converted output imports through the native installer unchanged (`assets import ... --license-file nirvana/LICENSE-CC-BY-4.0.txt --license-file
nirvana/REUSE-dep5.txt`: 1,441 files, content hash recorded as a `local` pack: an import is not pinned to a git tree, so `verified: tree no`) and
`assets verify` reports it ok. `nirvana/MANIFEST.json` lists, per icon, the Git blob and sha256 of the source DCI, the layer used, its source pixels and
the sha256 of the PNG (valid for the recorded ImageMagick build; the source pins are the stable ones).

## Request to the asset-installer owner: a typed native conversion (optional, later)

`image` 0.25 with its `webp` feature is already a dependency, so a native, Python-free conversion fits the installer's model (read pinned
bytes, verify the git tree, build into staging, never exec). Proposed catalogue shape (rejected by today's validator, so it is **not** in
`asset-packs.toml`):

```toml
[[source]]
id = "deepin-desktop-theme"
repo = "https://github.com/linuxdeepin/deepin-desktop-theme"
revision = "master"
commit = "a0c79e31467ab602d96605475dca69eb835c5d90"
license = "CC-BY-4.0"
license_files = ["LICENSES/CC-BY-4.0.txt", ".reuse/dep5"]
# no archive_url: the repository is ~1 GB; use --source-dir with a sparse clone (recipe above)

[[pack]]
id = "deepin-nirvana"
name = "nirvana"
provides = ["icons"]
source = "deepin-desktop-theme"
path = "nirvana/dsg-icons/nirvana"
tree = "fb56b39daf1364bc2c62baa13a5b672292845725"     # git rev-parse a0c79e3:nirvana/dsg-icons/nirvana
install_name = "nirvana"
convert = "dci"            # proposed: every *.dci in `path` -> <size>/<name>.png
convert_size = 128         # proposed: px, one of 64/96/128/192/256
inherits = "bloom"         # proposed: index.theme Inherits (nirvana's own stub says bloom,Papirus)
```

Rules to implement (all measured; the reference implementation is `tools/build_nirvana.py`, the format is the DSG icon file specification,
deepin-specifications d00a44b `unstable/图标文件规范.md`): header `DCI\0`, version 1, 3-byte LE root count; entry = type (1 file, 2 directory,
3 link) + 63-byte NUL-terminated name + 8-byte LE size + content; path `<size>/<state>.<tone>/<scale>/<layer>`; nirvana stores one plain WebP
`256/normal.light/3/1.webp` (768 x 768, a few smaller icons at 16, 24, 48 or 64) per icon and a link for `normal.dark`. Convert only a single plain WebP layer of the largest size and
scale; refuse and list everything else (any layer not named `<prior>.webp`, several layers, other formats); icon names `[A-Za-z0-9][A-Za-z0-9._+-]{0,127}`;
refuse the whole file on any length that exceeds its container. The output must keep the CC-BY-4.0 text and a notice that the images were
resized. Test vectors: the 1,435 `git_blob` / `source_sha256` values in `nirvana/MANIFEST.json`, the four refusals above, and the synthetic containers in
`tools/test_build_nirvana.py`.

Until that exists the answer for "install Deepin's default icons" is: Bloom through the catalogue (`deepin-bloom`, what the toolkit actually
selects) and, for maintainers who want nirvana, the recipe above.
