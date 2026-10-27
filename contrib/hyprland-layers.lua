-- Hyprland's default layer animations make the bar's panels and the dock's magnification look glitchy.
hl.layer_rule({ match = { namespace = "^macshell-" }, no_anim = true })
-- The Launchpad overlay: blurred, no animation.
hl.layer_rule({ match = { namespace = "^maclaunch$" }, blur = true, ignore_alpha = 0.05, no_anim = true })
-- The Spotlight overlay: blurred, no animation.
hl.layer_rule({ match = { namespace = "^macspot$" }, blur = true, ignore_alpha = 0.05, no_anim = true })
