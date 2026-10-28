-- Floating, centred Launchpad window (tested on Hyprland with the Lua config).
hl.window_rule({ match = { class = "^declaunch$" }, float = true, center = true, size = { 1000, 680 } })
