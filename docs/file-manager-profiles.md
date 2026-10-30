# Familiar Files appearance

Settings → Appearance → All apps has **File manager appearance** outside the
Advanced section: **Follow theme**, Native Files, Windows Explorer inspired, and
macOS Finder inspired. Follow theme is the default when no preference has been
saved. Windows 11 and Frutiger Aero select Explorer; macOS and Graphite select
Finder; other themes select Native. The row shows the currently resolved profile.
GTK app integration must be enabled. Reopen GNOME Files to load a change.

Explorer uses compact sidebar/list rows, an 8px grid inset and squarer selections.
Its address breadcrumbs sit in a framed view-colour surface; the column header
uses the sidebar colour with a fine separator. Finder uses softer sidebar
selections, stronger sidebar labels, a 12px grid inset and more generous spacing.
Its breadcrumb surface is softly tinted, and alternating unselected list rows
help scan filenames. Hover, selection and drop feedback retain their native
priority. Both have a subtle header separator and shaped floating status badges.

All surface colours and separators derive from the selected theme's GTK palette;
there are no embedded light/dark colours or icon substitutions. Breadcrumb button
sizes and overflow stay native. Column widths, horizontal row/cell padding and
rubberband-selection margins are unchanged to preserve Nautilus hit testing.
An explicit Native, Explorer or Finder choice stays fixed when a theme changes.
Previously saved explicit choices are preserved. Follow theme resolves against the
desktop theme identity, so changing a colour palette or using macOS window buttons
on an unrelated rice theme does not change the Files profile. Native Files removes
the additional geometry rules. The preference is `system.fileManagerStyle` in
`decebal.json`: `theme` (also accepts `follow-theme`), `native`, `explorer` or `finder`. Disabling app integration restores the
managed GTK files through the existing backup system.

These are visual profiles for modern GTK4 Nautilus, not replacements for its
navigation, toolbar, file operations, or extensions. Other file managers and GTK3
apps retain their own layouts. Icons remain independently selectable in Icons.

Use Files' existing navigation with either profile:

- Press **Ctrl+L** to type or paste a folder path, then **Enter** to open it;
  **Escape** returns to the breadcrumb bar.
- **Alt+Left / Alt+Right** move back and forward; **Alt+Up** opens the parent folder.
- Use Files’ view controls to choose list or grid view. The profile does not force
  a view mode or change your sorting, zoom, selection, or file-operation preferences.

There is no Windows ribbon, Finder column view, or replacement address bar.
The profiles shape Files' existing widgets. They preserve native file selection,
context menus, drag and drop, breadcrumb navigation and keyboard handling.

Selectors follow upstream Nautilus's `.nautilus-window`, `placessidebar`,
`.nautilus-list-view` and `.nautilus-grid-view` nodes. References:
- https://gitlab.gnome.org/GNOME/nautilus/-/blob/master/src/nautilus-window.c
- https://gitlab.gnome.org/GNOME/nautilus/-/blob/master/src/gtk/nautilusgtkplacessidebar.c
- https://gitlab.gnome.org/GNOME/nautilus/-/blob/master/src/resources/style.css

Validation covers GTK 4.24 CSS parsing and GNOME Files 51.1 in a private Xvfb
session with an isolated D-Bus, HOME and XDG directories. The native check opens
fixture folders in list/grid views using the automatically resolved Windows 11 and
macOS profiles, plus a dark Windows 11 run using a separate Nord palette through
the compiled app-integration pipeline, and exercises address navigation, back/parent,
and creating a folder. This does not change the live desktop or user preferences.
Other Nautilus versions may vary in their widget structure; these profiles are
not a promise of pixel-identical Explorer or Finder layouts.

Run the native regression from a checkout with rdwm built:

```sh
E2E_DISPLAY=:339 FILE_MANAGER_EVIDENCE_DIR=/tmp/files-profile-evidence \
  bash tests/e2e/file-manager-profiles.sh
```

Choose an unused display number of at least 2. `RDWM_BIN_DIR` can override the
window-manager binary directory. `DECTHEMECTL_BIN` selects the compiled CLI (by
default `target/release/decthemectl`, rebuilt with Follow theme support). The
harness compiles only a tiny standalone CSS
generator from the current source, establishes private HOME/XDG paths before any
native process starts, and uses private session buses. It removes its temporary
state on exit and keeps screenshots/logs in the evidence directory (a printed
temporary directory by default). Exit 77 means a required tool or private display
is unavailable; no release build or installation is performed.
