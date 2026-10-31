PREFIX ?= /usr/local
DESTDIR ?=
SHOWCASE_THEMES = gruvbox-dark everforest synthwave frutiger-aero gnome-adwaita-like
# Programs that did not exist under the old macshell name get no mac* compatibility wrapper.
NO_COMPAT = declock decmon decwall decnotes dectimer
BINS = decwall decdock decbar dechaven declaunch decspot decswitch decexpose dechud declock decdash decmon decsession decutil decsettings decthemectl decnotify decwm decclip dechot decwidgets decnotes dectimer
UNITS = decebal-timer.service decebal-live-wallpaper.service decebal-widgets.service decebal-dock.service decebal-bar.service decebal-notify.service decebal-clip.service decebal-hot.service decebal-wallpaper-colors.service
# Themes that ship assets (wallpaper, widgets, screenshot): installed as system packages, found next to the compiled-in manifest.
ASSET_THEMES = nothing-dot nothing-light brutalist terminal-minimal terminal-amber orbit-glass paper-signal moss-atlas signal-deck nextkde gnome-adwaita gnome-catppuccin-dock
AUTOSTART = decebal-timer.desktop decebal-live-wallpaper.desktop decebal-widgets.desktop decebal-dock.desktop decebal-bar.desktop decebal-notify.desktop decebal-clip.desktop decebal-hot.desktop decebal-wallpaper-colors.desktop

all:
	cargo build --release --locked

install: all
	for b in $(BINS); do install -Dm755 target/release/$$b $(DESTDIR)$(PREFIX)/bin/$$b; done
	# DEPRECATED compatibility names: macbar -> decbar, ... (one release, so existing keybinds and autostart keep working)
	for b in $(filter-out $(NO_COMPAT),$(BINS)); do install -Dm755 contrib/compat/mac-compat $(DESTDIR)$(PREFIX)/bin/mac$${b#dec}; done
	for u in $(UNITS); do install -Dm644 contrib/$$u $(DESTDIR)$(PREFIX)/lib/systemd/user/$$u; sed -i 's|/usr/bin/|$(PREFIX)/bin/|' $(DESTDIR)$(PREFIX)/lib/systemd/user/$$u; done
	for d in $(AUTOSTART); do install -Dm644 contrib/$$d $(DESTDIR)/etc/xdg/autostart/$$d; done
	install -Dm644 contrib/dechaven.desktop $(DESTDIR)$(PREFIX)/share/applications/dechaven.desktop
	install -Dm644 contrib/decsettings.desktop $(DESTDIR)$(PREFIX)/share/applications/decsettings.desktop
	install -Dm644 contrib/decnotes.desktop $(DESTDIR)$(PREFIX)/share/applications/decnotes.desktop
	install -Dm644 contrib/dectimer.desktop $(DESTDIR)$(PREFIX)/share/applications/dectimer.desktop
	install -Dm644 contrib/decmon.desktop $(DESTDIR)$(PREFIX)/share/applications/decmon.desktop
	install -Dm644 contrib/dbus/org.freedesktop.Notifications.service $(DESTDIR)$(PREFIX)/share/decebal/dbus/org.freedesktop.Notifications.service
	# every theme package (manifest, wallpapers, widgets, screenshots): /usr/share/decebal/themes/<id>/
	for f in $$(cd themes && find . -type f); do install -Dm644 themes/$$f $(DESTDIR)$(PREFIX)/share/decebal/themes/$${f#./}; done
	install -Dm644 README.md $(DESTDIR)$(PREFIX)/share/doc/decebal/README.md
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/decebal/LICENSE

uninstall:
	for b in $(BINS); do rm -f $(DESTDIR)$(PREFIX)/bin/$$b $(DESTDIR)$(PREFIX)/bin/mac$${b#dec}; done
	rm -f $(DESTDIR)$(PREFIX)/bin/macshell-session
	for u in $(UNITS); do rm -f $(DESTDIR)$(PREFIX)/lib/systemd/user/$$u; done
	for d in $(AUTOSTART); do rm -f $(DESTDIR)/etc/xdg/autostart/$$d; done
	rm -f $(DESTDIR)$(PREFIX)/share/applications/dechaven.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/decsettings.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/decnotes.desktop $(DESTDIR)$(PREFIX)/share/applications/dectimer.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/decmon.desktop
	rm -rf $(DESTDIR)$(PREFIX)/share/decebal/dbus $(DESTDIR)$(PREFIX)/share/decebal/themes $(DESTDIR)$(PREFIX)/share/doc/decebal
	rm -rf $(DESTDIR)$(PREFIX)/share/licenses/decebal
	for t in $(SHOWCASE_THEMES); do rm -rf $(DESTDIR)$(PREFIX)/share/decebal/themes/$$t; done

# Extra login session "Hyprland (decebal)": the Rust shell without Quickshell (Omarchy + SDDM + hyprlock/hypridle/mako).
# Export literal values instead of interpolating them into a shell recipe.
# The installer rejects non-portable PREFIX characters before writing files.
export PREFIX DESTDIR
install-session:
	python3 contrib/session/install.py

.PHONY: check-session-install
check-session-install:
	python3 tests/session-install.py

# End-to-end tests against rdwm on a private Xvfb (need Xvfb, xdotool, jq and a built rdwm; see the scripts).
e2e-theme: all
	tests/e2e/theme.sh

# Font presets: decthemectl font, bar / dashboard / Theme pane screenshots (Xvfb, xdotool, import, jq, dbus-daemon).
e2e-fonts:
	PROFILE=release tests/e2e/fonts.sh
# "Apply to all apps" in a private HOME, Xvfb and D-Bus session (needs Xvfb, xdotool, import, xrdb, dbus-run-session, PyGObject; nautilus and Omarchy are used when present).
e2e-system: all
	tests/e2e/system-theme.sh

e2e-decsettings: all
	tests/e2e/decsettings.sh

# decdash on a private Xvfb + private session bus with fake MPRIS players (needs Xvfb, xdotool, import, dbus-daemon, tesseract).
e2e-dash:
	tests/e2e/dash.sh

# decnotify on a private session bus + Xvfb (needs Xvfb, dbus-daemon, busctl, xdotool, import).
e2e-notify:
	cargo build --release --locked --bin decnotify
	DECNOTIFY_BIN=$(CURDIR)/target/release/decnotify tests/e2e/notify.sh

# decclip (clipboard history) on a private Xvfb (needs Xvfb, xclip, xdotool, xprop, import, jq).
e2e-clip:
	tests/e2e/clip.sh

# dechot (hot corners) on a private Xvfb with shimmed commands (needs Xvfb, xdotool, xrandr, xprop).
e2e-hot:
	tests/e2e/hot.sh

# The Caelestia bar (left edge) on a private Xvfb + rdwm (needs Xvfb, xdotool, xprop, import, jq, convert).
e2e-procmon:
	tests/e2e/procmon.sh

e2e-awake:
	tests/e2e/awake.sh

e2e-vbar:
	tests/e2e/vbar.sh

# Every Caelestia layout (left, right, top, bottom, split) on a private Xvfb :96 + rdwm (needs Xvfb, xdotool, xprop, import, jq).
e2e-layouts:
	tests/e2e/layouts.sh

# decsession + decutil on a private Xvfb with shimmed systemctl/loginctl/ffmpeg/xset/picom/powerprofilesctl (Xvfb, xdotool, import).
e2e-session: all
	tests/e2e/session.sh

test:
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo test --all-features

.PHONY: all install install-session uninstall test e2e-decsettings e2e-theme e2e-system e2e-fonts e2e-notify e2e-clip e2e-hot e2e-session e2e-dash e2e-vbar e2e-procmon e2e-awake e2e-layouts

# Shared widgets and the new design gallery, each on an isolated desktop.
e2e-widgets-settings:
	tests/e2e/widgets-settings.sh

e2e-theme-studio:
	tests/e2e/theme-studio.sh

e2e-glass:
	tests/e2e/glass.sh

# Recent end-to-end regressions, each on a separate private X11 desktop.
.PHONY: e2e-desktop-smoke e2e-theme-dock e2e-launcher-screenshot e2e-preview-cache e2e-widget-creator-icons
e2e-desktop-smoke: e2e-theme-dock e2e-launcher-screenshot e2e-preview-cache e2e-widget-creator-icons e2e-notes-timer e2e-welcome-accessibility e2e-curated-wallpaper e2e-timer-recovery e2e-shadow-controls check-theme-art

e2e-theme-dock: all
	tests/e2e/theme-dock-lifecycle.sh

e2e-launcher-screenshot: all
	tests/e2e/launcher-screenshot.sh

e2e-preview-cache: all
	tests/e2e/theme-preview-cache.sh

e2e-widget-creator-icons: all
	tests/e2e/widget-creator-icons.sh

# Private native Notes/Timer integration, no user session or notification bus.
e2e-notes-timer: all
	tests/e2e/notes-timer.sh

.PHONY: e2e-notes-timer

# Optional native Nautilus check; no full shell rebuild is needed.
.PHONY: e2e-file-manager-profiles
e2e-file-manager-profiles:
	bash tests/e2e/file-manager-profiles.sh

.PHONY: e2e-app-indicators
e2e-app-indicators: all
	bash tests/e2e/sni-fixture.sh
	bash tests/e2e/sni-backend.sh
	PROFILE=release bash tests/e2e/sni-ui.sh
	PROFILE=release bash tests/e2e/sni-ui.sh --tiny-rail

e2e-welcome-accessibility: all
	tests/e2e/welcome-accessibility.sh

.PHONY: e2e-welcome-accessibility

.PHONY: e2e-curated-wallpaper
e2e-curated-wallpaper: all
	MS_BIN_DIR=$(CURDIR)/target/release tests/e2e/curated-wallpaper.sh

# Requires a matching rdwm build with unified X11 shadow support.
.PHONY: e2e-theme-shadows
e2e-theme-shadows: all
	MS_BIN_DIR=$(CURDIR)/target/release tests/e2e/theme-shadows.sh

.PHONY: e2e-timer-recovery e2e-shadow-controls check-theme-art
e2e-timer-recovery: all
	TIMER_BIN=$(CURDIR)/target/release/dectimer python3 tests/timer-recovery.py

e2e-shadow-controls: all
	MS_BIN_DIR=$(CURDIR)/target/release tests/e2e/shadow-controls.sh

check-theme-art:
	python3 tests/wallpaper-package-contract.py
	python3 tests/retire-generated-assets.py
