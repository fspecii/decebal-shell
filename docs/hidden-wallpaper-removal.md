# Hidden compatibility wallpaper retirement

Follow-up to accepted `401ebc932cd19c1268787746f75f8283b944f35e`, scoped only to
hidden legacy compatibility packages still copied by `make install`.

| Package | Removed wallpaper | Preserved compatibility |
| --- | --- | --- |
| caelestia-mocha | mocha.jpg gradient | Mocha/Latte tables and left pill structure |
| everforest | forest.png geometric forest | Dark/light colour tables and rounded layout |
| kanagawa | wave.jpg drawn waves | Wave/Lotus colours, serif captions and right rail |
| material-you-dynamic | aurora.jpg gradient | Exact fixed colour tables, fonts and bottom pill layout |
| nothing-light | dots-light.png dot texture | Always-light tables, native ring clock/battery |
| rose-pine | dusk.jpg geometric hills | Dusk/Dawn colours and top pill structure |
| terminal-amber | phosphor.png scanlines | Exact amber/cream tables and Terminal Minimal inheritance |
| tokyo-night | neon-grid.jpg drawn skyline | Night/Day colours and split-bar structure |

Removed **8 wallpapers, 8 package screenshots and 14 documentation shots** that
embedded those generated images: 2,654,988 bytes total. The actual raster files
were visually inspected; prior theme documentation also identifies them as
procedurally generated, with no attributed third-party photography. No art-only
generator remained for these eight packages; native evidence/render tools are
useful UI tooling and are left untouched. No new replacement screenshots or
wallpaper assets were generated.

The entire `[wallpaper]` section and screenshot metadata are removed. In particular,
Material You no longer silently opts into wallpaper-derived recolouring or its
old scheme/strength/follow-mode defaults. Its fixed compatibility palette is
unchanged; opt into Match wallpaper separately after choosing a photo. Terminal
Amber retains `extends = "terminal-minimal"`. No palette file, caption geometry,
font chain, bar/dock field or widget source changed.

`generated-wallpaper-removal.json` now covers **25 modified packages** across the
two batches. Each new alias records its own `401ebc9` baseline, exact removed-file
SHA256/size, canonical preserved-manifest hash and widget-file hashes. Existing
records for the visible-package batch are unchanged. All **34 shipped package
manifests** now omit wallpaper metadata. Only Sweeteh's two previously attributed,
byte-identical upstream Unsplash photographs remain as optional bundled images;
source links, licenses and attribution are retained unchanged in this follow-up.
Examples and unrelated tiny test image/solid fixtures are unchanged.

No aliases were promoted to visible themes. The 21-ID / 42-choice catalogue is
unchanged; no searches, new image IDs, network calls or photo downloads occurred.
Installed/live assets and user-selected wallpaper files are not deleted. Parent
owns managed-install cleanup, runtime picker/cache and updates to legacy canonical
output snapshots that previously expected automatic wallpaper/extraction.

## Focused validation

- `python3 tests/wallpaper-package-contract.py`: four offline audit tests verify
  exact structural/palette preservation, retired-file absence, all 34 art-free
  manifests and retained Sweeteh photo hashes/credits.
- `cargo test --locked --offline --test hidden_wallpaper`: validates/reproducibly
  packs all 8 aliases; dark/light plans preserve independent image/mode/background,
  custom widgets and Wallhaven selection without writing source configuration.
- Targeted Clippy, formatting and staged whitespace checks.

No runtime `src` file, core golden test, parent GUI/native control, preview worker,
renderer/helper, install/config, process, push, checkout or agent change.
