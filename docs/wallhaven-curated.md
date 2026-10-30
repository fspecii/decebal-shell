# Curated Wallhaven recommendations — 2026-10-30

This catalogue supplies **Settings → Wallpaper → For this theme**. It does not
change live configuration by itself. [`../data/wallhaven-curated.json`](../data/wallhaven-curated.json)
contains **21 theme IDs / 42 distinct images / 42 dark-light choices**. Every
non-legacy package installed by `make install` is covered (including the five
showcase folders; `everforest` is a palette alias and excluded). Aster is the one
new package. No uncovered IDs at this checkpoint. Generated artwork/defaults are removed from visible and hidden packages; all 34 shipped manifests omit wallpaper metadata. Sweeteh’s attributed upstream photos remain optional assets. Separate picker/cache integration is implemented and tested; the catalogue never triggers downloads from theme apply.

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
Browse query has no general AI-exclusion preference. Curated downloads enforce fresh detail checks across every returned tag before accepting the image, since classification can change.

All chosen large thumbnails were visually inspected in subject contact sheets;
all 42 original full-resolution recommendations were subsequently downloaded,
decoded and reviewed at 16:9 with native-pixel detail strips.
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
| `glyph-rice` | [zpzqeg](https://wallhaven.cc/w/zpzqeg) | [kxygyd](https://wallhaven.cc/w/kxygyd) | Low-key photographic woodland / white winter negative space keep the dot typography dominant. |
| `gnome-adwaita-like` | [3l6jj9](https://wallhaven.cc/w/3l6jj9) | [qzgymr](https://wallhaven.cc/w/qzgymr) | Cloud and mountain photography gives clean Adwaita-like surfaces breathing room. |
| `gruvbox-dark` | [9dpejk](https://wallhaven.cc/w/9dpejk) | [po73qj](https://wallhaven.cc/w/po73qj) | Copper foliage and orange sandstone complement warm cream/charcoal rather than drawing contour lines. |
| `kiln-court` | [w5eqmp](https://wallhaven.cc/w/w5eqmp) | [85xg5k](https://wallhaven.cc/w/85xg5k) | Earth pigment comes from real sand and architecture rather than rendered vaults. |
| `macos` | [l3edjp](https://wallhaven.cc/w/l3edjp) | [6de6wl](https://wallhaven.cc/w/6de6wl) | Natural alpine photography replaces synthetic curves without imitating an Apple asset. |
| `moss-atlas` | [vqxkr5](https://wallhaven.cc/w/vqxkr5) | [768yeo](https://wallhaven.cc/w/768yeo) | Actual woodland depth suits the organic right-hand instrument rail. |
| `mountain-neon-dot` | [ey7prw](https://wallhaven.cc/w/ey7prw) | [2kqkmy](https://wallhaven.cc/w/2kqkmy) | Alpine night photography plus a legible white-snow light companion; no AI mountain illustration. |
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
The choices were assessed from the actual full-resolution files at 16:9, not
just thumbnails. Each JSON image record now includes full-file SHA256 and crop/
readability cautions.
Ultrawide/portrait and multiple monitors remain unverified; keep the subject and
retarget cropping per display. Neither this catalogue nor Aster paints UI into
photography. Native fixed-data QA also checked Aster, Glyph and Mountain using cached originals.
Mountain light `nr7zq0` failed because dark clouds obscured transparent homelab
text; it was replaced by the actual 2017 snowfield photo `2kqkmy`. The clock and
homelab are readable without moving or editing widgets. Glyph's busy dark-city pick was also replaced by `zpzqeg` after bright rooftop
edges competed with fine grey labels. The calmer native composition improves
legibility; fine text still needs display-specific checks.
Bare terminal clocks and user-moved widgets still need display-specific checks. Do not add opaque fake widgets to
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
  Glyph with the initially inspected aerial-city photo `dpr913`; full-resolution native
  follow-up subsequently preferred the calmer Swiss valley `zpzqeg`.
- `lmzpzp` Moon: tagged space art/digital art; replaced for Orbit with a sourced
  Milky Way photograph `5y3q31`.
- `j59zrp` neon street: tagged concept/digital art; replaced for Synthwave with
  the inspected neon building photo `3qq8jd`. Other illustrated/3D-looking search
  results were not selected. No guessed upstream artist identity.
- Parent `f68d460` and persistent-index `837799f` were read **read-only**. No engine,
  Settings, widget creator, Icons or rdwm source was copied over or changed.
- Runtime selection/download/cache and native X11 shadow integration are implemented. Generated-default retirement covers all 34 shipped manifests; see `generated-wallpaper-removal.md`, `hidden-wallpaper-removal.md` and `window-shadow-design.md`. Hardware gates remain separate.
- All42 current choices have full-resolution crop review. Native composition QA
  covers Aster, Glyph and Mountain, not all 21 live desktops. Physical scaling,
  ultrawide/portrait and arbitrary user widget positions remain unverified.
- No new font/cursor/icon assets, live installation, application restarts or
  theme application. Existing Keep / Replace / Cancel remains tested.

## Full-resolution follow-up and replacement

42 initial full files decoded successfully (about 199 MiB cached, at most 3 concurrent
downloads, 30 s/file, 32 MiB/file cap). No mirror, installer, live apply or download
loop. One bounded additional snow search returned 5 results; three detail responses
were inspected and two full candidates were fetched. `2kqkmy` replaced Mountain light. One additional detail/full candidate already
seen on the monochrome search page, `zpzqeg`, replaced Glyph dark `dpr913`.
No further replacement search or fetch is scheduled.
The other candidate `gw5mol` was not selected: its edge signature would require
more careful fit-mode handling, whereas the snowfield is clear under the unchanged
transparent widgets. No invented image ID or source attribution.

The current 42 images retain null verified author/license values. Photo appearance,
old upload dates and absent AI tags do not prove capture method or grant reuse
rights. Keep watermarks. In particular `7p6wke` has a visible JT mark; prefer fit
for Sweeteh's wide `21y32m` if necessary to retain its full frame/watermark.
Full original hashes and individual caveats are in `images.*.full_resolution_review`;
the rejected alpine photo is in `rejected_recommendations`. Raw photographs,
seven contact sheets and native photographic evidence remain only under
`/tmp/decebal-wallhaven-curation`, outside git.
