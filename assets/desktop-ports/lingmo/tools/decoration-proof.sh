#!/bin/sh
# Real old/new window-manager proof of the mode-specific window style: a private rdwm (borrowed or installed build, read-only) in a private Xvfb on a
# reserved high display, the real decthemectl applying the Lingmo package in one mode, then what the config and the RUNNING rdwm hold.
# usage: decoration-proof.sh DECTHEMECTL RDWM_BIN_DIR WORK_DIR DISPLAY_NUM light|dark
# Output (stdout): the style the apply wrote, the style the running rdwm reports, the persistent record, the apply's exit status and message.
# Nothing live is touched: private HOME/XDG_*/DECEBAL_CONFIG, a private XDG_RUNTIME_DIR socket, a private Xvfb; WORK_DIR must not exist.
set -eu
DT=$1; RBIN=$2; W=$3; N=$4; MODE=$5
HERE=$(cd "$(dirname "$0")" && pwd); ROOT=$(cd "$HERE/../../../.." && pwd)
EXTRA_ENV="DECEBAL_CONFIG=$W/dcfg DECEBAL_THEMES=$ROOT/themes" sh "$HERE/private-rdwm.sh" "$RBIN" "$W" "$N" \
  "mkdir -p \"\$DECEBAL_CONFIG\"" \
  "\"$DT\" set mode $MODE >/dev/null" \
  "if \"$DT\" theme apply lingmo >\"\$TMPDIR/apply.out\" 2>&1; then echo \"apply exit 0\"; else echo \"apply exit \$?\"; fi; sed 's/^/apply: /' \"\$TMPDIR/apply.out\"" \
  "echo \"config style: \$(grep -E '^style *=' \"\$HOME/.config/rdwm/config.toml\" 2>/dev/null | head -n 1)\"" \
  "echo \"running rdwm styles listed: \$(\"\$RDWMCTL\" schema 2>/dev/null | python3 -I -c 'import json,sys;d=json.load(sys.stdin);k=[x for x in d[\"keys\"] if x[\"section\"]==\"decoration\" and x[\"key\"]==\"style\"];print(len(k[0][\"choices\"]) if k else None)' 2>/dev/null || echo unknown)\"" \
  "echo \"running rdwm style now: \$(\"\$RDWMCTL\" config 2>/dev/null | python3 -I -c 'import json,sys;d=json.load(sys.stdin);print(d.get(\"config\",d).get(\"decoration\",{}).get(\"style\"))' 2>/dev/null || echo unknown)\"" \
  "echo \"record: \$(python3 -I -c 'import json,sys;d=json.load(open(sys.argv[1]));print(json.dumps(d.get(\"decoration_compat\")))' \"\$DECEBAL_CONFIG/decebal.json\" 2>/dev/null || echo none)\""
