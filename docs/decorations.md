# Window button and titlebar catalogue

Audit: 30 October 2026, read-only rdwm checkout. Exact revision and source hashes
are recorded in [audit.json](decorations/audit.json). Inspected rdwm-deco
config.rs, styles.rs, titlebar.rs, renderer lib.rs and core tests/deco.rs, plus
actual offline `rdwmctl schema --offline` output. No rdwm renderer was duplicated
or edited. This first delivery exposes existing implementations, not new ports.

Appearance → Customise → Window style now has a **Button and titlebar design**
picker for 14 real supported IDs. An older live rdwm only sees its supported
subset. The refreshed offline schema exposes the modern set too. Border-only is
excluded because it cannot meet the close/minimise/maximise requirement; existing
hand-authored border-only configurations remain loadable.

Choosing a design enables titlebars/buttons and removes stale geometry overrides
so the native style defaults apply. Colors, font and scale remain independent.
Maximise is the default zoom action if none was explicitly set; a user's explicit
fullscreen/monocle/float choice is preserved. The control is named Maximise / zoom,
not Green button. No theme, widget layout, palette or lock behavior is changed by
choosing a decoration design. The normal config transaction/validation is reused.

The UI's default metrics come from `src/settings/decoration-catalogue.json`,
automatically exported by the **actual native renderer**, not copied from a
macOS row. This keeps height/radius/button-size controls honest when a value is
inherited. Regenerate the schema and metadata when upgrading rdwm; the catalogue
snapshot should move with its corresponding native renderer version.

## Existing native catalogue

| Native ID | Side / shape / look | Native height / radius | Status and 1× evidence |
| --- | --- | --- | --- |
| `macos` | Left / Circle / Lights | 28 / 10 px | Exposed: [dark](decorations/native/macos-dark-1x.png), [light](decorations/native/macos-light-1x.png) |
| `macos-light` | Left / Circle / Lights | 28 / 10 px | Exposed: [dark](decorations/native/macos-light-dark-1x.png), [light](decorations/native/macos-light-light-1x.png) |
| `classic` | Right / Cell / Ghost | 28 / 0 px | Exposed: [dark](decorations/native/classic-dark-1x.png), [light](decorations/native/classic-light-1x.png) |
| `windows` | Right / Cell / Ghost | 32 / 8 px | Exposed: [dark](decorations/native/windows-dark-1x.png), [light](decorations/native/windows-light-1x.png) |
| `windows-light` | Right / Cell / Ghost | 32 / 8 px | Exposed: [dark](decorations/native/windows-light-dark-1x.png), [light](decorations/native/windows-light-light-1x.png) |
| `adwaita` | Right / Circle / Ghost | 36 / 12 px | Exposed: [dark](decorations/native/adwaita-dark-1x.png), [light](decorations/native/adwaita-light-1x.png) |
| `adwaita-light` | Right / Circle / Ghost | 36 / 12 px | Exposed: [dark](decorations/native/adwaita-light-dark-1x.png), [light](decorations/native/adwaita-light-light-1x.png) |
| `breeze` | Right / Circle / Ghost | 30 / 5 px | Exposed: [dark](decorations/native/breeze-dark-1x.png), [light](decorations/native/breeze-light-1x.png) |
| `arc` | Right / Circle / Ghost | 30 / 6 px | Exposed: [dark](decorations/native/arc-dark-1x.png), [light](decorations/native/arc-light-1x.png) |
| `materia` | Right / Circle / Ghost | 34 / 3 px | Exposed: [dark](decorations/native/materia-dark-1x.png), [light](decorations/native/materia-light-1x.png) |
| `nord` | Right / Circle / Lights | 30 / 8 px | Exposed: [dark](decorations/native/nord-dark-1x.png), [light](decorations/native/nord-light-1x.png) |
| `gruvbox` | Right / Square / Lights | 28 / 4 px | Exposed: [dark](decorations/native/gruvbox-dark-1x.png), [light](decorations/native/gruvbox-light-1x.png) |
| `dracula` | Left / Circle / Lights | 28 / 8 px | Exposed: [dark](decorations/native/dracula-dark-1x.png), [light](decorations/native/dracula-light-1x.png) |
| `minimal` | Right / Circle / Lights | 20 / 0 px | Exposed: [dark](decorations/native/minimal-dark-1x.png), [light](decorations/native/minimal-light-1x.png) |
| `borderonly` | Right / Circle / Ghost | 8 / 0 px | Diagnostic only; no buttons: [dark](decorations/native/borderonly-dark-1x.png), [light](decorations/native/borderonly-light-1x.png) |

The named GNOME/Adwaita, KDE/Breeze, Arc and Material designs already exist in
rdwm. Nord/Gruvbox/Dracula entries combine the renderer's own geometry with named
color treatments; they are not additional full desktop themes. `*-light` entries
are native variant IDs, not a claim of new button structure. Decebal's independent
palettes can recolor any structure. These are rdwm's interpretations, not pixel
clones of every release of a GTK/Qt/CSD theme.

Reference families: [GNOME/libadwaita](https://gitlab.gnome.org/GNOME/libadwaita),
[KDE Breeze](https://github.com/KDE/breeze),
[Arc](https://github.com/jnsh/arc-theme),
[Materia](https://github.com/nana-4/materia-theme).
This delivery redistributes no artwork, GTK/Qt theme files or proprietary button
assets from them. The existing native code and generated previews use rdwm's MIT
implementation; the source family names remain attribution/reference labels.

## Reproduction and QA

```
python tools/build-decoration-preview.py /path/to/rdwm
helper=target/decoration-preview-helper/target/debug/decebal-deco-preview
"$helper" metadata > src/settings/decoration-catalogue.json
"$helper" catalogue docs/decorations/native
```

The optional helper compiles against read-only rdwm-deco; all build files stay
in this checkout's ignored target/. It generated 60 native sheets: 15 IDs ×
dark/light neutral application palettes × 1×/2×. Each sheet contains focused,
unfocused, close-hover, minimise-hover and maximise-hover rows. The exact drawn
button-center hit areas are tested through rdwm-deco's real hit-test function at
both scales. The user-selected theme's caption samples are also generated by
this helper in automatic gallery previews; no generic traffic lights are drawn.

The read-only native test suite passed 526 tests (including doc tests), zero
failures, two ignored. It includes all-style geometry/button routing, close and
minimise commands, zoom/maximise modes, text/button overlap and 1×–3×/fractional
render scales. Artifacts were built under `target/rdwm-decoration-audit`; no
rdwm source, runtime binary or live WM/session changed. GUI clicks on live
application windows were deliberately not used as a test.

Decebal's tests cover the supported-set filter, all 14 design transactions,
native defaults, enabled controls, color/font/DPI preservation, explicit zoom
choice retention and search indexing. Native picker UI evidence is generated
headlessly by the ignored `native_decoration_picker_evidence` test using private
config. See [dark](decorations/picker-dark.png) and
[light](decorations/picker-light.png). No display server or live config is used.

## Incremental missing-style backlog

1. **Elementary / Pantheon**: not an existing style ID. Inspect a pinned
   [elementary stylesheet](https://github.com/elementary/stylesheet) and Gala
   reference/license before proposing geometry, glyphs and hover/inactive states.
   Scope: one new native rdwm-deco style entry if existing shape/glyph primitives
   suffice, corresponding enum/schema registration, all three functional caption
   controls, native hit/action/scale tests and generated evidence. Preserve the
   user's control availability even where a reference omits minimise. This needs
   a proper tool-managed **rdwm checkout coordinated by the parent**; no edit of
   `/home/ambsd/Projects/rdwm` or shell-created worktree is authorized here.
2. **Orchis**: first inspect exact upstream variants and determine whether its
   buttons require a new shape/interaction or just an Adwaita/Materia palette.
   Do not create a redundant renderer for colors alone.
3. **Catppuccin / Nordic**: first compare actual decoration structure with existing
   macOS/Nord/Breeze IDs. Prefer named palettes when geometry is already covered;
   add a renderer only for a documented structural difference.

One missing design per reviewed/tested commit. Existing GNOME/Adwaita, KDE/Breeze,
Arc and Materia are already exposed and are not backlog placeholders. Broader
window-manager/client-side decoration parity, cursor packs and app themes are
separate from this structural picker. Parent owns integration and deployment.

Final Decebal checkpoint: 1,467 tests passed, zero failed, five ignored and the
current-user PAM-start probe filtered. The ignored native picker evidence test
was then run explicitly in separate private dark/light processes: both passed.
No production authentication or live window-manager interaction was used.
