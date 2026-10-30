#!/usr/bin/env bash
# Private native Nautilus profile regression; no Cargo build or live preferences.
# E2E_DISPLAY=:N (N >= 2), RDWM_BIN_DIR, FILE_MANAGER_EVIDENCE_DIR are optional.
set -eu
HERE=$(cd "$(dirname "$0")/../.." && pwd)
if [ "${1:-}" = --private-run ]; then
  [ -n "${FILE_MANAGER_PRIVATE_DIR:-}" ] || exit 2
  pids=()
  cleanup() {
    for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
    for pid in "${pids[@]}"; do wait "$pid" 2>/dev/null || true; done
  }
  trap cleanup EXIT
  Xvfb "$DISPLAY" -screen 0 1600x1000x24 -nolisten tcp >"$FILE_MANAGER_EVIDENCE_DIR/xvfb.log" 2>&1 & pids+=($!)
  for _ in $(seq 1 50); do xdotool getdisplaygeometry >/dev/null 2>&1 && break; sleep .1; done
  xdotool getdisplaygeometry >/dev/null
  "$RDWM_BIN_DIR/rdwm" --backend x11 --config "$XDG_CONFIG_HOME/rdwm/config.toml" >"$FILE_MANAGER_EVIDENCE_DIR/rdwm.log" 2>&1 & pids+=($!)
  sleep .5
  kill -0 "${pids[1]}"
  read -r -a selected_profiles <<< "${FILE_MANAGER_PROFILES:-explorer finder explorer-dark}"
  for profile in "${selected_profiles[@]}"; do
    case "$profile" in explorer|finder|explorer-dark) ;; *) exit 2 ;; esac
    if [[ $profile == explorer-dark ]]; then
      # Start the managed pipeline from a clean private baseline; previous standalone
      # profiles intentionally have no managed marker and must not leak Finder rules.
      : > "$XDG_CONFIG_HOME/gtk-4.0/gtk.css"
      printf '%s\n' '{"theme":{"name":"windows11","mode":"dark","palette":"nord"},"system":{"enabled":true,"fileManagerStyle":"theme"}}' > "$DECEBAL_CONFIG/decebal.json"
      "$DECTHEMECTL_BIN" apply-system --only gtk --json > "$FILE_MANAGER_EVIDENCE_DIR/explorer-dark-apply.json"
      rg 'Files visual profile: explorer' "$XDG_CONFIG_HOME/gtk-4.0/gtk.css" >/dev/null
      python3 - <<'CHECK'
import json,os,pathlib
config=json.loads((pathlib.Path(os.environ['DECEBAL_CONFIG'])/'decebal.json').read_text())
assert config['theme']=={'name':'windows11','mode':'dark','palette':'nord'}
assert config['system']['fileManagerStyle']=='theme'
css=(pathlib.Path(os.environ['XDG_CONFIG_HOME'])/'gtk-4.0/gtk.css').read_text()
assert ': dark */' in css and 'Files visual profile: explorer' in css
CHECK
    else
      cp "$FILE_MANAGER_PRIVATE_DIR/$profile.css" "$XDG_CONFIG_HOME/gtk-4.0/gtk.css"
    fi
    # A fresh bus prevents native services from retaining the previous app's CSS.
    PROFILE="$profile" env -u DBUS_SESSION_BUS_ADDRESS dbus-run-session -- timeout -k 2 45 python3 "$HERE/tests/e2e/file-manager-profiles.py" >"$FILE_MANAGER_EVIDENCE_DIR/$profile.log" 2>&1
    if rg -i 'theme parsing error|css.*(error|warning)' "$FILE_MANAGER_EVIDENCE_DIR/$profile.log"; then exit 1; fi
    rg '^PASS ' "$FILE_MANAGER_EVIDENCE_DIR/$profile.log"
  done
  exit
fi
N=${E2E_DISPLAY:-:339}
[[ "$N" =~ ^:([0-9]{1,4})$ ]] || exit 2
N=":$((10#${BASH_REMATCH[1]}))"
[ "${N#:}" -ge 2 ] || exit 2
[ ! -e "/tmp/.X11-unix/X${N#:}" ] && [ ! -e "/tmp/.X${N#:}-lock" ] || exit 77
for command in rustc python3 dbus-run-session Xvfb xdotool nautilus import rg timeout; do command -v "$command" >/dev/null || exit 77; done
export DECTHEMECTL_BIN=${DECTHEMECTL_BIN:-$HERE/target/release/decthemectl}
[ -x "$DECTHEMECTL_BIN" ] || exit 77
export RDWM_BIN_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
[ -x "$RDWM_BIN_DIR/rdwm" ] || exit 77
export FILE_MANAGER_PRIVATE_DIR
FILE_MANAGER_PRIVATE_DIR=$(mktemp -d /tmp/decebal-files-private.XXXXXX)
trap 'rm -rf "$FILE_MANAGER_PRIVATE_DIR"' EXIT
export FILE_MANAGER_EVIDENCE_DIR=${FILE_MANAGER_EVIDENCE_DIR:-$(mktemp -d /tmp/decebal-files-evidence.XXXXXX)}
mkdir -p "$FILE_MANAGER_EVIDENCE_DIR"
FILE_MANAGER_EVIDENCE_DIR=$(cd "$FILE_MANAGER_EVIDENCE_DIR" && pwd)
# Establish ALL isolation before starting Xvfb, the WM, D-Bus or native GTK.
export HOME="$FILE_MANAGER_PRIVATE_DIR/home" XDG_CONFIG_HOME="$FILE_MANAGER_PRIVATE_DIR/config" XDG_RUNTIME_DIR="$FILE_MANAGER_PRIVATE_DIR/run"
export XDG_CACHE_HOME="$FILE_MANAGER_PRIVATE_DIR/cache" XDG_STATE_HOME="$FILE_MANAGER_PRIVATE_DIR/state" XDG_DATA_HOME="$FILE_MANAGER_PRIVATE_DIR/data"
export DECEBAL_CONFIG="$XDG_CONFIG_HOME/omarchy" DECSETTINGS_CONFIG="$XDG_CONFIG_HOME/rdwm/config.toml"
export DISPLAY="$N" DECEBAL_BACKEND=x11 DECEBAL_SYSTEM_AUTOMATIC=0 GSETTINGS_BACKEND=memory GTK_A11Y=none NO_AT_BRIDGE=1
export DBUS_SYSTEM_BUS_ADDRESS=unix:path=/nonexistent
unset DBUS_SESSION_BUS_ADDRESS DBUS_STARTER_ADDRESS DBUS_STARTER_BUS_TYPE WAYLAND_DISPLAY RDWM_SOCK GTK_THEME GTK_PATH GTK_MODULES XAUTHORITY
mkdir -p "$HOME/Example folder/Child folder" "$XDG_CONFIG_HOME/gtk-4.0" "$XDG_CONFIG_HOME/rdwm" "$DECEBAL_CONFIG" "$XDG_RUNTIME_DIR" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME"
chmod 700 "$XDG_RUNTIME_DIR"
cat > "$XDG_CONFIG_HOME/rdwm/config.toml" <<'TOML'
[general]
layouts = ["floating"]
[decoration]
enabled = true
TOML
for n in 'Read me.txt' 'Project brief.txt' 'Design notes.txt' 'Meeting notes.txt' 'Tasks.txt' 'Welcome.txt'; do printf 'Private native fixture.\n' > "$HOME/Example folder/$n"; done
python3 - "$HERE" "$FILE_MANAGER_PRIVATE_DIR" <<'PY'
import json,pathlib,sys
root,temp=map(pathlib.Path,sys.argv[1:])
source='#[path='+json.dumps(str(root/'src/theme/system/file_manager.rs'))+'] mod file_manager;\n'
source+='fn main() { let dir=std::env::args().nth(1).unwrap(); for (theme,want) in [("windows11","explorer"),("macos","finder")] { let style=file_manager::Style::FollowTheme.resolve(theme); assert_eq!(style.id(),want); std::fs::write(format!("{}/{}.css",dir,style.id()),file_manager::css(style)).unwrap(); } }\n'
(temp/'generate.rs').write_text(source)
PY
rustc --edition=2021 -A dead_code "$FILE_MANAGER_PRIVATE_DIR/generate.rs" -o "$FILE_MANAGER_PRIVATE_DIR/generate"
"$FILE_MANAGER_PRIVATE_DIR/generate" "$FILE_MANAGER_PRIVATE_DIR"
env -u DBUS_SESSION_BUS_ADDRESS dbus-run-session -- bash "$HERE/tests/e2e/file-manager-profiles.sh" --private-run
printf 'Evidence: %s\n' "$FILE_MANAGER_EVIDENCE_DIR"
