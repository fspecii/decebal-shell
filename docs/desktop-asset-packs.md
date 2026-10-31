# Desktop asset packs: icon and cursor themes from pinned upstream sources

Desktop ports (openKylin, Deepin, Lingmo, ...) look right only with their own icon and cursor themes. Those themes are
large (50-60 MB, tens of thousands of files and alias symlinks) and mostly GPL / CC-BY-SA licensed, so they are **not**
copied into this MIT repository and they are **not** part of a `.dtheme` package (which only carries images, fonts and
text). Instead a port ships a small declarative file that pins where the assets come from, and decebal's installer
(`src/desktop_assets/`) fetches or imports them into the user's icon folder, verifying every byte, keeping the
upstream licence next to the files, and never running anything from the upstream tree.

Installed packs are ordinary freedesktop icon / cursor themes: the Icons pane, `decthemectl icons` and the cursor
setting of "Apply to all apps" see them like any other installed theme, and each stays independently selectable.

* [Companion metadata](#companion-metadata-asset-packstoml) (the contract for port owners)
* [Commands](#commands) * [What install does](#what-install-and-import-do) * [Safety](#safety-model)
* [Selection and theme recommendations](#selection-and-theme-recommendations) * [Installed provenance](#installed-provenance)
* [Limits](#limits-and-known-gaps)

## Companion metadata (`asset-packs.toml`)

Each port owns `assets/desktop-ports/<family>/asset-packs.toml`. It holds data only: no commands, no build steps, no
URLs that are not `https`. The shared theme manifest (`theme.toml`) is **not** extended; the link between a theme and
its packs is the `[[recommend]]` table here.

```toml
format = 1                      # required; a reader refuses other numbers
family = "deepin"               # required; equals the folder name under assets/desktop-ports/

[[source]]                      # one pinned upstream tree, shared by the packs cut from it
id = "deepin-icon-theme"        # required: [a-z0-9][a-z0-9-]*, at most 64, unique in the file
repo = "https://github.com/linuxdeepin/deepin-icon-theme"   # required, https; informational
revision = "master"             # required: the branch or tag that was pinned (informational)
commit = "5d11ba4e4013a4d1e1c601265d55c66b95184922"         # required: 40 hex; checked against a git archive's pax comment
license = "GPL-3.0-or-later"    # required: SPDX expression as the upstream states it
license_files = ["LICENSES/GPL-3.0-or-later.txt", "LICENSES/CC-BY-4.0.txt"]  # required, >= 1: paths in the source root
# optional: lets `decthemectl assets install ID` download the source itself
archive_url = "https://github.com/linuxdeepin/deepin-icon-theme/archive/5d11ba4e4013a4d1e1c601265d55c66b95184922.tar.gz"
archive_sha256 = "<64 hex of the archive file>"   # required when archive_url is set
strip_components = 1            # leading path parts of the archive to drop (GitHub/Gitee archives: 1); default 0

[[pack]]
id = "deepin-bloom"             # required, unique across ALL catalogues: prefix it with the family
name = "Bloom"                  # required: what lists show
provides = ["icons", "cursors"] # required: "icons", "cursors" or both (bloom/ carries both)
source = "deepin-icon-theme"    # required: a [[source]] id of this file
path = "bloom"                  # required: the theme folder inside the source ("." = the root)
tree = "<40 hex>"               # required: `git rev-parse <commit>:<path>`, verified on every install
install_name = "bloom"          # optional: folder under ~/.local/share/icons (= the theme id users select);
                                #   default: the last part of `path`. [A-Za-z0-9][A-Za-z0-9._+-]*, at most 64
license = "GPL-3.0-or-later"    # optional: overrides the source's licence for this pack
license_files = ["bloom/COPYING"]   # optional: more files (source-root relative) copied with this pack
link_sources = []               # optional: other source folders whose files symlinks of `path` may point into
                                #   (e.g. ["ocean"] for ocean-dark); such links become plain copies
description = "..."             # optional

[[recommend]]                   # optional: which packs a theme of the port suggests
theme = "deepin"                # required: a theme id (themes/<id>/theme.toml)
icons = "deepin-bloom"          # optional: a pack id providing icons
cursors = "deepin-bloom"        # optional: a pack id providing cursors
```

Rules the validator enforces (`decthemectl assets catalog check FILE`, also a `cargo test` over every catalogue in
the repository):

* unknown keys are errors ("did you mean ...?"); ids are unique; `source` / `recommend.*` refer to existing ids;
  a recommended pack provides the kind it is recommended for;
* every path is relative, `/`-separated, without `..`, `.` parts, backslashes or control characters;
* `commit` and `tree` are 40 lowercase hex digits, `archive_sha256` 64; URLs are `https://`;
* `license` is non-empty and `license_files` lists at least one file per pack (source + pack lists combined).

Catalogues compiled into decebal are listed in `src/desktop_assets/builtin.rs` (one `include_str!` line per family,
added by the root when a port lands); a test fails when a file under `assets/desktop-ports/*/asset-packs.toml` is
missing from that list. Extra catalogues are read at run time from `$DECEBAL_ASSET_CATALOGS` (colon-separated files or
folders), `~/.config/decebal/asset-packs/*.toml` and `$XDG_DATA_DIRS/decebal/asset-packs/*.toml`.

How a port owner gets the pins from a local clone (read-only git queries, nothing from the repository is run):

```sh
git -C deepin-icon-theme rev-parse HEAD            # commit
git -C deepin-icon-theme rev-parse HEAD:bloom      # tree of the pack's path
curl -L -o a.tgz "$archive_url" && sha256sum a.tgz # archive_sha256 (optional)
```

## Commands

`decthemectl assets help` prints them. `--root DIR` (or `$DECEBAL_ASSET_ROOT`) installs somewhere other than
`$XDG_DATA_HOME/icons`; `--json` gives machine-readable output for `list`, `info`, `status`, `verify`.

| command | does |
|---|---|
| `list` | every catalogue pack (installed / not installed / modified / name taken by a foreign theme) and every installed decebal pack |
| `info ID\|NAME` | metadata, pins, licence, installed state and recommendations of one pack |
| `install ID [--archive FILE \| --source-dir DIR] [--force] [--dry-run]` | install a catalogue pack: download `archive_url` (https, `archive_sha256` checked), or a pre-downloaded archive (sha256 checked), or a local clone of the source (checked by the `tree` OID only) |
| `import PATH --provides icons,cursors --license SPDX [--license-file REL]... [--name NAME] [--path REL] [--strip N] [--tree OID] [--sha256 HEX] [--force] [--dry-run]` | install an archive or folder that is not in a catalogue (recorded as `local`; the licence is still required) |
| `verify [NAME...]` | re-hash installed packs against their provenance record |
| `remove NAME [--force]` | delete a pack decebal installed (never a theme it did not install; `--force` to remove one in use) |
| `use icons\|cursors NAME\|auto` | select a theme explicitly (survives theme changes) |
| `use icons\|cursors theme` | hand the choice back to the theme's recommendation |
| `status` | the icon and cursor selection, who made it (you / theme / desktop), the theme's recommendation and whether it is installed |
| `preview NAME [--png OUT]` | resolve sample icons and the core cursors in a pack, optionally draw them to a PNG |
| `catalog check FILE...` | validate companion metadata |

## What install and import do

1. **Stage**: a private folder `<root>/.decebal-staging/<name>.<pid>.<n>/` (mode 0700, same file system as the target, so
   the final step is a `rename`). A lock file there serialises concurrent installs.
2. **Read** the source as a stream: `.tar`, `.tar.gz` / `.tgz` (gzip is decompressed with a cap while it is read), or a
   folder (walked without following links). Only `path`, the licence files and `link_sources` are extracted;
   everything else is skipped unread. A git archive's pax comment is compared with `commit`.
3. **Verify**: the git tree OID of `path` (blob/tree objects computed from the exact bytes and modes read) must equal
   `tree`; for downloads and `--archive` the SHA-256 of the archive must equal `archive_sha256`. Any mismatch stops here.
4. **Build** the theme folder: icon files (`.svg`, `.png`, `.xpm`), `index.theme` / `cursor.theme`, Xcursor files under
   `cursors/` (magic `Xcur` checked), and text (`README*`, `COPYING*`, `LICENSE*`, `AUTHORS*`, `*.md`, `*.txt`).
   Everything else (build files, scripts, `Makefile`, `CMakeLists.txt`, `icon-theme.cache`, ...) is left out and counted.
   Files are written 0644, folders 0755: executable bits never survive. Internal symlinks are kept (relative);
   links into `link_sources` become copies; links to nothing are dropped with a warning.
5. **Validate**: icons need an `index.theme` with `[Icon Theme]` and existing `Directories`; cursors need `cursors/`
   with a core pointer (`left_ptr` or `default`). `Inherits=` names must be plain names; a theme inheriting itself (DMZ's
   `cursor.theme`) is fine, missing parents are warnings. A cursor-only pack without `index.theme` gets a two-line one.
6. **Licence**: every licence file is copied to `<theme>/LICENSES/`; a missing one stops the install.
7. **Provenance**: `<theme>/.decebal-asset.json` (below) with the content hash of the result.
8. **Install** atomically. The target `<root>/<install_name>`:
   * absent: `rename` the staged folder into place;
   * a theme decebal did not install (no provenance file): **refused**, always (remove it yourself, or `--name`);
   * the same pack with the same content: nothing to do;
   * an older / different decebal pack: needs `--force`; the old folder is renamed aside, the new one renamed in, the old
     one deleted; if the second rename fails the old one is renamed back (rollback).
   A theme of the same name in a later root (`/usr/share/icons/bloom`) is reported: the new one shadows it.
   On any error the staging folder is removed; the target is untouched.

## Safety model

* **Nothing is executed.** No upstream `install.sh`, `Makefile`, `CMakeLists.txt`, post-install hook, `xcursorgen` or
  shell is run, and no value from metadata or an archive reaches a shell. Downloads use the built-in HTTPS client.
* **Paths**: archive entries and folder walks refuse absolute paths, `..`, empty or `.` parts, backslashes, control
  characters, paths over 255 bytes or 24 levels, duplicate entries and entries below a symlink entry.
* **Links**: symlink targets must be relative and resolve (lexically, through at most 16 hops) inside the source;
  absolute or escaping targets refuse the pack. No symlink is ever created in the staging source tree, so a later entry
  cannot be written through one. Hard links are copies of an earlier entry. Devices, FIFOs, sparse files and base-256
  sizes are refused.
* **Caps** (defaults): 120 000 entries, 16 MiB per file, 1 GiB extracted, 512 MiB archive, 1 MiB per licence file.
* **Ownership**: only folders carrying `.decebal-asset.json` are ever replaced or removed.

## Selection and theme recommendations

* The icon theme is `decebal.json` `icons.theme` (unchanged, `docs/icons.md`); the cursor theme is `system.cursorTheme`,
  which "Apply to all apps" writes to GTK `settings.ini`, gsettings and `Xcursor.theme` (unchanged).
* Who chose them is kept in `decebal.json` `asset_packs`:

```json
"asset_packs": {
  "icons":   {"user": true},
  "cursors": {"theme": "deepin", "applied": "bloom", "baseline": null},
  "recommended": {"theme": "deepin", "icons": "bloom", "cursors": "bloom", "missing": []}
}
```

* `decthemectl assets use`, the Icons pane and a hand edit of `icons.theme` / `system.cursorTheme` (the value no longer
  equals `applied`) make the choice **explicit** (`user: true`): applying a theme never changes it again; the
  recommendation is still shown. `use ... theme` (or the pane's *Use the theme's suggestion*) clears it.
* Otherwise `theme apply` sets the slot to the theme's recommendation, remembering the previous value as `baseline`;
  a later theme that recommends nothing puts `baseline` back. A recommendation counts only when the pack is installed;
  otherwise it is listed in `recommended.missing` and the pane offers to install it.
* Recommendation sources, first wins: the companion `[[recommend]]` entry for the theme id, then the theme's own
  `[icons] theme` (icons only; existing behaviour). The other `[icons]` keys (scale, tint, fallback) merge as before.
* Wallpaper and widget policies are not touched.

## Installed provenance

`<theme>/.decebal-asset.json` (hidden, ignored by toolkits):

```json
{
  "format": 1, "pack": "deepin-bloom", "name": "Bloom", "provides": ["icons", "cursors"], "install_name": "bloom",
  "family": "deepin", "origin": "catalog",
  "source": {"id": "deepin-icon-theme", "repo": "https://github.com/linuxdeepin/deepin-icon-theme",
             "revision": "master", "commit": "5d11...", "path": "bloom", "tree": "3c8e...",
             "archive_sha256": "...", "fetched_from": "archive|download|directory"},
  "verified": {"tree": true, "archive_sha256": true, "commit": true},
  "license": "GPL-3.0-or-later", "license_files": ["LICENSES/GPL-3.0-or-later.txt"],
  "content_hash": "sha256:...", "files": 9512, "symlinks": 7542, "skipped": 12,
  "installed_by": "decebal 0.2.1"
}
```

`content_hash` is `sha256` over the sorted entries (`path NUL F NUL size NUL sha256(file) LF` for files,
`path NUL L NUL target LF` for links), without the provenance file itself; `verify` recomputes it.

## Limits and known gaps

* Formats: `.tar`, `.tar.gz`, folders. `.tar.xz`, `.zip` and `.7z` are refused with a message (no decoder in the
  dependency set); unpack them yourself and `import` the folder.
* Packs that need a build step (SVG cursors rendered by `inkscape` + `xcursorgen`) are importable only from their
  prebuilt output; the installer does not render or compile anything.
* `git archive` honours `export-ignore` / `export-subst`; an upstream using them gives a different tree OID for the
  archive than for the clone. Pin what you verify.
* decebal's own surfaces do not load Xcursor themes themselves; the cursor theme reaches applications through
  "Apply to all apps" (GTK, gsettings, X resources) and a new session.
