#!/usr/bin/env bash
# No build/install/live daemon activity, and no binary directory is defaulted:
#   MS_BIN_DIR=/abs/dir/with/decnotify+decthemectl bash tests/e2e/notify-theme.sh
# Optional: E2E_DISPLAY=:700..:65535 (must be unoccupied; default allocated)
#           NOTIFY_THEME_EVIDENCE_DIR=/new/dir preserves screenshots/logs.
set -euo pipefail
: "${MS_BIN_DIR:?set MS_BIN_DIR to an absolute directory containing decnotify and decthemectl}"
export MS_BIN_DIR
exec /usr/bin/python3 "$(dirname "$0")/notify-theme.py" "$@"
