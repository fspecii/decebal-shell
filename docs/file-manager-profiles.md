# Familiar Files appearance

Settings → Appearance → All apps has **File manager appearance** outside the
Advanced section: Native Files, Windows Explorer inspired, and macOS Finder
inspired. GTK app integration must be enabled. Reopen GNOME Files to load a change.

Explorer uses compact rows and squarer selections; Finder uses softer sidebar
selections and more generous spacing. Both inherit the selected theme's colours.
The profile is an independent preference: changing a theme keeps it. Native Files
removes the additional geometry rules. Disabling app integration restores the
managed GTK files through the existing backup system.

These are visual profiles for modern GTK4 Nautilus, not replacements for its
navigation, toolbar, file operations, or extensions. Other file managers and GTK3
apps retain their own layouts. Icons remain independently selectable in Icons.

Selectors follow upstream Nautilus's `.nautilus-window`, `placessidebar`,
`.nautilus-list-view` and `.nautilus-grid-view` nodes. References:
- https://gitlab.gnome.org/GNOME/nautilus/-/blob/master/src/nautilus-window.c
- https://gitlab.gnome.org/GNOME/nautilus/-/blob/master/src/gtk/nautilusgtkplacessidebar.c
- https://gitlab.gnome.org/GNOME/nautilus/-/blob/master/src/resources/style.css
