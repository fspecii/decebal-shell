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
license_oids = ["<40 hex>", "<40 hex>"]   # optional: `git rev-parse <commit>:<file>` of each, same order; pins the
                                #   licence texts independently of the archive bytes
# optional: lets `decthemectl assets install ID` download the source itself
archive_url = "https://github.com/linuxdeepin/deepin-icon-theme/archive/5d11ba4e4013a4d1e1c601265d55c66b95184922.tar.gz"
archive_sha256 = "<64 hex>"     # optional: only for byte-stable files (release assets); forge-generated archives
                                #   change bytes without changing content, the tree ids pin the content
strip_components = 1            # leading path parts of archives to drop: 1 for GitHub/Gitee/`git archive --prefix`
                                #   tarballs (set it even without archive_url: `--archive` uses it); default 0
git_url = "https://github.com/linuxdeepin/deepin-icon-theme.git"   # optional: fetch only the pinned git objects
                                #   (see "Fetching pinned git objects"); needs license_oids for every licence file

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
license_oids = ["<40 hex>"]     # optional, as for the source
link_sources = ["Papirus"]      # optional: other source folders that links of `path` point into (Papirus-Dark's size
                                #   folders link to ../Papirus). File links become copies, folder links are expanded
                                #   into real folders: the installed pack never links outside itself
link_source_trees = ["<40 hex>"]  # `git rev-parse <commit>:<folder>` of each link source, same order. Without it a
                                #   catalogue install needs a matching archive_sha256, else it is refused before anything
                                #   is built
index = ["icons/breeze.theme.in", "icons/commonthemeinfo.theme.in"]   # optional: source files concatenated into
                                #   index.theme when the theme folder has none (Adwaita: "index.theme" in the repo root)
index_oids = ["<40 hex>", "<40 hex>"]   # required with index: `git rev-parse <commit>:<file>` of each, same order
description = "..."             # optional

[[pack.text_links]]             # optional (at most 4): a folder of REGULAR files that each hold a link target as text,
from = "links"                  #   a link tree stored without its link modes (Lingmo's links/). Only a folder named here is
tree = "<40 hex>"               #   read this way. Required: `git rev-parse <commit>:<from>`, verified before any text is read
onto = "."                      #   optional: where its files land, relative to `path` (default its root)
replace = false                 #   optional: true lets an alias replace a pack file of the same name (upstream copied the
                                #   links over the files); otherwise that is refused

[[pack.dir_alias]]              # optional (at most 256): a folder link the upstream build creates
name = "actions@2x"             #   pack-relative; must not exist yet
target = "actions"              #   pack-relative folder; must exist and must not enclose `name`

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
* `commit`, `tree`, `index_oids`, `link_source_trees` and `license_oids` are 40 lowercase hex digits, one per listed
  folder / file in the same order; `archive_sha256` is 64 (and needs `archive_url`);
* a `link_sources` folder does not overlap the pack's `path`;
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

### Declared aliases (`text_links`, `dir_alias`)

Some upstreams create links at build time instead of storing them: Lingmo's icon theme keeps 14 137 alias files in
`links/` as regular text files (each holds a target such as `media-playback-pause.svg`) and its CMake install copies
them over `src/` and adds `actions@2x -> actions` (and `@3x`) for eight contexts. A pack declares that data, nothing is
run, and nothing undeclared is guessed:

* only a folder named in `[[pack.text_links]]` is read as aliases, and only after its own tree id matched (for any
  origin; a missing pin refuses the install);
* each file must be a plain relative target: UTF-8, at most 1 KiB, no control character, line break or backslash, not
  absolute, no empty component, and it must resolve inside the pack folder from the place it lands. Anything else
  (binary data, a real symlink in the folder, a path out of the pack) refuses the pack;
* an alias may not name a folder of the pack or lie below a file; replacing a pack file needs `replace = true`;
* a folder alias must point at an existing folder of the pack that does not enclose it;
* the aliases then become ordinary links of the staged source: chains collapse to the final file (at most 16 hops,
  longer ones count as loops), loops and links to nothing are dropped with at most 20 warning lines plus a total,
  and the installed links never leave the pack. Alias names without an icon extension are left out like any file;
* provenance records `aliases = [text links, pack files replaced, folder aliases]`, the folder's tree among
  `link_sources`, and `text_links` in `verified.outside`.

Real result (private root, a clean clone at `0b6a4bab`): 14 137 aliases (15 replacing `src/` files), 16 folder aliases,
96 of 96 icon directories (was 32), 33 aliases that dangle upstream dropped, 0 links leaving the pack, `complete: yes`.

### DSG icon containers (`[pack.dci_convert]`)

Deepin 25's default icon theme, nirvana, is stored as DCI containers (the DSG icon file specification: `DCI\0`, version 1,
a 3-byte little-endian root count; entries of a 1-byte type (1 file, 2 folder, 3 link), a 63-byte NUL-terminated UTF-8
name, an 8-byte little-endian size and the content; images at `<size>/<state>.<tone>/<scale>/<layer>`). A pack opts in
with one typed table; nothing else in the format changes:

```toml
[[pack]]
id = "deepin-nirvana"
name = "nirvana"
provides = ["icons"]                       # icons only
source = "deepin-desktop-theme"
path = "nirvana/dsg-icons/nirvana"         # holds the *.dci files (and links between them), no index.theme
tree = "fb56b39daf1364bc2c62baa13a5b672292845725"   # verified, with the licence blob ids, BEFORE any container is parsed
install_name = "nirvana"

[pack.dci_convert]
size = 128                                 # output pixels: 64 | 96 | 128 | 192 | 256; images are only ever shrunk
inherits = ["bloom"]                       # Inherits= of the generated index.theme (theme names)
```

What the installer does, natively (no subprocess, no new dependency: `image` already decodes WebP):

* every `<name>.dci` directly in `path`, `name` matching `[A-Za-z0-9][A-Za-z0-9._+-]{0,127}`, is parsed as hostile input:
  at most 8 MiB, nesting 6, 256 entries a folder, names UTF-8 without `/` or control characters, every declared
  length checked against its container before use, the root count must match, unknown entry types refused;
* the image used is the single layer of `normal.light` at the largest size and scale (sizes 1..1024, scales 1..4, plain
  decimals). It is converted only when it is a *plain* layer `<priority>.webp` with a consistent RIFF size, a
  still (not animated) WebP of at most 4 MiB and 1536 x 1536 pixels (read from its header before decoding). Palette,
  padding, colour-adjust, `.alpha8` and multi-layer images need Deepin's palette engine and are **refused for that
  icon**, as is any malformed container: the icon is listed with its reason, never partly converted;
* a link `a.dci -> b.dci` (to a sibling inside `path`) becomes `<size>/a.png -> b.png` when `b` converted, else it is
  refused with `b`'s reason; links anywhere else are refused;
* budgets over the whole pack: 20 000 containers, the source bytes of the install, 2^31 decoded pixels and 256 MiB of
  PNG output; exceeding one refuses the install;
* the output is `<size>/<name>.png`, a generated `index.theme`, the licence files (copied, never invented), a
  `NOTICE.txt` saying the images were decoded and shrunk to at most `size` pixels, and `CONVERSION.json` listing for
  every icon the source git blob, its sha256, the layer used, its pixels and the PNG's sha256, every refused icon with
  its reason, and every link. The install report and the provenance record the converted / refused counts: an install
  with refused icons is reported as such, not as complete.

Real pinned input (deepin-desktop-theme a0c79e3, tree fb56b39d: 668 containers and 771 links), through
`decthemectl assets install deepin-nirvana --source-dir <clean sparse clone>` into a private root: 1 435 of 1 439 icons
(664 converted, 771 links to them), the same 4 refusals as the maintainer recipe
`assets/desktop-ports/deepin/tools/build_nirvana.py` with identical reasons (three palette layers `1.0.webp`, and
`deepin-xdgicon-convert`: "entry '256' declares 31888 bytes but only 21522 remain"), 0 differences from its
`MANIFEST.json` in source git blob, source sha256, layer and source pixels, tree and licence blobs verified before
conversion, `verify` ok, no link leaving the theme, PNGs at most 128 px. Two installs into different roots gave the same
content hash. A debug build takes about four minutes. The PNG bytes differ from the recipe's (a different encoder); the
source pins are the stable proof.

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

### Fetching pinned git objects (`git_url`)

For a source without a usable archive (deepin-desktop-theme is ~1 GB of history and its archive is mostly other
themes), a `[[source]]` may name `git_url`: `https://host/path`, a host name and plain path parts, no user, port,
query or fragment. `install ID` uses it when there is no `archive_url` (`--git` chooses it explicitly). It speaks git's
smart HTTP protocol version 2 (git's `gitprotocol-v2` / `gitprotocol-http` / `gitformat-pack` documents) with the
built-in HTTPS client and asks only for objects the catalogue pins by id, so every pack of such a source needs
`license_oids` for every licence file and `link_source_trees` for every `link_sources` folder (the validator refuses
the catalogue otherwise; `tree`, `text_links.tree` and `index_oids` are required anyway).

Three requests, each reply untrusted and capped while it streams:

| request | asks for | cap |
|---|---|---|
| `GET <git_url>/info/refs?service=git-upload-pack`, `Git-Protocol: version=2` | the capability advertisement: version 2, `fetch` with `shallow` and `filter`, SHA-1 ids | 64 KiB |
| `fetch`: `want <commit>`, `deepen 1`, `filter blob:none` | the pinned commit and its trees, no file content | 64 MiB |
| `fetch`: `want` each pinned tree and blob, no filter | exactly those, with everything below the trees | `max_archive` (512 MiB) |

* **Binding**: after the second request every pin must be found at its path below the pinned commit's own tree
  (`<commit>:<path>` for the pack's `tree`, `link_source_trees`, `text_links`, `license_oids`, `index_oids`), so the
  content is the commit's, wherever the server stored it. `verified.commit` means exactly this for a git fetch.
* **Network**: https only, **no redirects** (a moved repository is an error naming where it went), no credentials,
  cookies or netrc; connect 15 s, each read 60 s, each request a deadline (1 min, 15 min for the packs). The pane's
  install supervisor (timeout, stop) bounds the whole install as for downloads.
* **Responses**: pkt-lines of 4..65520 bytes; an `ERR` line or side band 3 stops with the server's text; progress
  (band 2, `no-progress` is requested) at most 1 MiB; only the sections a shallow fetch has; nothing after the end.
* **Packs** are written to the job folder while they stream and read from there: version 2, the object count exact,
  the SHA-1 trailer checked, nothing between the last object and it; types commit, tree, blob, tag and the two delta
  types only; no thin packs (every delta base in the same pack; an offset base must be an earlier object's start);
  delta chains of at most 64; every copy / insert instruction checked against its base and result; at most
  `max_entries` objects, each at most `max_file` (16 MiB) inflated, an object inflating to more or less than its header
  says is refused, and all inflating together is bounded by `max_stream` (4 GiB of work, a compression bomb stops
  there). Objects are rebuilt from the file when needed: memory follows the largest object, not the pack. Every
  object's id is recomputed from its bytes; an object asked for and not sent refuses the install.
* **Staging**: the pinned trees are expanded through the same reader as archives and folders (path rules, link rules,
  entry, size and depth caps; gitlinks, non-canonical modes, unsorted or repeated tree entries and names with `/`,
  `..`, backslashes or control characters are refused), and the tree ids are then computed again from the staged bytes
  exactly as for an archive. Conversion (`[pack.dci_convert]`) and everything after it are unchanged.
* **Failure** of any step installs nothing and says so, naming `--source-dir` / `--archive` as the way that still works
  (an upstream that rewrote its history answers `not our ref`: the pin is gone from the forge, not changed).
* **Provenance**: `fetched_from: "git"`, `url`, and `git: {commit_chain, objects, requests: [{what, bytes, sha256}]}`.

Identity is SHA-1, as for every tree pin of this format; decebal does not run collision detection (GitHub refuses
colliding objects on push). The forge sees the request (address, user agent), as for archive downloads.

Real result (pinned deepin-desktop-theme a0c79e3, tree fb56b39d, private HOME and root): see "Evidence".

## Commands

`decthemectl assets help` prints them. `--root DIR` (or `$DECEBAL_ASSET_ROOT`) installs somewhere other than
`$XDG_DATA_HOME/icons`; `--json` gives machine-readable output for `list`, `info`, `status`, `verify`.

| command | does |
|---|---|
| `list` | every catalogue pack (installed / not installed / the catalogue pins a newer revision / name taken by a foreign theme) and every installed decebal pack |
| `info ID\|NAME` | metadata, pins, licence, installed state and recommendations of one pack |
| `install ID [--archive FILE [--strip N] \| --source-dir DIR \| --git] [--force] [--dry-run]` | install a catalogue pack: download `archive_url` (https), or fetch the pinned git objects from `git_url` (when there is no `archive_url`, or with `--git`), or use a downloaded archive, or a local clone of the source; the `tree` id (and `archive_sha256`, the archive's commit, `index_oids`) are checked |
| `import PATH --provides icons,cursors --license SPDX --license-file REL... [--name NAME] [--path REL] [--strip N] [--link-source REL]... [--index REL]... [--tree OID] [--sha256 HEX] [--force] [--dry-run]` | install an archive or folder that is not in a catalogue (recorded as `local`; a licence file is still required). `REL` paths are relative to the source root |
| `verify [NAME...]` | re-hash installed packs against their provenance record; also reports DEGRADED packs (exit 1) |
| `remove NAME [--force]` | delete a pack decebal installed (never a theme it did not install; `--force` to remove one in use) |
| `use icons\|cursors NAME\|auto` | select a theme explicitly (survives theme changes) |
| `use icons\|cursors theme` | hand the choice back to the theme's recommendation |
| `status` | the icon and cursor selection, who made it (you / theme / desktop), the theme's recommendation and whether it is installed |
| `preview NAME [--png OUT]` | resolve sample icons and the core cursors in a pack, optionally draw them to a PNG |
| `catalog check FILE...` | validate companion metadata |

## What install and import do

1. **Lock and stage**: the install lock (`<root>/.decebal-staging/lock`) is taken **first** and held to the end; every
   observation of the target is made under it. A private folder `<root>/.decebal-staging/<name>.<pid>.<n>/` (mode
   0700, same file system as the target, so the final step is a `rename`) holds the work.
2. **Snapshot**: an archive given with `--archive` is copied into the job folder while it is hashed (bytes capped as
   they stream); a download is written there while it is hashed. Only that owned snapshot is sniffed and read, so the
   bytes that were checked against `archive_sha256` are the bytes that are staged.
   **Read** the source as a stream: `.tar`, `.tar.gz` / `.tgz` (gzip is decompressed with a cap while it is read), or a
   folder (walked without following links). Only `path`, the licence files and `link_sources` are extracted;
   everything else is skipped unread. A git archive's pax comment is compared with `commit`.
3. **Verify**: the git tree OID of `path` (blob/tree objects computed from the exact bytes and modes read) must equal
   `tree`; a git archive's commit must equal `commit`; when pinned, the SHA-256 of the archive must equal
   `archive_sha256` (checked before the archive is read). Any mismatch stops here.
4. **Build** the theme folder: icon files (`.svg`, `.png`, `.xpm`), `index.theme` / `cursor.theme`, Xcursor files under
   `cursors/` (magic `Xcur` checked), and text (`README*`, `COPYING*`, `LICENSE*`, `AUTHORS*`, `*.md`, `*.txt`).
   Plasma 6 `cursors_scalable/<name>/metadata.json` is kept when it is a JSON array or object of at most 64 KiB.
   Everything else (build files, scripts, `Makefile`, `CMakeLists.txt`, `icon-theme.cache`, ...) is left out and counted.
   Folder links into `link_sources` are expanded from the staged source: links inside a copy stay links, a folder
   copied once is linked (inside the pack) when it is referenced again, a link to an enclosing folder is cut, overlapping
   writes are refused, and depth (8), entries and bytes are bounded by the install limits.
   Files are written 0644, folders 0755: executable bits never survive. Internal symlinks are kept (relative);
   links into `link_sources` become copies; links to nothing are dropped with a warning.
5. **Build cursors** declared with `[[pack.xcursor]]`: each PNG (read from the verified source, its size taken from the
   PNG header and checked against 256 px a side and one budget for all cursors of the pack, 64 MiB / 4096 images,
   before it is decoded; a PNG reached through a link into `link_sources` counts as an outside input)
   becomes a premultiplied ARGB image of an Xcursor file with its hotspot (which must lie inside the image) and delay;
   aliases become links. A cursor both shipped in `cursors/` and declared is an error. The PNGs are not installed, and
   a cursor-only pack leaves out images outside `cursors/` and `cursors_scalable/`.
6. **Validate** (and measure): fewer than half of the `Directories` of index.theme present marks the pack
   **DEGRADED** (report, provenance, `verify`, `info`, `list`, the pane); fewer than a quarter refuses it unless
   `--allow-degraded`. Then: a theme folder without `index.theme` gets the concatenated `index` files (each checked against its
   `index_oids` blob id; they are read, not installed). Icons need an `index.theme` with `[Icon Theme]` and existing `Directories`; cursors need `cursors/`
   with a core pointer (`left_ptr` or `default`). `Inherits=` names must be plain names; a theme inheriting itself (DMZ's
   `cursor.theme`) is fine, missing parents are warnings. A cursor-only pack without `index.theme` gets a two-line one.
7. **Licence**: every licence file is copied to `<theme>/LICENSES/`; a missing one stops the install.
8. **Provenance**: `<theme>/.decebal-asset.json` (below) with the content hash of the result.
9. **Install** atomically. The target `<root>/<install_name>`:
   * absent: `rename` the staged folder into place;
   * a theme decebal did not install (no provenance file): **refused**, always (remove it yourself, or `--name`);
   * the same pack with the same content: nothing to do;
   * an older / different decebal pack: needs `--force`. The policy (foreign, pack identity, `--force`, unchanged,
     modified) is decided again from a fresh observation right before committing. `RENAME_EXCHANGE` swaps the folders
     atomically and the swapped-out folder must be the pack that was observed (otherwise it is swapped back and nothing
     is replaced). Without `renameat2` the old folder is renamed aside and the new one in; if that fails the old one is
     renamed back, and if even that fails it is moved to `<root>/.decebal-recovery/` (never cleaned up automatically),
     or the job folder is kept: an error path never deletes a previous version.
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
* **Caps** (defaults): 120 000 entries (counted while folders are read), 16 MiB per file, 1 GiB extracted, 512 MiB
  archive, 1 MiB per licence file, 64 MiB / 4096 images of generated cursors.
* **Ownership**: only folders carrying `.decebal-asset.json` are ever replaced or removed. `remove` decides under the
  install lock (what is there, whether it is in use) and checks the folder it moved aside is the pack it observed.

## Selection and theme recommendations

Applying a theme applies its matching icons (and cursors, where it has some): **Follow theme** is the default for both
slots; **Custom** is the explicit override, made in Settings > Icons.

* The icon theme is `decebal.json` `icons.theme` (unchanged, `docs/icons.md`); the cursor theme is `system.cursorTheme`,
  which "Apply to all apps" writes to GTK `settings.ini`, gsettings and `Xcursor.theme` (unchanged).
* Who decides them is kept in `decebal.json` `asset_packs`:

```json
"asset_packs": {
  "icons":   {"mode": "follow", "theme": "nextkde", "applied": "breeze", "baseline": "Papirus", "restore": "Papirus"},
  "cursors": {"mode": "custom"},
  "recommended": {"theme": "nextkde", "icons": "breeze", "cursors": null, "missing": ["decebal-breeze-cursors"],
                  "icons_pack": "decebal-breeze", "cursors_pack": "decebal-breeze-cursors",
                  "basis": "coverage:upstream (KDE Plasma's own Breeze icons and cursors)"},
  "generation": 42,
  "downloads": "ask"
}
```

* **Follow** (default, also when there is no record): `theme apply` sets the slot to the theme's matching pack,
  remembering the previous value as `baseline` (put back when a later theme matches nothing). When the pack is not
  installed, the slot keeps its current value (no flash to something else) and the pack is downloaded, verified and
  applied in the background (below).
* **Custom**: the Icons pane's theme list (*Use this theme*), its *Custom* switch, `decthemectl assets use icons|cursors
  NAME|auto`, `decthemectl icons set` and a hand edit of the value (it no longer equals `applied`). Applying a theme never
  changes a custom slot; the report says what the theme would have applied. Choosing *Follow theme* again keeps the
  custom value as `restore`: *Restore X* in the pane (`assets use ... restore`) makes it custom again.
* A record written before slot modes existed (`{"user": true}`) is a manual choice and stays **custom**: no theme and no
  late background install overrides it; *Follow theme* in Icons hands it back to the theme (its value kept in `restore`).
* `generation` is bumped by every theme apply and every slot choice; `downloads: "ask"` (the pane's *Download matching
  packs* switch, `assets downloads auto|ask`) turns automatic downloads off: missing packs are then only offered with
  an *Install* button. The default is automatic: the user authorised downloading the matching packs of the themes they
  apply.
* **Matching pack**, first found wins: the port catalogue's `[[recommend]]` for the theme id; the theme's own
  `[icons] theme` when that theme is installed or a catalogue has a pack of that name; decebal's coverage table
  (`src/desktop_assets/coverage/themes.toml`). Every theme in `themes/` is covered (a test fails otherwise). Each
  coverage entry says its basis: `upstream` (the icons of the desktop the theme recreates: Adwaita for the GNOME themes,
  Breeze icons and cursors for nextkde), `manifest` (what the theme names, as a pinned pack) or `fallback` (no free
  matching set exists or is pinned: macOS, Windows 11 and Nothing OS icons are not redistributable, palettes such as Nord
  or Dracula publish no freedesktop icon theme), with its reason. Fallbacks keep their upstream name and licence
  (Papirus-Dark stays Papirus-Dark); nothing is relabelled as a theme's own icons. The coverage packs that no port
  provides (Adwaita, Breeze, Breeze cursors) are in `src/desktop_assets/coverage/packs.toml`, fetched with `git_url`;
  every pack a theme can apply by itself has a download. Without a cursor match the cursor slot is not changed.

**Background workflow.** After `theme apply` has written its files (Settings and the CLI alike), and after *Follow theme*
or *Retry* in the pane, `follow::request` starts at most one job per configuration and generation,
`decthemectl assets follow-theme --generation G --job TOKEN`: fixed arguments, no shell, its own process group,
detached from the caller, output to its own fresh 0600 log `job-<token>.log` (the newest five are kept). Its state is in
`$XDG_STATE_HOME/decebal/assets/<first 16 hex of sha256(canonical decebal.json path)>/` (0700):

* **Claim**: under that folder's `lock` (flock), `request` reuses a live job of the same generation (or one whose claim is
  younger than 30 s and has no worker recorded yet), otherwise stops the older job, writes a claim with a new unique
  token, starts the worker and records its PID and start time before releasing the lock. A concurrent caller waits and
  then sees the claim. A claim whose requester died before recording a worker expires after the 30 s.
* **Compare-and-set**: every progress, terminal, stop and watchdog write changes `follow.json` only while it still
  carries the writer's token; a worker whose token is no longer current stops working and writes nothing. An old job,
  its watchdog or an old stop therefore never overwrites the current job's status. A non-terminal job whose worker is
  gone is shown as `interrupted`.
* **Stop** (pane *Stop*, `assets follow-theme --stop`, and a newer claim) targets only the current job: its process is
  pinned with a pidfd, the pinned process's start time checked, and SIGTERM sent through the pidfd to the worker itself
  (never a bare PID or process group; no pidfd support means no signal). The worker cancels its install cooperatively
  (the install takes its normal error path and removes its staging folder) and records `stopped`.
* **Apply**: the job installs each missing pack once (the verified installer: pins, licences, nothing from the pack run,
  atomic publication), then applies in **one** `json_update_doc` transaction only if `generation` is still `G`, the
  recommendation is still that theme's, the **selected theme** (`theme.name`, the exact theme id) is still that theme, and
  the slot still follows it with the value it had; otherwise it records `superseded` and changes nothing, so a completion
  can never override a later theme, a direct edit of the theme or a manual choice. `generation` is external input: a
  value that is not a u64 counts as 0, and the maximum wraps to 1 (only equality is compared).
* **Deadline**: 30 minutes, then the job is cancelled like a stop; one that has not ended a minute later is ended and
  marked failed (only while it is still the current job).

After applying, the job hands the change to app integration through one routing point,
`desktop_assets::publish::selection_changed` (the GTK lane owns the toolkit side); see the limits below: until the
narrow icon/cursor publication is finished and proven with native apps, GTK and Qt apps follow only through "Apply to
all apps" (on by default; an explicit `system.enabled = false` stays off). The shell itself uses the new icons at once.
`follow.json` holds the phase (`starting`, `installing`, `stopping`; `applied`, `superseded`, `failed`, `stopped`), the
pack being installed, the error and the log path. The pane shows it with *Stop* and *Retry*; `assets follow-theme
--status` and `assets status` print it. A failure keeps the current icons and names `--archive` / `--source-dir` as the
way that still works.

* Wallpaper and widget policies are not touched.

**Icons pane** (System Settings -> Icons): besides the icon themes, a *Cursor theme* menu (installed cursor themes,
independent of the icon theme; choosing one makes the cursor custom), *Theme matching* with *Follow theme* / *Custom*
for icons and for the cursor (and *Restore X* while following), *Download matching packs*, the background download with
*Stop* / *Retry* and its outcome, an *Install* button for each matching pack that is missing when downloads are set to
ask (it runs `decthemectl assets install ID` in the background, never through a shell, and shows the outcome), and the
installed packs with licence, source and revision.

## Installed provenance

`<theme>/.decebal-asset.json` (hidden, ignored by toolkits):

```json
{
  "format": 1, "pack": "deepin-bloom", "name": "Bloom", "provides": ["icons", "cursors"], "install_name": "bloom",
  "family": "deepin", "origin": "catalog",
  "source": {"id": "deepin-icon-theme", "repo": "https://github.com/linuxdeepin/deepin-icon-theme",
             "revision": "master", "commit": "5d11...", "path": "bloom", "tree": "3c8e...",
             "archive_sha256": "...", "fetched_from": "archive|download|directory|git", "url": "...",
             "git": {"commit_chain": true, "objects": 862, "requests": [{"what": "...", "bytes": 0, "sha256": "..."}]}},
  "verified": {"tree": true, "archive_sha256": false, "commit": true,
               "outside": [{"what": "link_sources", "verified": true, "by": "tree"},
                           {"what": "licences", "verified": true, "by": "blob"}],
               "complete": true},
  "link_sources": [["Papirus", "56b5312e..."]], "from_link_sources": [48961, 127583232, 43507],
  "icon_dirs": [210, 210], "degraded": false,
  "license": "GPL-3.0-or-later", "license_files": ["LICENSES/GPL-3.0-or-later.txt"],
  "content_hash": "sha256:...", "files": 9512, "symlinks": 7542, "skipped": 12,
  "installed_by": "decebal 0.2.1"
}
```

`verified.tree` covers only the pack's own folder. Everything the pack took from outside it is listed in
`verified.outside` with what checked it (`tree`: `link_source_trees`; `blob`: `index_oids` / `license_oids`;
`archive_sha256`: a pinned archive; `none`), and `verified.complete` is true only when every installed byte came from
verified input. `commit` compares a git archive's pax comment with the pin: a consistency check, not authentication;
for a git fetch it means every pin was found below the pinned commit's tree. The install report prints the same as one line (`outside the pack folder: ...; complete: yes|NO`).

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
* the port owners' own catalogues (Plasma, GNOME, Lingmo) loaded together and validated; six packs downloaded over
  HTTPS from their `archive_url`s with `tree`, `archive_sha256` and `commit` verified (Layan white cursors, Lingmo,
  Papirus, Papirus-Dark, Breeze Light; Breeze cursors from a local `git archive` whose bytes equal the pinned sha256).
* Papirus-Dark (folder links into `../Papirus`), from the GNOME owner's cached archive (sha256 = their pin) into a new
  private root: 210 of 210 directories (12 before link-source expansion), 0 links leaving the pack, `complete: yes`
  with `link_source_trees`, `verify` ok, 16 of 16 sample icons resolved.
* Deepin nirvana fetched with `git_url` (no clone, no archive) from github.com at the pinned a0c79e3 / tree fb56b39d,
  with the proposed catalogue data plus `git_url`, by the real `decthemectl` in a private HOME and root, three times
  (2026-10-31): 3 requests each, 20.9 MB in all (capabilities 191 B, the commit's 131 trees ~435 KB, the pinned
  objects ~20.47 MB: 862 objects, 1 tree, 859 blobs, 2 deltas); every pin found below the commit; `tree yes`,
  `commit yes`, licences verified by blob, `complete: yes`; the same 1 435 of 1 439 icons and the same 4 refusals as
  the clone; content hash `0714a442...` **identical to the `--source-dir` installs**; `verify` ok; no external tool run.
  Pack bytes differ by a few hundred bytes between runs (the forge recompresses), the content does not. The fetch
  takes about 2 s; the rest of the ~4 min (debug build) is the DCI conversion. Refused with nothing published and no
  staging left: a tree pin the commit does not have (after 2 requests, no content fetched), an unknown commit
  (`not our ref`), a missing repository (HTTP 401) and a host that is no git server (HTTP 404). The full repository
  is ~976 MiB packed.

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
* Licence files outside the pack's `path` are verified only by `license_oids` or a pinned `archive_sha256`; without
  either the install says `licences NOT pinned; complete: NO`.
* Expanding link sources copies their files (Papirus-Dark: ~49 000 files, ~125 MiB besides its own): self-contained
  by design, at the cost of disk space.
