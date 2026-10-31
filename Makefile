PREFIX ?= /usr/local
DESTDIR ?=
PYTHON ?= python3
CARGO ?= cargo
# Written by `make`: the exact executables cargo reported for that build (wherever CARGO_TARGET_DIR or cargo config put
# them) and their sha256. `make install` installs only those, re-checking every digest, never a stale target/release.
BUILD_MANIFEST ?= target/decebal-build-manifest.json
# Binaries, wrappers, units, autostart and desktop entries, themes (fonts, widgets, licences) and docs are listed once,
# in packaging/stage.py; it also writes $(PREFIX)/share/decebal/install-inventory.json, which `make uninstall` uses.
# Values reach it through the environment only, never interpolated into shell text (DESTDIR may contain spaces).
export PREFIX DESTDIR BUILD_MANIFEST
export DECEBAL_CARGO := $(CARGO)

all:
	$(PYTHON) packaging/stage.py build

install: all
	$(PYTHON) packaging/stage.py install

uninstall:
	$(PYTHON) packaging/stage.py uninstall

# Extra login session "Hyprland (decebal)": the Rust shell without Quickshell (Omarchy + SDDM + hyprlock/hypridle/mako).
# PREFIX and DESTDIR reach it through the environment (exported above); it rejects non-portable PREFIX characters.
install-session:
	python3 contrib/session/install.py

.PHONY: check-session-install
check-session-install:
	python3 tests/session-install.py

# Staged install contract with a fake cargo in a private DESTDIR/HOME (no Rust build).
.PHONY: check-packaging
check-packaging:
	python3 tests/packaging-stage.py

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
