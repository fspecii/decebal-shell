#!/usr/bin/env bash
# Reproduce Terminal Minimal CLI/package resolution without any live session.
# Captures built-in-only and full-package paths; never builds or installs.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "$0")/../.." && pwd)
BIN=${TERMINAL_THEME_BIN:-$HOME/.local/share/rdwm-decebal/bin/decthemectl}
export TERMINAL_THEME_REPO="$HERE" TERMINAL_THEME_BIN="$BIN"
if [ "${TERMINAL_THEME_NATIVE:-0}" = 1 ]; then
  exec /usr/bin/python3 "$HERE/tests/e2e/theme-apply-terminal-native.py"
fi
exec /usr/bin/python3 - <<'PY'
import hashlib,json,os,shutil,subprocess,tempfile
from pathlib import Path
repo=Path(os.environ['TERMINAL_THEME_REPO']);binary=Path(os.environ['TERMINAL_THEME_BIN']).resolve()
if not binary.is_file(): raise SystemExit('Missing decthemectl: '+str(binary))
with tempfile.TemporaryDirectory(prefix='decebal-terminal-apply.') as temp:
 root=Path(temp);env=dict(os.environ)
 for key in list(env):
  if key in ('DISPLAY','WAYLAND_DISPLAY','RDWM_SOCK','SESSION_MANAGER','AT_SPI_BUS_ADDRESS') or key.startswith(('DECBAR_DEBUG_','DECDOCK_DEBUG_')): env.pop(key,None)
 for key,name in {'HOME':'home','XDG_CONFIG_HOME':'cfg','XDG_CONFIG_DIRS':'noconfig','XDG_STATE_HOME':'state','XDG_CACHE_HOME':'cache','XDG_DATA_HOME':'data','XDG_DATA_DIRS':'nodata','XDG_RUNTIME_DIR':'run','DECEBAL_CONFIG':'cfg/omarchy','DECEBAL_THEMES':'themes','DECEBAL_SYSTEM_THEMES':'systemthemes'}.items():
  path=root/name;path.mkdir(parents=True,exist_ok=True);env[key]=str(path)
 os.chmod(env['XDG_RUNTIME_DIR'],0o700)
 env.update(DBUS_SESSION_BUS_ADDRESS='unix:path='+temp+'/missing-session',DBUS_SYSTEM_BUS_ADDRESS='unix:path='+temp+'/missing-system',DECEBAL_SYSTEM_AUTOMATIC='0',DECSETTINGS_CONFIG=temp+'/rdwm.toml',RDWM_CONFIG=temp+'/rdwm.toml',RDWMCTL='/nonexistent/rdwmctl')
 Path(env['DECSETTINGS_CONFIG']).write_text('[general]\n')
 def run(*args,ok=True):
  result=subprocess.run([str(binary),*args],env=env,text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=20)
  print('COMMAND',*args,'EXIT',result.returncode)
  if args[0] != 'export': print(result.stdout.strip())
  if result.stderr: print('STDERR:',result.stderr.strip())
  if ok: assert result.returncode==0
  return result
 print('BINARY',binary,'SHA256',hashlib.sha256(binary.read_bytes()).hexdigest())
 exported=run('export','terminal-minimal')
 assert exported.stdout==(repo/'themes/terminal-minimal/theme.toml').read_text(), 'installed manifest differs from current source'
 run('theme','validate',str(repo/'themes/terminal-minimal'))
 run('theme','apply','terminal-minimal','--no-rdwm','--keep-widgets')
 settings=Path(env['DECEBAL_CONFIG'])/'decebal.json'
 initial=json.loads(settings.read_text())
 print('BUILTIN_ONLY_WIDGET_ITEMS',initial.get('widgets',{}).get('items',[]))
 assert initial['theme']['name']=='terminal-minimal'
 run('apply','terminal-minimal','--no-rdwm')
 shutil.copytree(repo/'themes/terminal-minimal',Path(env['DECEBAL_THEMES'])/'terminal-minimal')
 settings.write_text(json.dumps({'widgets':{'enabled':False,'items':[{'use':'builtin:clock-card','x':123}]}}))
 ask=run('theme','apply','terminal-minimal','--no-rdwm',ok=False)
 assert ask.returncode!=0 and 'custom layout' in ask.stderr, 'custom-layout confirmation not enforced'
 run('theme','apply','terminal-minimal','--no-rdwm','--keep-widgets')
 assert json.loads(settings.read_text())['widgets']['items'][0]['use']=='builtin:clock-card'
 run('theme','apply','terminal-minimal','--no-rdwm','--replace-widgets')
 replacement=json.loads(settings.read_text())['widgets']['items']
 assert replacement and replacement[0]['use']=='tty-clock' and replacement[0]['source']=='terminal-minimal'
 print('PASS Terminal Minimal validates/applies; full package supplies tty-clock; custom layout choice works. Built-in-only widget inventory above checks deployment completeness.')
PY
