#!/usr/bin/env python3
"""Binary boundary test: suppress automatic retints even with an inherited bus.
The bus is deliberately a fake address and gsettings is a logging fake: no live
session or application can be reached even if the guard regresses.
"""
import json, os, pathlib, signal, subprocess, tempfile, time

binary=os.environ.get('DECTHEMECTL')
if not binary:
    raise SystemExit('Set DECTHEMECTL to the built binary')
with tempfile.TemporaryDirectory(prefix='decebal-automatic-guard-') as scratch:
    root=pathlib.Path(scratch)
    for d in ['home','config/decebal','state','runtime','bin','outside','data','cache','datadirs','catalogs']:(root/d).mkdir(parents=True,exist_ok=True)
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
    env={k:v for k,v in os.environ.items() if k in ('PATH','LANG','LC_ALL')}
    env.update(HOME=str(root/'home'),XDG_CONFIG_HOME=str(root/'config'),XDG_STATE_HOME=str(root/'state'),XDG_DATA_HOME=str(root/'data'),XDG_CACHE_HOME=str(root/'cache'),XDG_DATA_DIRS=str(root/'datadirs'),DECEBAL_ASSET_ROOT=str(root/'data/icons'),DECEBAL_ASSET_CATALOGS=str(root/'catalogs'),DECEBAL_ASSET_STATE=str(root/'state/assets'),XDG_RUNTIME_DIR=str(root/'runtime'),DECEBAL_CONFIG=str(root/'config/decebal'),RDWMCTL='/bin/false',GSETTINGS_BACKEND='memory',DBUS_SESSION_BUS_ADDRESS='unix:path='+str(root/'outside/session-bus'),PROBE_EXTERNAL_LOG=str(log),PATH=str(root/'bin')+os.pathsep+env['PATH'],DECEBAL_SYSTEM_AUTOMATIC='0')
    env.pop('RDWM_SOCK',None)
    # Only GTK is selected; other helpers can never run even if the guard fails.
    config={'asset_packs':{'downloads':'ask'},'system':{'targets':{k:False for k in ['omarchy','browser','editors','hooks','qt','kvantum','x11','terminals']}}}
    (root/'config/decebal/decebal.json').write_text(json.dumps(config))
    def run(*args):return subprocess.check_output([binary,*args],env=env,text=True,timeout=30)
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
    # Exercise real dispatchers and debounced child workers, rather than SYNC=1.
    # Keep each process-group leader unreaped until cleanup so its group id cannot be reused.
    env.pop('DECEBAL_SYSTEM_SYNC')
    leaders=[]
    producer_logs={}
    producer_themes={}
    retried=set()
    try:
        for theme in ('dracula', 'nord'):
            log_path=root/f'burst-{theme}.log'
            output=open(log_path,'w')
            try:
                leaders.append(subprocess.Popen([binary,'apply',theme,'--mode','light','--no-rdwm'],env=env,stdout=output,stderr=subprocess.STDOUT,start_new_session=True))
                producer_logs[leaders[-1].pid]=log_path
                producer_themes[leaders[-1].pid]=theme
            finally:
                output.close()
        deadline=time.monotonic()+20
        for child in leaders:
            status=None
            while time.monotonic()<deadline:
                status=os.waitid(os.P_PID,child.pid,os.WEXITED|os.WNOHANG|os.WNOWAIT)
                if status is not None:
                    break
                time.sleep(.02)
            assert status is not None, 'theme producer did not exit within its deadline'
            if status.si_code!=os.CLD_EXITED or status.si_status!=0:
                text=producer_logs[child.pid].read_text()
                theme=producer_themes[child.pid]
                assert 'changed on disk while saving; nothing was written' in text and theme not in retried, text[-4000:]
                # A concurrent theme transaction is deliberately refused rather than overwriting a newer save.
                # Retry that command once; this creates a second real asynchronous producer without hiding errors.
                retried.add(theme)
                log_path=root/f'burst-{theme}-retry.log'
                with open(log_path,'w') as output:
                    retry=subprocess.Popen([binary,'apply',theme,'--mode','light','--no-rdwm'],env=env,stdout=output,stderr=subprocess.STDOUT,start_new_session=True)
                leaders.append(retry)
                producer_logs[retry.pid]=log_path
                producer_themes[retry.pid]=theme
        palette=json.loads(run('system','palette','--json'))
        state=root/'state/decebal/system'
        while time.monotonic()<deadline:
            queues=[]
            for p in state.glob('requests-*.json'):
                try:
                    queues.append(json.loads(p.read_text()))
                except FileNotFoundError:
                    pass  # a worker completed between glob and read
            result_file=state/'last-result.json'
            result=json.loads(result_file.read_text()) if result_file.exists() else {}
            if not any(q.get('pending') or q.get('running') for q in queues) and result.get('theme')==palette['id']:
                break
            time.sleep(.05)
        else:
            raise AssertionError('asynchronous theme publication failed to settle on the newest saved palette')
        assert result.get('error') is None, result
        css=(root/'config/gtk-4.0/gtk.css').read_text().lower()
        assert palette['bg'].lower() in css, 'the saved palette did not reach GTK output'
        assert result.get('applied')==['gtk'], 'the burst must not enable unrelated app targets'
    finally:
        for child in leaders:
            # Do not call poll/wait before this signal: the unreaped direct child reserves the group id.
            try:
                os.killpg(child.pid,signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait(timeout=5)
    print('PASS: suppressed sync/async requests, explicit apply, normal default, and concurrent real asynchronous producers')
