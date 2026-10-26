-- Hyprland's default layer animations make the bar's panels and the dock's magnification look glitchy.
hl.layer_rule({ match = { namespace = "^macshell-" }, no_anim = true })
