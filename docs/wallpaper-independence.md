# Wallpaper is an independent choice

Changing a desktop theme, its colour mode, or its palette keeps your wallpaper
image, scaling, background colour and slideshow preferences. Theme undo and
preview cancellation also keep wallpaper choices made after the theme change.
Package `[wallpaper]` defaults and `match_colors` no longer change those settings.
Wallpaper colour matching remains an explicit choice in the Wallpaper section.

**Settings → Wallpaper → For this theme** opens two individually selected
Wallhaven photographs for the selected supported theme: a dark and a light
recommendation. The Wallhaven Show selector exposes the same view. Custom themes
without recommendations can use Browse, Favourites or My backgrounds normally.

Opening a recommendation downloads only its preview and metadata. Set wallpaper
or Download starts the full image download. A fresh detail check must still report
SFW and must not contain the AI-art marker or excluded generator/illustration tags;
changed or unavailable recommendations are refused, with no automatic substitute.
The uploader, unverified authorship and unknown licence are displayed separately.
Visual review and absence of AI tags cannot certify a photograph's provenance.

Downloaded pictures use the existing backgrounds folder and can be selected
there offline. Theme-gallery previews use a downloaded recommendation when it is
available locally, with proportional fill cropping; otherwise they show the
theme's background colour. Generating a theme preview never downloads or applies
a wallpaper. Custom theme previews may still sample a package's own local image.
Preview PNGs remain cached across launches; a changed local sample invalidates
that preview's cache key.

Theme undo still restores other theme-related settings from its saved snapshot;
this change only makes the wallpaper preference independent.
