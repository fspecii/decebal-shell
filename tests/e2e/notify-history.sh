#!/usr/bin/env bash
# Actual notification daemon + native history UI on a private owned D-Bus/Xvfb/WM.
# Never builds or installs, and no binary directory is defaulted: the caller supplies
# immutable, already built binaries.
#   MS_BIN_DIR=/abs/dir/with/decnotify RDWM_BIN_DIR=/abs/dir/with/rdwm bash tests/e2e/notify-history.sh
# Optional: E2E_DISPLAY=:700..:65535 (must be unoccupied; default allocated)
#           NOTIFY_HISTORY_EVIDENCE_DIR=/new/dir preserves synthetic screenshots/logs.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd)
: "${MS_BIN_DIR:?set MS_BIN_DIR to an absolute directory containing decnotify}"
: "${RDWM_BIN_DIR:?set RDWM_BIN_DIR to an absolute directory containing rdwm}"
export MS_BIN_DIR RDWM_BIN_DIR
exec /usr/bin/python3 "$HERE/notify-history.py" "$@"
