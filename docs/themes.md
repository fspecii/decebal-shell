# Themes gallery

Five Caelestia-inspired theme packages ship with decebal (`themes/<id>/`, compiled in; the wallpapers are installed to
`/usr/share/decebal/themes/<id>/wallpapers/` by `make install` / the PKGBUILD, which is where a built-in theme finds its
picture). Each has a dark and a light variant, a generated wallpaper under 300 KB (no third-party artwork) and a different mix of bar position, dock
style and titlebar style. The screenshots (`docs/themes/<id>.png`, light variants `<id>-light.png`) are taken by `tests/e2e/themes.sh` on a private Xvfb
with `SHOTS=docs/themes tests/e2e/themes.sh`. The System Settings window in front is the Theme pane, which also lists them.

Apply one with `decthemectl theme apply <id>` (`--preview 10` to try it for 10 seconds, `decthemectl theme undo` to go back).
Anything can then be overridden by the user: `decthemectl theme duplicate <id> mine` makes an editable copy. See [theme-engine.md](theme-engine.md) and [caelestia.md](caelestia.md).

| Theme | Look | Bar | Dock | Titlebars | Apply |
|---|---|---|---|---|---|
| ![Caelestia Mocha](themes/caelestia-mocha.png) | **Caelestia Mocha**: Catppuccin Mocha (Latte in light mode) in the Caelestia shape: a vertical pill bar on the left edge, 20 px radii, tonal mauve surfaces, no dock, Materia circle buttons. | left | hidden (auto) | materia | `decthemectl theme apply caelestia-mocha` |
| ![Rose Pine](themes/rose-pine.png) | **Rose Pine**: Rose Pine dusk (Dawn in light mode): muted rose and gold on plum, a pill bar along the top, a round magnifying dock, traffic-light titlebars. | top | mac, pill | macos | `decthemectl theme apply rose-pine` |
| ![Tokyo Night](themes/tokyo-night.png) | **Tokyo Night**: Tokyo Night (Day in light mode): electric blue and violet on navy with a neon skyline wallpaper, a split top and bottom bar, a compact dock, dot buttons. | split | mac, compact | nord | `decthemectl theme apply tokyo-night` |
| ![Kanagawa](themes/kanagawa.png) | **Kanagawa**: Kanagawa Wave (Lotus in light mode): ink-wash blues on sumi ink, a pill bar on the right, serif window titles, a left-aligned Windows-style taskbar, thin minimal titlebars. | right | windows taskbar | minimal | `decthemectl theme apply kanagawa` |
| ![Material You Dynamic](themes/material-you-dynamic.png) | **Material You Dynamic**: Match wallpaper is on (vibrant scheme, full surfaces), so the shell takes its colours from the picture; a generated aurora gradient is bundled. Bottom pill bar, floating dock, Breeze titlebars. | bottom | mac, floating | breeze | `decthemectl theme apply material-you-dynamic` |

Tags: caelestia-mocha `catppuccin pill vertical-bar`, rose-pine `rose-pine top-bar dock`, tokyo-night `neon split-bar`, kanagawa `ink right-bar serif taskbar`, material-you-dynamic `material-you dynamic wallpaper`.

`material-you-dynamic` writes `theme.dynamic` (Match wallpaper) when applied and the next theme switches it off again; change the wallpaper and the colours follow, see [theming.md](theming.md#match-wallpaper).
