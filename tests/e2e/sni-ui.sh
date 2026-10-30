#!/usr/bin/env bash
# Native SNI regression; deliberately never builds/installs binaries.
# E2E_DISPLAY=:345 PROFILE=release bash tests/e2e/sni-ui.sh
# Add --tiny-rail for a 600px-tall rail; --probe-real-app checks OBS protocol only.
# SNI_UI_EVIDENCE_DIR=/tmp/... keeps screenshots and logs; default temp removed.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "$0")/../.." && pwd)
export SNI_UI_REPO="$HERE"
export MS_BIN_DIR=${MS_BIN_DIR:-${CARGO_TARGET_DIR:-$HERE/target}/${PROFILE:-release}}
export RDWM_BIN_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
exec /usr/bin/python3 "$HERE/tests/e2e/sni-ui.py" "$@"
