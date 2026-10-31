# Lingmo port — native evidence and remaining gaps (2026-10-31)

**Status: NOT READY.** This is what was actually rendered and measured, with exact tools, and what was not.

## Tools (all read-only, nothing live touched)

| What | Exact |
| --- | --- |
| shared engine | commit `b28cca5fb2e81b84d2cfcd65e9fa978391041e50` (`feat/claude-universal-panel-applets`, 2026-10-31 03:24 +02:00, "Draw a neutral logo when a theme's own logo cannot be had"). Binaries built by the Plasma port owner from a committed-tree archive of that commit; this port did **not** rebuild them and cannot independently prove the binaries match the archive. |
| `decthemectl` | `~/.cache/decebal-plasma-ports/target-b28cca5/debug/decthemectl`, sha256 `37906080cca08cd89826315ca4307ecce720e0323ee712ad7e6bc2149defe409` |
| `decbar` | same dir, sha256 `7972a7fd8af96053fc3a67ea1f2c0a6bc0c0a862cbc8a91d1c97bbf16f7f16ba` |
| `decdock` | same dir, sha256 `85cb4ffd57f9eca7f50d080d8a0bf1996d865c69404e208d0ed60034db4a9192` |
| scale engine | commit `aa0580a69ce48a65b069ee81ff7a8075c88eb44c` ("Draw every bar renderer at the display scale on X11 (100/125/200%)", 2026-10-31 04:27 +02:00), committed tree only (`git archive`), built **by this port** with `flock --close …/slot1.lock nice -n 10 cargo build -j2 --bin decthemectl --bin decbar --bin decdock` into `~/.cache/decebal-lingmo-target/shared-aa0580a-target` (private HOME, display/session variables cleared); source hash before and after: `Cargo.lock` sha256 `ca646e7ee7975f920e7196dc1ed56c06cdc7da7887fc434d1f656dd14561e4df`, `src/` tree digest `90d87a0e202e0b401e201cbf31f61cb4185ab47daea23dfc287aa3ecd32d53da`. Binaries: `decthemectl` `b9ed70108d647b77c9b0a3a9ddb1a0be159d4b49f7e66eeb252b9387029fcb80`, `decbar` `3535063c4d1a40f3710047f516984062db9611f36a0c76e102523a374a0ce6fa`, `decdock` `bb1e57e455e495a31f8fc659e1afc1f7c6b0b2e55b5912b16f840edbbe0a2f36`. |
| first-pass engine | `…/target-shared/debug/decthemectl` (commit `71e3434`, sha256 prefix `9b90d626e2c5f9e3`): validated the package; its preview still draws an Apple glyph for an unresolvable `icon:` logo, which `b28cca5` fixes. |
| environment | `assets/desktop-ports/lingmo/tools/*.sh`: `env -i`, private `HOME`/`XDG_*`/`DECEBAL_CONFIG`/`TMPDIR`, private Xvfb display with its own `dbus-run-session`, no window manager, no compositor. |
| tests | `tests/lingmo_desktop_port.rs`, 18/18 integration and 130 of 131 lib tests (the lib slice: `decoration_compat`, `settings::{decorations,io,schema,appearance}`, `theme::engine`) passing on repository HEAD `8e5b44f` (digest of `src tests themes Cargo.*` in `build14-inputs.txt`, re-verified after the run); the one lib failure is the historical `theme::engine::golden::every_builtin_resolves_to_the_same_theme_as_before` (31 builtins versus 29 expected since root `98148e3` added two GNOME packages); `golden.rs` and `builtin.rs` are unchanged from `98148e3`, and the same test is run on an untouched `98148e3` export below

## Display harness rule (added 2026-10-31, after a rollout X11 transport incident)

`tools/gui-capture.sh` now starts its private Xvfb only on an explicitly reserved **high display number (700 or more)**, refuses `:0` and any lower number, and refuses a number whose
filesystem socket, lock file or abstract socket already exists. After the start it proves that its own process is the server for exactly that display and holds the inode of **both**
socket namespaces (`/tmp/.X11-unix/X<N>` and `@/tmp/.X11-unix/X<N>`) before any client runs. It never probes with `-displayfd`, never unlinks a socket or lock, and waits for its own
server to exit so that it leaves nothing behind (re-checked at `:712`: no filesystem socket, abstract socket or lock afterwards). Verified with refusals for 0, 99, 699 and a
non-number, and with a busy `:711` held by a throwaway server of this port that was then stopped by its own PID.

All captures committed before this rule used explicit display numbers below 700 (never `:0`, never auto-probed) on servers this port started and stopped; they are preserved as evidence.
One pre-fix run at `:710` left its own stale filesystem socket `/tmp/.X11-unix/X710` (no abstract socket, no lock, no listener); it was left in place, not deleted, and `:710` is not reused.

## Captured

* `native/preview/`: `decthemectl theme render lingmo` (the native preview painter) of the pinned `aa0580a` build, light and dark, with **both** the bar and the dock on each of the four edges
  (the edge matrix script rewrites only `[bar]`/`[dock]` `position` in a private copy). Fixed sample data, 1920×1200.
* `native/real-gui/`: the real `decbar` and `decdock` processes under a private Xvfb over a **flat** backdrop (`#1b3a5c` dark, `#b8d4e9` light; a test colour,
  not a wallpaper): top bar + bottom dock, 1366×600 short windows, and user-edge overrides set through `decthemectl panel`
  (bar left / dock right, bar right / dock left, bar bottom / dock top). Hashes in `SHA256SUMS`.
* Private (not committed, because the Wallhaven photographs carry no redistribution permission): the same bar and dock over the two chosen photographs
  (`l8pe5r` dark, `mldor9` light). sha256: dark 1920×1080 `a74fa0d6a880dd6cb2f54e7a3576a6c1f4fe90b50ac6e09c04254a1569626e8d`,
  light `9be4a01c17a9a4fca1018cb8385fd727ab8afa9eb18b229a2ab004b344313eb2`, dark short `462b46421c753f221cdfe3355d411f706c9e853b6cb55874917fa14cfef83272`,
  light short `cfef9acad90b0b9a233d2113bb3d9ae401442e28e1148b89b9dd8b17b215310e`. Both photographs were inspected at 1:1.
* `native/composited/`: the same real `decbar` + `decdock` with a private **picom** (XRender backend, `contrib/picom/decebal-glass.conf`'s scoped `_DECEBAL_BLUR` rule with a Gaussian
  blur; no GLX) and the backdrop as a real window beneath the bars, so `bar_alpha = 0.55` and the dock tint blend with what is behind them. Flat backdrops are committed; the photo versions
  are private (sha256: dark `6e96a32bb93cae5c2f169a2d71c50dc9541b8061372fedff7cd050892aac24dc`, light `4dae8d5ad08d7a5d69d9d45dbab01ab724e0c46cda2489518d24e45b54e020ce`, dark short
  `cc3c06f726be7bb47c18f4b5d8b188797f04b428970eb4fe0613f0512bee7f59`, light short `d826bec56d5d391694269ef61bc60e3c0b906bc136b2ea6e64d79a59736ca07f`). Without the compositor the
  engine draws the bar over black (gap 2 below); with it the light bar over the Moraine Lake sky is `#e5edf6` and the dark bar over the night sky `#112c47`.
* `native/scale/`: the real `decbar` and `decdock` of `aa0580a` under the private picom at `DECEBAL_SCALE` 1, 1.25 and 2 with `Xft.dpi` 96, 120 and 192, flat backdrops, dark and light, 1920×1080.
  Measured on the dark captures: the flat bar is 26, 33 and 52 px tall (ratio 1 : 1.27 : 2.0) and its text scales with it, so **the flat bar passes at 100%, 125% and 200%**.
  The mac dock is identical at all three scales (same pixels, same extent), so **the dock does not scale in this build**; no 125%/200% claim is made for the dock.
* `native/icon-pack/`: the real Lingmo icon pack installed (into a throwaway directory) by `reference-tools/verify-icon-recipe.py` (the `reference-recipes.toml` recipe: tree hashes verified, 14,104 text links converted to symlinks,
  33 dangling dropped) into a private icon directory and selected with `decthemectl icons set Lingmo`: the bar logo is Lingmo's own multicolour mark through `[branding] logo = "icon:systemlogo"`
  and the dock's trash is Lingmo's. The launcher tile is the engine's built-in Launchpad drawing; file-manager, browser and terminal icons come from the host's desktop files and the pack's
  fallbacks, so they are not Lingmo's application icons.
* Wallpaper legibility: [wallhaven-lingmo-records.json](wallhaven-lingmo-records.json) `bar_legibility`: the package's bar ink on the 32 px bar at 55% over every
  40 px cell of the centre-filled image, four edges, four viewports: minimum contrast over all edges and viewports 5.50:1 (light, `mldor9`) and 9.08:1 (dark, `l8pe5r`).

## What the captures show

* the flat bar draws on all four edges with the real applet set (menu strip, status icons, clock); the mac dock draws on top, left, right and bottom, dock radius and
  54 px icons as configured; the bar and the dock stand off each other when they share an edge;
* without the Lingmo icon pack the logo is the contract's neutral grid (specified fallback, not Lingmo's mark); with the pack installed and selected it is the real `systemlogo`.

## Gaps (measured, not hidden)

1. **Scale and bar height (updated, measured with this branch's own `decbar`/`decdock`, merged framework `101b29f` plus the bar-height contract):**
   * **Flat bar:** scales (`aa0580a`) and now draws the theme's declared `[bar] height`: the strip is **32 / 40 / 64 px** at 100 / 125 / 200 % on top **and** bottom, a side rail **40 / 50 / 80 px**
     (a rail is never thinner than 40; 32 would not fit its stacked cells), and 32 px on a 1366×600 output (`native/bar-height/`, `measurements.json`, `assets/desktop-ports/lingmo/tools/measure-strip.py`).
     The manifest's 32 is not just claimed: it equals `StatusBarWindow::kHeight` extracted from the pinned original by `reference-tools/extract-reference-numbers.py` (`native/reference-numbers.json`, commit `1cdaf0a`),
     and `tests/lingmo_desktop_port.rs` ties the two together.
   * **Mac dock:** still does **not** scale: 82 rows by 401 columns at 100, 125 and 200 % (measured). No 125 % / 200 % claim for the dock.
   * `theme render` still pins `DECEBAL_SCALE=1`.
   * Shipped themes whose declared flat or floating `[bar] height` was ignored until now and is now drawn: aster 28, brutalist 28, everforest 28 (floating), frutiger-aero 30, gnome-adwaita-like 28, gruvbox-dark 22,
     paper-signal 22, terminal-minimal 20 (raised to the 22 px text floor); nextkde, nothing-dot and nothing-light declare 26 (unchanged). Themes without a declaration keep 26 and the 40 px rail exactly.
2. **Compositing**: under bare Xvfb nothing composites and the engine draws `bar_alpha = 0.55` over black (light bar `#878789`, dark ink about 4.3:1, below 4.5:1). With the private
   picom run it blends with the wallpaper as intended (legibility record above). Still not exercised: the engine's native glass lens (`glass_refraction`), which samples the root pixmap
   (not set in these runs), GLX, and a hardware compositor; so no claim is made about the dock's refraction.
3. **App menu**: the flat bar draws the fixed placeholder menu strip ("Desktop File Edit View Window Help"). Lingmo's real bar shows the focused application's own menus
   through the AppMenu registrar; the shared engine has no consumer yet (see the Plasma port's branding contract).
4. **Per-surface glass** (dock lens 36, popups 20, bar 0 with a blur of sigma 20) and the **wallpaper-adaptive bar ink** are not expressible: `[metrics]` is global.
5. **Dock icon gap 16** has no theme key; the engine's spacing is a user dock setting.
6. **Window decoration**: nearest native style (`adwaita`). No native `lingmo` style exists; no decoration capture was made (the native helper was unavailable in the preview:
   "Decoration native helper unavailable"). The contract for the decorations owner is in README section 4.
7. **Tray, battery, network and volume icons** were absent in the private bus (nothing to show); battery/Wi-Fi/volume appeared in the `theme render` fixed-sample preview only.
8. **Launchpad**: not reproduced. The engine's launcher (`declaunch`, `launcher.style` default or dux) is a floating, centred, searchable card with a short icon grid; Lingmo's launcher is a
   full-screen paged grid over a blurred wallpaper (README section 2). A private capture of `declaunch` with the pack installed lists the host's installed applications, so it is not committed
   (sha256 `c3a20f477931bf56ec205b19cf60db2516f5f6776077fee72c72be3a58929053`). Needs a launcher style that pages a full-screen grid (launcher owner) before any claim.
   **Control centre**: rendered natively by `decbar --png … --open control` in `native/control-centre/` (card style, accent `#3385ff`, tile radius from the engine, not Lingmo's measured 16 px):
   the offscreen painter's neutral backdrop makes the dark panel look greyer than it would over a wallpaper, and the tile set (Wi-Fi, Focus, Night Shift, Record, Inhibit Idle, Game Mode, DND,
   Stay Awake, Screenshot, Notifications, Settings) is the engine's, not Lingmo's control centre.
9. Cursors and fonts are not installed in any capture; nothing about their appearance is claimed. The icon pack appears only in `native/icon-pack/`.

## Historical baseline failure, run on an untouched export (2026-10-31)

`theme::engine::golden::every_builtin_resolves_to_the_same_theme_as_before` fails with `left: 31, right: 29` (`BUILTIN` has 31 entries, the golden lists expect 29) **on a clean export of root
`98148e3a1454d3b5f1d09f9d947ff9d171610118` as well**: `git archive 98148e3` into a fresh directory (source digest of `src tests themes Cargo.*`
`b4024b3e05983797c13462a1b0573cca71ec9f179f3de3308ece23dbbf5da74e`, no Lingmo package in it), its own separate target directory, `cargo test --lib -- theme::engine::golden`
under `flock --close …/slot1.lock nice -n 10 cargo -j2` with the usual isolation: 5 passed, 1 failed, identical panic at `golden.rs:57`. The two ids missing from the golden lists are `gnome-adwaita` and
`gnome-catppuccin-dock`. This port did not change `golden.rs` or `builtin.rs` and did not touch the failure; it needs a root-side fix.

## Asset catalogue against the shared installer (2026-10-31)

`decthemectl` built from this branch at merge `07b732c` (`src/` identical to root `de2db55`), sha256 `a8e0fa40ab60253d1fef666c8ded14b1527d5f556327f62019a85a4ced16f82d`, private HOME, a fresh private
root, `DECEBAL_ASSET_CATALOGS` pointing at `assets/desktop-ports/lingmo/asset-packs.toml`, nothing downloaded and nothing installed (`--dry-run`, source = my local clone at the pinned commit):

* `assets catalog check`: **ok (1 sources, 1 packs, 1 recommendations)**. (An earlier revision was *rejected* by this validator: its description was 644 characters, the limit is 400.)
* `assets install lingmo-icons --source-dir <clone> --dry-run`: 10,397 files, 0 links, 32 icon directories; `verified: tree yes` (`19ae416…`); licence `GPL-3.0-or-later`;
  after pinning `license_oids` (`LICENSE` blob `e62ec04c…`): `outside the pack folder: licences verified (blob); complete: yes`.
* **DEGRADED, as disclosed:** only 32 of the 96 directories listed in `index.theme` exist. The other 64 are `@2x`/`@3x` directories that upstream creates as symlinks in its cmake script, which the installer never runs.
  The installer marks the pack DEGRADED (it refuses only below a quarter) and the catalogue says so.
* **Text aliases, not claimed:** `links/` is 14,137 regular git blobs (0 symlinks anywhere in `src/` or `links/`, checked with `git ls-tree`); the contract's `link_sources` / `link_source_trees` expand git symlinks only, so no
  `link_sources` is declared and the aliases are a disclosed fallback (alias-only names fall back to breeze / hicolor).
* The installer left only its own staging lock in the private root; no theme folder was created.

