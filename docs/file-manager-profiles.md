# Familiar Files appearance

Settings → Appearance → All apps has **File manager appearance** outside the
Advanced section: Native Files, Windows Explorer inspired, and macOS Finder
inspired. GTK app integration must be enabled. Reopen GNOME Files to load a change.

Explorer uses compact rows, an 8px grid inset and squarer selections; Finder uses
softer sidebar selections, a 12px grid inset and more generous row spacing.
Breadcrumb corners follow each profile, while native breadcrumb button sizes and
overflow remain unchanged. Both inherit the selected theme's colours.
The profile is an independent preference: changing a theme keeps it. Native Files
removes the additional geometry rules. Disabling app integration restores the
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

The generated native, Explorer and Finder stylesheets have also been parsed by
GTK 4.24's native CSS provider without errors. This verifies CSS syntax; it does
not replace checking appearance and navigation in your installed Nautilus version.
