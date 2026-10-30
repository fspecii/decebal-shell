# Native Dux launcher: Applications provider

This opt-in port uses `[launcher] style = "dux"`. Other themes retain the
existing Launchpad grid. It uses the same installed `.desktop` applications,
filter and typed `spawn_app` launch path; typed text never becomes a shell
command, plugin expression or arbitrary-command provider.

## Measured source contract

Source: `duxlabs-dev/dux-shell` commit
`87312075dd414a6f60691464bcd6c6a9bb242a7e`, inspected read-only. The original
license/provenance ledger remains in `dux-adaptation-handoff.md`. The actual
source-component captures at `/tmp/decebal-dux-replica-specialist/launcher-capture`
were viewed in both font scales. The `upstream-full-*` matrix renders actual
LauncherContent in the full/compact620×620 configuration. The wrapper excludes
modal chrome and shadow; it is source-content evidence, not full-modal action QA.

| Source | Default contract | Native result |
| --- | --- | --- |
| `Common/settings/SettingsSpec.js` | `launcherStyle=full`, `dankLauncherV2Size=compact`, footer enabled, outer border disabled | Full compact native modal; no outer outline. |
| `DLauncherV2/Controller.qml:sectionDefinitions` | **Applications default list**, not grid; Pinned/settings/files are separate list providers | Native Applications list. Existing app launch trust is unchanged. |
| `Components/LauncherMetrics.qml`, `Theme.qml` | Compact width620; preferred height620; screen margin100; search48; section32; footer32; eight rows at56+2 pitch; bottom fade reservation24 | 620×620 when space permits, clamped to output minus100; actual hit rectangles share the list layout. |
| `LauncherContent.qml` | Search inset12, results below header, footer at bottom | Search `(12,12,width−24,48)`, section y64..96, first row y100, footer last32px. |
| `Components/LauncherRow.qml` | Row padding16; icon36; name `fontSizeLarge=16`, supporting `fontSizeSmall=12`; icon-to-text gap16 | Real app icons, names and bounded real desktop-entry comments. Name x=80 in a620px modal. |
| `DCommon/Widgets/DSearchField.qml`, `DTextField.qml` | Pill search radius24; icon24; focused primary icon; text16; clear accessory | Native editable UTF-8 query and real clear hit target. Left/right edit query; up/down/page navigate results. |
| `DListItem`/`DListHighlight` | Grouped rows: outer radius16, inner4; selected tonal container | Native row groups and selected state, semantic palette colours. |
| `ResultsList.qml` | Empty Apps provider: apps glyph and “No results found” | Explicit empty state, no fabricated matches. |

The Source's full model also has All, Files, Plugins, Pinned and contextual
Actions. This bounded native provider does **not** advertise those capabilities.
Its footer says Apps, with working navigation/launch hints. It does not paint
functional-looking provider switches or an Actions menu that do nothing.

## Intentional accessibility correction

The actual upstream component capture at fontScale2 keeps a56px row and58px
pitch while its name/subtitle text boxes together reach approximately74px.
Visible rows overlap. Native fonts are not shrunk to imitate that defect.

At100%, rows remain56px. Enlarged text grows the native row floor to
`max(56,42×fontScale)` (84px at200%) while keeping source icon size36 and search
height48. The actual-font evidence test checks primary/supporting glyph ink
bounds, separation inside each row and separation from the next row. Fewer rows
fit, and page/wheel/selection scrolling keep them reachable. This is a deliberate
200% fidelity difference for readable controls.

## Paint and data lifecycle

Desktop comments are read once during Dux state creation, only from the
application's recorded `.desktop` file, capped at64KiB and240 displayed
characters. Missing comments remain absent. The list uses the existing cached
installed-app model; it does not introduce filesystem search or app-history
storage.

Only visible36px icon pixmaps are held in the Dux state's bounded map. They are
prepared on construction, resize, query/navigation/scroll events or theme/icon
generation changes. The Dux painter only reads that map; it never calls an icon
loader, reads a file or touches a network. The modal pixmap is reused across
frames. Material icon fonts are prepared outside painting. Text is fitted using
its actual weight and bounded before repeated measurement.

## Verification

`dux_launcher_native_render_evidence` is ignored in the ordinary parallel suite
because it changes process-global font/theme state. Run it alone with an
explicit private `DECEBAL_CONFIG` selecting the actual Dux package and fonts,
and `DUX_LAUNCHER_EVIDENCE_DIR=/tmp/evidence`. It emits16 actual native-font PNGs:
light/dark,100/200%,620/320px panel heights, first page/scrolled page. It checks
actual Google Sans, icon cache population, row bounds/hits, paging and true
primary/supporting ink separation.

`tests/e2e/dux-launcher-native.py BIN PACKAGE EVIDENCE` copies the candidate only
into private test storage. It creates private `.desktop` fixtures, blocks the
inherited session/system buses, isolates every HOME/XDG path, and opens Xvfb on
440..469 (never0 or430). The eight native cases cover light/dark,100/200%,960×720
and960×420: list paging, an unmatched command-shaped query that cannot execute,
empty state, the real clear accessory, filtering and Enter launching exactly the
selected typed desktop fixture. All private processes/files are cleaned up.

Final isolated gates passed on 2026-10-30:

- `/tmp/decebal-dux-launcher-raster`: 16 actual-font renders, including the
  monitor sizing/centered-popup regression and true ink separation assertions.
- `/tmp/decebal-dux-launcher-native/checks.json`: all eight native combinations
  passed with actual window geometry assertions, paging, unmatched-query safety,
  clear-button hit testing and typed fixture launch. The 960×420 panel is
  centered at `(170,50)`, sized620×320; its200% text remains separated.

Native testing caught and fixed a real X11 host issue: Dux now supplies its
monitor-clamped size before window creation and requests centered placement.
Legacy launcher placement remains unchanged. The launch harness accepts
xdotool's possible key-release BadWindow after a successful key-press launch;
it independently requires the exact fixture action and process termination.

## Remaining differences

This is the Applications-provider composition, not the complete DLauncherV2
controller. Source pinning/history, All/Files/Plugins/settings providers,
list/grid/tile switching, contextual action panels, category filtering and
source badges, source fuzzy scoring and rich query highlighting are not implemented;
matching retains the existing prefix/substring app filter. No provider is replaced with a static picture.

Selection moves immediately; the Source highlight's spatial tween, ripple and
modal enter/exit motion remain outstanding. Reduced motion has no extra timer
or worker here. Layer-shell/frame-connected emergence and global keyboard entry
remain backend/platform work. These limits are separate from the functioning
native app query, navigation and trusted application launch path.

Desktop descriptions are now cached on demand for visible rows, not reread for
every installed app at construction. Reads are capped at64KiB per entry and
respect LC_MESSAGES locale fallback (country/modifier/language/plain Comment)
from the freedesktop desktop-entry specification:
https://specifications.freedesktop.org/desktop-entry/latest/localized-keys.html .
The shared application index and visible icon/description preparation retain
their existing synchronous event-time I/O boundary; this is not a fully async
application-index implementation. No file reads occur in painting.
