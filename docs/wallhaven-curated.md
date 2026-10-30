# Curated Wallhaven recommendations — 2026-10-30

This is an advisory catalogue for the parent's download/cache/selection work, not
a live configuration change. [`../data/wallhaven-curated.json`](../data/wallhaven-curated.json)
contains **21 theme IDs / 42 distinct images / 42 dark-light choices**. Every
non-legacy package installed by `make install` is covered (including the five
showcase folders; `everforest` is a palette alias and excluded). Aster is the one
new package. No uncovered IDs at this checkpoint. Existing generated-art packages
are deliberately unchanged here; the parent owns removing their defaults and
wiring these selections into runtime. This catalogue alone does not remove them.

## Selection and provenance

Fetched the actual public SFW API, not guessed IDs or search-engine thumbnails:
[API reference](https://wallhaven.cc/help/api), detail API for each selected ID,
[AI-art tag 133451](https://wallhaven.cc/api/v1/tag/133451). Nine bounded search
pages (at most 216 results, with duplicates), provider large thumbnails and 45
selected/rejected detail lookups were enough; no library mirroring. Search
requests used General / SFW / landscape / at least 2560×1440, photography and
subject terms, and `-ai_art`. Exact requests are stored in `policy.search_urls`.
The current API help does **not** document `ai_art_filter`; `0` was sent only as
an advisory parameter, never treated as an enforced filter. The authoritative
curation post-filter checks each actual detail response: SFW, no AI-art tag
133451 or named AI-generator tags. Selected candidates tagged digital art,
illustration or space art were also rejected. Current Decebal provider query
code has no AI-exclusion preference yet; the parent must enforce the detail
filter again when downloading, since classification can change.

All chosen large thumbnails were visually inspected in subject contact sheets;
each choice has an individual rationale and placement/crop note. They have
photographic appearance. Missing tags and visual inspection cannot certify the
capture method, and this is not a claim of verified non-AI authorship. Do not
silently substitute an unreviewed result if an ID disappears or becomes flagged.
The photographed sky/water/forest variety was chosen over illustrative results
returned even by photography searches.

The JSON `images` map holds page/full/thumbnail/API URLs, dimensions, exact detail
tags and hashes of the inspected detail/thumbnail bytes. `themes.<id>.dark/light`
references that map. **Uploader is not author**: uploader usernames are recorded
as uploader only, `is_author` and verified author name remain null. Some detail
tags name a photographer; these are explicitly unverified `reported_candidate`
values, not promoted to author credits. API source strings are preserved as
reported source links; missing sources are null. Attempts to open some linked
Unsplash/Flickr originals were blocked, so their author/license assertions were
not inferred. Unknown author/license is not replaced with an uploader or a
blanket Unsplash/NASA/public-domain claim.

The API provides no per-image license. All 42 license identifiers remain null:
**no redistribution permission established**. Wallhaven states images remain the
property of their original owners. No full image, provider thumbnail, derived
photo screenshot or contact sheet is committed. Cached thumbnails, raw metadata,
contact sheets and private-photo evidence are under `/tmp/decebal-wallhaven-curation`.
Preserve any photographer watermark. The MIT license covers Aster's package
configuration/docs, not the linked photography.

## Theme map

All pairs are distinct; none needs a shared image in both modes in this batch.
“Dark/light” is the intended UI pairing, not an exposure classification. Light
UI remains legible over moody photographs through its actual backed surfaces.
More detailed sky/subject/edge notes are in each JSON record.

| Theme ID | Dark Wallhaven | Light Wallhaven | Design fit |
| --- | --- | --- | --- |
| `aster` | [9mqvz1](https://wallhaven.cc/w/9mqvz1) | [wex83x](https://wallhaven.cc/w/wex83x) | Quiet coastal photography complements slate/porcelain surfaces and small Breeze captions. |
| `brutalist` | [zx9z8j](https://wallhaven.cc/w/zx9z8j) | [d6o77l](https://wallhaven.cc/w/d6o77l) | Strong photographic diagonals match hard edges; no decorative vector geometry is needed. |
| `caelestia` | [p82qrj](https://wallhaven.cc/w/p82qrj) | [jxyowm](https://wallhaven.cc/w/jxyowm) | Photographic green and rose tones give Material You a natural source when explicitly enabled. |
| `frutiger-aero` | [yq8x8d](https://wallhaven.cc/w/yq8x8d) | [7j6wpy](https://wallhaven.cc/w/7j6wpy) | Photographic water and clouds retain optimistic aqua without generated bubbles or hills. |
| `glyph-rice` | [dpr913](https://wallhaven.cc/w/dpr913) | [kxygyd](https://wallhaven.cc/w/kxygyd) | Low-key city texture / white winter negative space keep the dot typography dominant. |
| `gnome-adwaita-like` | [3l6jj9](https://wallhaven.cc/w/3l6jj9) | [qzgymr](https://wallhaven.cc/w/qzgymr) | Cloud and mountain photography gives clean Adwaita-like surfaces breathing room. |
| `gruvbox-dark` | [9dpejk](https://wallhaven.cc/w/9dpejk) | [po73qj](https://wallhaven.cc/w/po73qj) | Copper foliage and orange sandstone complement warm cream/charcoal rather than drawing contour lines. |
| `kiln-court` | [w5eqmp](https://wallhaven.cc/w/w5eqmp) | [85xg5k](https://wallhaven.cc/w/85xg5k) | Earth pigment comes from real sand and architecture rather than rendered vaults. |
| `macos` | [l3edjp](https://wallhaven.cc/w/l3edjp) | [6de6wl](https://wallhaven.cc/w/6de6wl) | Natural alpine photography replaces synthetic curves without imitating an Apple asset. |
| `moss-atlas` | [vqxkr5](https://wallhaven.cc/w/vqxkr5) | [768yeo](https://wallhaven.cc/w/768yeo) | Actual woodland depth suits the organic right-hand instrument rail. |
| `mountain-neon-dot` | [ey7prw](https://wallhaven.cc/w/ey7prw) | [nr7zq0](https://wallhaven.cc/w/nr7zq0) | Faithful snow-mountain photography with the reference's teal night atmosphere; no AI mountain illustration. |
| `nextkde` | [57o619](https://wallhaven.cc/w/57o619) | [vmxq3m](https://wallhaven.cc/w/vmxq3m) | Soft blue reflections fit the broad glass surfaces and top-panel / launch-dock composition. |
| `nothing-dot` | [p9qovj](https://wallhaven.cc/w/p9qovj) | [7p186o](https://wallhaven.cc/w/7p186o) | Real monochrome lighting and landscape complement the single red interface accent. |
| `orbit-glass` | [5y3q31](https://wallhaven.cc/w/5y3q31) | [lygg6r](https://wallhaven.cc/w/lygg6r) | Use real astronomy/space photography rather than a synthetic eclipse. Right rail only. |
| `paper-signal` | [eyv7xl](https://wallhaven.cc/w/eyv7xl) | [x83e2z](https://wallhaven.cc/w/x83e2z) | Monochrome still life and snow keep the editorial palette in charge. |
| `signal-deck` | [xl19lv](https://wallhaven.cc/w/xl19lv) | [ymm56g](https://wallhaven.cc/w/ymm56g) | Real orbital weather observations suit the scientific instrument layout without invented survey artwork. |
| `sweeteh` | [yxvodx](https://wallhaven.cc/w/yxvodx) | [21y32m](https://wallhaven.cc/w/21y32m) | Real sky photography suits raised Material tiles and explicit native island surfaces. |
| `synthwave` | [3qq8jd](https://wallhaven.cc/w/3qq8jd) | [39zjq6](https://wallhaven.cc/w/39zjq6) | Actual night lights and sunset colour replace the procedural sun/grid; no uncredited cyberpunk illustration. |
| `terminal-minimal` | [o571qp](https://wallhaven.cc/w/o571qp) | [21ooly](https://wallhaven.cc/w/21ooly) | Large quiet tonal fields respect a tiny bar and bare desktop clock. |
| `tidal-loom` | [m9wr19](https://wallhaven.cc/w/m9wr19) | [3qqd26](https://wallhaven.cc/w/3qqd26) | Natural repeated forms replace procedural weaving while keeping the cool material rhythm. |
| `windows11` | [7p6wke](https://wallhaven.cc/w/7p6wke) | [yqxzqx](https://wallhaven.cc/w/yqxzqx) | Wide natural horizons feel familiar behind a single centred taskbar. |

## Readability and runtime handoff

Center-fill keeps image aspect; it necessarily crops a different monitor aspect.
The choices were assessed at landscape desktop aspect from actual thumbnails.
Ultrawide/portrait and multiple monitors remain unverified; keep the subject and
retarget cropping per display. Neither this catalogue nor Aster paints UI into
photography. In particular, Glyph's dot clock, Mountain's transparent clock and
the bare terminal clock need a final full-resolution contrast check; other
instrument cards retain their own backing. Do not add opaque fake widgets to
wallpaper files to solve contrast.

Parent integration should recheck purity/AI tags on the detail response, honour
user-chosen Wallpaper settings separately, download/cache only after explicit
selection, and report offline/deleted/reclassified results honestly. Keep exact
source links and the uploader/author/license distinction visible. Do not imply
that a cache is a redistribution license, nor use generated fallback artwork.
Existing pure theme background colours may show while no image is selected;
that state must be labelled rather than sold as an installed photo default.

## Rejected approaches / remaining gaps

- `54l6l4` macro: looked photographic but also tagged digital art; replaced for
  Glyph with the inspected monochrome aerial-city photo `dpr913`.
- `lmzpzp` Moon: tagged space art/digital art; replaced for Orbit with a sourced
  Milky Way photograph `5y3q31`.
- `j59zrp` neon street: tagged concept/digital art; replaced for Synthwave with
  the inspected neon building photo `3qq8jd`. Other illustrated/3D-looking search
  results were not selected. No guessed upstream artist identity.
- Parent `f68d460` and persistent-index `837799f` were read **read-only**. No engine,
  Settings, widget creator, Icons or rdwm source was copied over or changed.
- Catalogue installation path, runtime selection/download/cache, old generated
  default removal and compositor shadow profile remain parent's work.
- Full-resolution contrast/crop proof is limited to Aster's two private QA photos;
  the other 40 are thumbnail-curated recommendations awaiting runtime QA.
- No new font/cursor/icon assets, live installation, application restarts or
  theme application. Existing Keep / Replace / Cancel remains tested.
