#!/usr/bin/env bash
# No build/install/live daemon activity. MS_BIN_DIR selects already built helpers.
set -euo pipefail
exec /usr/bin/python3 "$(dirname "$0")/notify-theme.py" "$@"
