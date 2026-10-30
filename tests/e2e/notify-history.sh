#!/usr/bin/env bash
# Actual notification daemon + native history UI on a private D-Bus/Xvfb/WM.
# Never builds or installs. PROFILE=debug|release E2E_DISPLAY=:348
# NOTIFY_HISTORY_EVIDENCE_DIR=/tmp/... preserves synthetic screenshots/logs.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "$0")/../.." && pwd)
export NOTIFY_HISTORY_REPO="$HERE"
export MS_BIN_DIR=${MS_BIN_DIR:-${CARGO_TARGET_DIR:-$HERE/target}/${PROFILE:-release}}
export RDWM_BIN_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
exec /usr/bin/python3 "$HERE/tests/e2e/notify-history.py" "$@"
