# Icon packs and cursors: evidence and findings

Icon and cursor artwork is GPL-3 / CC-BY-SA-3.0 upstream material, so previews that show it are **not committed**; they live in
`~/.cache/decebal-openkylin-evidence` and are reproducible with the commands below.

**Installing and building is not done in this lane.** The packs are described by `assets/desktop-ports/openkylin/asset-packs.toml` and installed by the shared,
reviewed installer (`decthemectl assets`, `src/desktop_assets`, docs: `docs/desktop-asset-packs.md`): pinned tree ids, bounded archive reader, native Xcursor
builder, atomic install, provenance record. The lane-private fetcher/builder this port first shipped was removed. Details and findings: `integration-contract.md` §2.

## Shared installer run (private root, cleared environment, no display or bus)

```sh
env -i PATH=/usr/bin:/bin HOME=$D/home XDG_CONFIG_HOME=$D/home XDG_DATA_HOME=$D/home XDG_CACHE_HOME=$D/cache DECEBAL_CONFIG=$D/cfg \
  DECEBAL_ASSET_ROOT=$D/root DECEBAL_ASSET_CATALOGS=assets/desktop-ports/openkylin DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent \
  decthemectl assets install openkylin-ukui-default --archive openkylin-theme.tar.gz --strip 1     # likewise the other five
```

Result with the asset lane's `decthemectl` (sha256 `ca4646c2e5dc…`, built 2026-10-31 07:23): all six packs installed, `tree yes`, `commit yes`; `assets verify` →
all `ok`; dark-sense: 57 cursors built natively + 61 links; `assets preview` drew 25 (icons) / 10 (cursors) samples from the installed files.

| preview (private) | sha256 | size |
|---|---|---|
| `DMZ-White.png` | `4d2e10906b0a8dd4…` | 7 KiB |
| `blue-crystal.png` | `7b5562de5c5f42df…` | 7 KiB |
| `cursor-sheet.png` | `8b1a74ace946d342…` | 160 KiB |
| `dark-sense.png` | `f7349c7d4fd3d3be…` | 9 KiB |
| `ukui-icon-theme-classical.png` | `d0b5a132d292fa49…` | 30 KiB |
| `ukui-icon-theme-default.png` | `63b8ea5b38a6c87b…` | 29 KiB |
| `ukui-icon-theme-fashion.png` | `2519cf3613a2c920…` | 24 KiB |

## Native Settings → Icons pane

`decsettings icons` (0.3.0 panel build) with `icons.theme` set to each pack, 1400×900, 100 % and 200 %. The 16-icon preview grid is drawn by Decebal's own
icon lookup (`src/icons`), so it proves the fetched pack is a valid, resolvable freedesktop theme: the three packs render as three clearly different
styles (default = flat rounded, fashion = flat colour, classical = skeuomorphic). The pane lists the installed packs next to Adwaita and the
others and keeps **Use this theme** per pack: choosing an openKylin theme never changes this choice.

| file | sha256 | size |
|---|---|---|
| `icons/settings-icons-ukui-icon-theme-classical-1.png` | `c1db6f037ad208bf…` | 68 KiB |\n| `icons/settings-icons-ukui-icon-theme-classical-2.png` | `c1db6f037ad208bf…` | 68 KiB |\n| `icons/settings-icons-ukui-icon-theme-default-1.png` | `f28fb4d13b18429d…` | 67 KiB |\n| `icons/settings-icons-ukui-icon-theme-default-2.png` | `f28fb4d13b18429d…` | 67 KiB |\n| `icons/settings-icons-ukui-icon-theme-fashion-1.png` | `4312c1d2eaead611…` | 61 KiB |\n| `icons/settings-icons-ukui-icon-theme-fashion-2.png` | `4312c1d2eaead611…` | 61 KiB |\n| `cursors.png` | `72869cdebcdff7b3…` | 152 KiB |

Reproduce: install the packs with the shared installer (below), then start `decsettings icons` on a private Xvfb (an explicit display number >= 700 verified as yours in both socket namespaces; never `:0`) with `XDG_DATA_HOME` pointing at the private root's parent and `{"icons":{"theme":"ukui-icon-theme-fashion"}}` in a private `DECEBAL_CONFIG`.

## Cursors

Hotspots: the natively built `dark-sense/left_ptr` reads back as 4,0 at 24 px / 9,3 at 48 px (the table in the catalogue), checked with the bounded evidence reader
(`tools/xcursor_read.py`, read-only, hard limits; also behind `tools/cursor_sheet.py`, which renders `dark-sense` and `DMZ-White` at nominal 24 (100 %),
32 (125 %) and 48 (200 %) on light and dark panels with hotspots marked).

Findings:
* None of the three upstream cursor themes ships CSS-spec names. The catalogue adds 25 as `aliases` for dark-sense/blue-crystal (additive). **`DMZ-White` gets none**:
  the catalogue format has no alias hook for prebuilt cursor packs (installer gap).
* `dark-sense/grabbing` is drawn as an arrow upstream (defect, not patched); `grab` is aliased to `hand2`.
* Animated cursors carry 5 (`left_ptr_watch`) or 10 (`watch`) frames at 50 ms in each of the sizes 24/32/48/64 (96 where shipped).
* Applying an openKylin theme does not write a cursor key (`tests/e2e/openkylin-preserve.py`); the Icons pane and `assets use` keep the choice independent.

## Icon lookup gaps seen

* The Settings pane's "Mail" tile (it looks up `mail-client`) falls back with all three packs: the default pack's only mail icon is `claws-mail` (upstream naming gap).
* Classical and Fashion inherit `ukui-icon-theme-default`; every pack inherits `gnome` (not shipped): installing one pack alone falls back to hicolor/Adwaita.
* `ukui-icon-theme-default` lists 128 directories in `index.theme`; 56 are empty upstream (`.gitxxx` placeholders), so the installer warns.
