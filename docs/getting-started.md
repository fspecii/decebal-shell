# Your first session with Decebal

Decebal supplies the desktop shell: Settings, app search, panels, notifications,
wallpapers, widgets and utilities. A window manager still handles application
windows. The integrated rdwm X11 session is the main path for the shortcuts below.
It is not a Linux distribution or a drop-in replacement for every KDE/GNOME service.

Follow the repository [installation instructions](../README.md#install) first.
When using `rdwm-decebal-session`, let that session supervise the panels; do not
also start a second set through user services or another autostart file. Other
window managers can launch the individual Decebal programs and supply their own
key bindings. X11 needs a compositor for transparency and blur.

## Find familiar tools

| What you want | Decebal |
|---|---|
| Start menu / Spotlight search | Super+Space in the rdwm sample profile; `decspot` |
| All installed apps | `declaunch` |
| System Settings | `decsettings` |
| Task Manager / Activity Monitor | `decmon` |
| Switch applications | Alt+Tab in the rdwm sample profile |
| Mission Control / window overview | `decexpose`; `--all` includes every workspace |
| Screenshot | Search “Screenshot” in Super+Space; Escape cancels region selection |
| Wallpaper | `decsettings wallpaper` |
| Icon chooser | `decsettings icons` |
| Desktop widgets | `decsettings widgets` |

Bindings are editable. See Settings → Shortcuts and rdwm's
[sample profile](https://github.com/fspecii/rdwm/blob/master/contrib/config-macos.toml)
if your installed keys differ.

## Choose a look, then change its parts

Open Appearance → Themes and choose a theme. A theme can supply colors, window
buttons, panels, wallpaper and widgets. Its preview is stored after its first
render and reused on later visits. A rail-only theme stops its dock process;
switching to a dock theme starts it again.

Wallpaper is a separate section. Pick a local picture or browse the configured
wallpaper sources there. Appearance → Palettes controls colors; Customise holds
fonts, window decorations and panel controls. You do not need to edit TOML to
change these preferences.

In Widgets, add from the available collection and adjust each widget's anchor
and offsets. If you customize a layout, a theme with widgets asks whether to keep
or replace it. A theme without widgets hides the collection without deleting it.

Appearance → All apps carries the palette to supported GTK apps, terminals,
browsers and editors. Explicit per-app opt-outs are respected. Browser updates
use Omarchy's policy helper where available; websites keep their own styling.
Already-open GTK apps need reopening for a full custom palette change.

## Move windows comfortably

In the current rdwm sample profile, Super+F enters fullscreen. Press it again to
return to a centered, freely movable window. Hold Shift while dragging when you
want a snap zone; ordinary dragging stays free. Opening another app can arrange
ordinary floating/tiled windows into the current workspace layout. An explicitly
centered Super+F window keeps its free position until you opt back into tiling.

## Recover a change or report a problem

Theme CLI changes record an undo point: `decthemectl theme undo`. App integration
can be disabled with `decthemectl system off`, which restores its managed originals.
Config & Presets provides the editable configuration and preset tools. Keep a
working preset before a large customization session.

For a failed app start, check `~/.local/state/decebal/launch.log`. For other
problems use `decthemectl log tail 50`; see [debugging](debugging.md). Include what
you clicked, the selected theme, X11/Wayland, and a relevant log excerpt. Review
logs and screenshots for private information before sharing them.
