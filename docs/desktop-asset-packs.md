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
archive_sha256 = "<64 hex>"     # optional: only for byte-stable files (release assets); forge-generated archives
                                #   change bytes without changing content, the tree ids pin the content
strip_components = 1            # leading path parts of archives to drop: 1 for GitHub/Gitee/`git archive --prefix`
                                #   tarballs (set it even without archive_url: `--archive` uses it); default 0

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
index = ["icons/breeze.theme.in", "icons/commonthemeinfo.theme.in"]   # optional: source files concatenated into
                                #   index.theme when the theme folder has none (Adwaita: "index.theme" in the repo root)
index_oids = ["<40 hex>", "<40 hex>"]   # required with index: `git rev-parse <commit>:<file>` of each, same order
description = "..."             # optional

[[pack.xcursor]]                # optional, repeatable: a cursor built from PNGs of the pack (needs "cursors" in provides)
name = "left_ptr"               # the file in cursors/
config = """                    # xcursorgen's own line format: size xhot yhot file.png [delay_ms], files relative to `path`
24 4 0 24x24/left_ptr.png
32 4 0 32x32/left_ptr.png
"""
aliases = ["arrow", "top_left_arrow"]   # links to it in cursors/

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
* `commit`, `tree` and `index_oids` are 40 lowercase hex digits, `archive_sha256` 64 (and needs `archive_url`);
  URLs are `https://`;
* `license` is non-empty and `license_files` lists at least one file per pack (source + pack lists combined).

Catalogues compiled into decebal are listed in `src/desktop_assets/builtin.rs` (one `include_str!` line per family,
added by the root when a port lands); a test fails when a file under `assets/desktop-ports/*/asset-packs.toml` is
missing from that list. Extra catalogues are read at run time from `$DECEBAL_ASSET_CATALOGS` (colon-separated files or
folders), `~/.config/decebal/asset-packs/*.toml` and `$XDG_DATA_DIRS/decebal/asset-packs/*.toml`.

How a port owner gets the pins from a local clone (read-only git queries, nothing from the repository is run):

```sh
git -C deepin-icon-theme rev-parse HEAD            # commit
git -C deepin-icon-theme rev-parse HEAD:bloom      # tree of the pack's path
git -C adwaita-icon-theme rev-parse HEAD:index.theme   # index_oids
sha256sum release-asset.tar.gz                     # archive_sha256, only for byte-stable files
```

### Example catalogues

`docs/desktop-asset-packs/examples/` holds one catalogue per desktop family with real pins (taken on 2026-10-31) that
a port can copy or point to: `deepin.toml` (bloom, bloom-dark: icons + cursors), `openkylin.toml` (UKUI default and
classical icons, DMZ white / black cursors), `lingmo.toml` (ocean, ocean-dark), `kde.toml` (Breeze icons through the
two-file `index`, Breeze cursors as `breeze_cursors`) and `gnome.toml` (Adwaita through the root `index.theme`).
They recommend packs for the theme ids `deepin`, `deepin-dark`, `openkylin`, `lingmo`, `lingmo-dark`, `nextkde` and
`gnome-adwaita-like`. A test keeps them valid. They are not compiled in: a port adds its own
`assets/desktop-ports/<family>/asset-packs.toml` (the root lists it in `src/desktop_assets/builtin.rs`); until then
`DECEBAL_ASSET_CATALOGS=docs/desktop-asset-packs/examples` makes them available.

What did not fit the format, found while pinning them (the port owners decide):

* openKylin's own cursors (`icons/cursors/dark-sense`) are PNGs whose hotspots live in a shell script (`cursors.sh`
  runs `xcursorgen`). `docs/desktop-asset-packs/xcursorgen2toml.py SCRIPT` reads such a script **as text** (regular
  expressions over its `echo ... > NAME.in`, `xcursorgen` and `ln -s` lines; it never runs it) and prints the
  `[[pack.xcursor]]` tables; `openkylin.toml` carries the result for dark-sense (57 cursors, 36 aliases). A port
  owner should review the output against the script before shipping it.
* `lingmo-cursor-themes` states no licence (its `debian/copyright` is the unfilled template), so it is left out;
  Lingmo's `ocean` folder carries its own cursors.
* Breeze Dark icons are generated at build time (no `icons-dark/` in the repository).
* Current Adwaita ships no cursors in its repository.

## Commands

`decthemectl assets help` prints them. `--root DIR` (or `$DECEBAL_ASSET_ROOT`) installs somewhere other than
`$XDG_DATA_HOME/icons`; `--json` gives machine-readable output for `list`, `info`, `status`, `verify`.

| command | does |
|---|---|
| `list` | every catalogue pack (installed / not installed / the catalogue pins a newer revision / name taken by a foreign theme) and every installed decebal pack |
| `info ID\|NAME` | metadata, pins, licence, installed state and recommendations of one pack |
| `install ID [--archive FILE [--strip N] \| --source-dir DIR] [--force] [--dry-run]` | install a catalogue pack: download `archive_url` (https), or use a downloaded archive, or a local clone of the source; the `tree` id (and `archive_sha256`, the archive's commit, `index_oids`) are checked |
| `import PATH --provides icons,cursors --license SPDX --license-file REL... [--name NAME] [--path REL] [--strip N] [--link-source REL]... [--index REL]... [--tree OID] [--sha256 HEX] [--force] [--dry-run]` | install an archive or folder that is not in a catalogue (recorded as `local`; a licence file is still required). `REL` paths are relative to the source root |
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
   `tree`; a git archive's commit must equal `commit`; when pinned, the SHA-256 of the archive must equal
   `archive_sha256` (checked before the archive is read). Any mismatch stops here.
4. **Build** the theme folder: icon files (`.svg`, `.png`, `.xpm`), `index.theme` / `cursor.theme`, Xcursor files under
   `cursors/` (magic `Xcur` checked), and text (`README*`, `COPYING*`, `LICENSE*`, `AUTHORS*`, `*.md`, `*.txt`).
   Everything else (build files, scripts, `Makefile`, `CMakeLists.txt`, `icon-theme.cache`, ...) is left out and counted.
   Files are written 0644, folders 0755: executable bits never survive. Internal symlinks are kept (relative);
   links into `link_sources` become copies; links to nothing are dropped with a warning.
5. **Build cursors** declared with `[[pack.xcursor]]`: each PNG (read from the verified source, at most 256 px a side)
   becomes a premultiplied ARGB image of an Xcursor file with its hotspot (which must lie inside the image) and delay;
   aliases become links. A cursor both shipped in `cursors/` and declared is an error. The PNGs are not installed, and
   a cursor-only pack leaves out images outside `cursors/` and `cursors_scalable/`.
6. **Validate**: a theme folder without `index.theme` gets the concatenated `index` files (each checked against its
   `index_oids` blob id; they are read, not installed). Icons need an `index.theme` with `[Icon Theme]` and existing `Directories`; cursors need `cursors/`
   with a core pointer (`left_ptr` or `default`). `Inherits=` names must be plain names; a theme inheriting itself (DMZ's
   `cursor.theme`) is fine, missing parents are warnings. A cursor-only pack without `index.theme` gets a two-line one.
7. **Licence**: every licence file is copied to `<theme>/LICENSES/`; a missing one stops the install.
8. **Provenance**: `<theme>/.decebal-asset.json` (below) with the content hash of the result.
9. **Install** atomically. The target `<root>/<install_name>`:
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

**Icons pane** (System Settings -> Icons): besides the icon themes, a *Cursor theme* menu (installed cursor themes,
independent of the icon theme; choosing one is explicit), the current theme's suggestion with *Use the theme's
suggestion*, an *Install* button for each suggested pack that is missing and has a download (it runs
`decthemectl assets install ID` in the background, never through a shell, and shows the outcome), and the installed
packs with licence, source and revision.

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

## Tests

* `cargo test --lib desktop_assets`: tar reader (pax / GNU names, checksums, caps, hostile headers), SHA-1 and git tree
  ids against `git write-tree`, staging (traversal, links, write-through-link, duplicates, hard links, folder sources
  with the same tree id as their archive), assembly (allowlist, links, licences, index files, cursor-only packs),
  Xcursor parsing / building, install (pins, foreign / conflicting folders, tamper and `--force`, rollback without
  `renameat2`, the lock), the policy, the CLI, the pane rows, and the example catalogues.
* `cargo test --test desktop_assets`: the real `decthemectl` in a cleared environment on folder sources.

## Evidence (2026-10-31)

Private smoke runs (own HOME / XDG / config / icon root, no display or bus, desktop tools recorded instead of run) of
the real `decthemectl` against `git archive` tarballs and a clone of the pinned upstream commits:

* installed and verified (`tree yes`, `commit yes` for archives): Deepin `bloom` (6359 files, 7501 links, 176 cursors),
  openKylin `ukui-icon-theme-default`, DMZ-White, Lingmo `ocean` (from a clone), KDE `breeze` (two-file index) and
  `breeze_cursors`, GNOME `Adwaita` (root index); `preview` resolved 13-16 of 16 sample icons and 10 of 10 cursors
  from the packs themselves and drew them from the installed files;
* refused through the binary: `..` and absolute entries, absolute / escaping links, a write below a link, a device,
  a duplicate, a 64 MiB entry, a missing licence, a wrong `--sha256`, a wrong `--tree`, the wrong archive for a pack;
  nothing was written outside the icon root; a foreign theme folder survived `--force` and `remove`;
* a tampered install is reported by `verify` and repaired only with `--force`;
* openKylin dark-sense, from the 137 MB `openkylin-theme` archive with the catalogue data of `openkylin.toml`: 57 Xcursor
  files built natively (5 sizes each, `watch` 10 frames x 5 sizes), 36 alias links, the PNG sources and `cursors.sh`
  not installed; 10 of 10 sample cursors resolved and decoded;
* explicit icon and cursor choices survived applying `deepin`, `nextkde` and `gnome-adwaita-like`; theme-owned slots
  followed and went back to their baseline.

Upstream symlinks that dangle in the pinned commit itself (e.g. Lingmo `ocean-dark/apps/scalable ->
../../Crule/apps/scalable`, Deepin `uos-activator.svg -> deepin-license-activator.svg`) are dropped with a warning.

## Limits and known gaps

* Formats: `.tar`, `.tar.gz`, folders. `.tar.xz`, `.zip` and `.7z` are refused with a message (no decoder in the
  dependency set); unpack them yourself and `import` the folder.
* Packs that need a build step beyond "PNG + hotspot -> Xcursor" (SVG cursors rendered by `inkscape`, generated icon
  variants such as Breeze Dark) are importable only from their prebuilt output; the installer renders no SVG and runs
  no tool.
* `git archive` honours `export-ignore` / `export-subst`; an upstream using them gives a different tree OID for the
  archive than for the clone. Pin what you verify.
* decebal's own surfaces do not load Xcursor themes themselves; the cursor theme reaches applications through
  "Apply to all apps" (GTK, gsettings, X resources) and a new session. When a theme-owned cursor goes back to "not
  set", "Apply to all apps" leaves the last written GTK / X value in place (it only writes a set value).
* Licence files and `link_sources` outside the pack's `path` are covered by `archive_sha256` when that is pinned, not
  by the tree id; `index` files are pinned by `index_oids`.
