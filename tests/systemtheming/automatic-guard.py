#!/usr/bin/env python3
"""Binary boundary test: suppress automatic retints even with an inherited bus.
The bus is deliberately a fake address and gsettings is a logging fake: no live
session or application can be reached even if the guard regresses.
"""
import json, os, pathlib, subprocess, tempfile, time

binary=os.environ.get('DECTHEMECTL')
if not binary:
    raise SystemExit('Set DECTHEMECTL to the built binary')
with tempfile.TemporaryDirectory(prefix='decebal-automatic-guard-') as scratch:
    root=pathlib.Path(scratch)
    for d in ['home','config/decebal','state','runtime','bin','outside']:(root/d).mkdir(parents=True,exist_ok=True)
    os.chmod(root/'runtime',0o700)
    log=root/'outside/retints.log'
    fake=root/'bin/gsettings'
    fake.write_text('''#!/bin/sh
# Theme preview/icon lookup may read icon-theme independently of app retinting.
if [ "$*" != "get org.gnome.desktop.interface icon-theme" ]; then
  printf '%s\\n' "$*" >> "$PROBE_EXTERNAL_LOG"
fi
case "$1" in
list-schemas) printf '%s\\n' org.gnome.desktop.interface org.gnome.desktop.wm.preferences ;;
get) printf "'default'\\n" ;;
esac
''')
    fake.chmod(0o755)
    env=os.environ.copy()
    env.update(HOME=str(root/'home'),XDG_CONFIG_HOME=str(root/'config'),XDG_STATE_HOME=str(root/'state'),XDG_RUNTIME_DIR=str(root/'runtime'),DECEBAL_CONFIG=str(root/'config/decebal'),RDWMCTL='/bin/false',GSETTINGS_BACKEND='memory',DBUS_SESSION_BUS_ADDRESS='unix:path='+str(root/'outside/session-bus'),PROBE_EXTERNAL_LOG=str(log),PATH=str(root/'bin')+os.pathsep+env['PATH'],DECEBAL_SYSTEM_AUTOMATIC='0')
    env.pop('RDWM_SOCK',None)
    # Only GTK is selected; other helpers can never run even if the guard fails.
    config={'system':{'targets':{k:False for k in ['omarchy','browser','editors','hooks','qt','kvantum','x11','terminals']}}}
    (root/'config/decebal/decebal.json').write_text(json.dumps(config))
    def run(*args):return subprocess.check_output([binary,*args],env=env,text=True)
    for sync in ['0','1']:
        env['DECEBAL_SYSTEM_SYNC']=sync
        run('apply','nord','--mode','dark','--no-rdwm')
        time.sleep(1.1)
        assert not log.exists(), 'suppressed automatic request reached external helper'
        assert not (root/'config/gtk-4.0/gtk.css').exists(), 'automatic guard wrote GTK configuration'
        assert not (root/'state/decebal/system').exists(), 'automatic guard created pending/state or spawned an apply'
    status=json.loads(run('system','status','--json'))
    assert status['enabled'], 'process override incorrectly changed persistent automatic default'
    dry=json.loads(run('apply-system','--only','gtk','--dry-run','--json'))
    assert dry['dry_run'] and 'gtk' in dry['applied'], 'override suppressed explicit app integration command'
    assert not log.exists() and not (root/'config/gtk-4.0/gtk.css').exists()
    # With the override absent, the same absent enabled key still defaults ON.
    # Fake gsettings records the call safely instead of addressing a real bus.
    env.pop('DECEBAL_SYSTEM_AUTOMATIC')
    env['DECEBAL_SYSTEM_SYNC']='1'
    run('apply','nord','--mode','dark','--no-rdwm')
    assert log.exists() and (root/'config/gtk-4.0/gtk.css').exists(), 'normal automatic default changed'
    print('PASS: automatic override suppresses sync/async I/O and helpers; explicit commands and normal default preserved')
