#!/usr/bin/env python3
"""Private X11 publisher checks: real bar, no-op apply/undo, isolated rendering.
No live display, bus, shell config or input. Uses copied binaries/private stubs.
"""
from pathlib import Path
import hashlib,json,os,shutil,subprocess,tempfile,time
from Xlib import display,Xatom

ROOT=Path(__file__).resolve().parents[2]
BINS=Path(os.environ.get('SHELL_BIN_DIR',ROOT/'target/release')).resolve()
PROBE=Path(os.environ.get('FRAME_SOURCE_BIN',ROOT/'target/release/examples/frame_source')).resolve()
OUT=Path(os.environ.get('FRAME_THEME_EVIDENCE','/tmp/frame-theme-publisher')).resolve();OUT.mkdir(parents=True,exist_ok=True)


def wait(fn,label):
    until=time.monotonic()+8
    while time.monotonic()<until:
        result=fn()
        if result:return result
        time.sleep(.05)
    raise AssertionError(label)


if __name__=='__main__':
    if not shutil.which('Xvfb'):raise SystemExit(77)
    with tempfile.TemporaryDirectory(prefix='frame-publisher-private-') as temp:
        base=Path(temp);env=dict(os.environ)
        for key in list(env):
            if key.startswith(('XDG_','DBUS_','DECEBAL_','DECSETTINGS_','RDWM','PULSE_')) or key in ['HOME','DISPLAY','WAYLAND_DISPLAY','XAUTHORITY','SESSION_MANAGER','AT_SPI_BUS_ADDRESS']:
                env.pop(key)
        for key,name in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_CONFIG_DIRS','configdirs'),('XDG_DATA_HOME','data'),('XDG_DATA_DIRS','datadirs'),('XDG_CACHE_HOME','cache'),('XDG_STATE_HOME','state'),('XDG_RUNTIME_DIR','run')]:
            p=base/name;p.mkdir(mode=0o700);env[key]=str(p)
        tools=base/'bin';tools.mkdir()
        for name in ['decbar','decthemectl']:shutil.copy2(BINS/name,tools/name)
        for name in ['decdock','decwidgets','decwall','rdwmctl','curl','notify-send','playerctl','wpctl','pactl','brightnessctl','nmcli','bluetoothctl']:
            p=tools/name;p.write_text('#!/bin/sh\nexit 0\n');p.chmod(0o755)
        shell=base/'shell';shell.mkdir();themes=base/'themes';themes.mkdir()
        env.update(DECEBAL_CONFIG=str(shell),DECEBAL_THEMES=str(themes),DECEBAL_SYSTEM_THEMES=str(base/'no-system-themes'),DECEBAL_SYSTEM_AUTOMATIC='0',DECEBAL_BACKEND='x11',RDWM_SOCK=str(base/'no-wm.sock'),DBUS_SESSION_BUS_ADDRESS='unix:path='+str(base/'no-bus'),DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(base/'no-system-bus'),PATH=str(tools)+':/usr/bin:/bin')
        cfg=shell/'decebal.json';cfg.write_text(json.dumps({'theme':{'name':'macos','mode':'dark','palette':'nord','syncRdwm':False},'bar':{'enabled':True},'dock':{'enabled':False},'widgets':{'enabled':False},'system':{'enabled':False}}))
        xr,xw=os.pipe();log=(OUT/'xvfb.log').open('w')
        xvfb=subprocess.Popen(['Xvfb','-displayfd',str(xw),'-screen','0','1280x900x24','-nolisten','tcp'],env=env,pass_fds=[xw],stdout=log,stderr=log);os.close(xw)
        with os.fdopen(xr) as f:env['DISPLAY']=':'+f.readline().strip()
        connection=display.Display(env['DISPLAY']);root=connection.screen().root;atom=connection.intern_atom('_DECEBAL_DESKTOP_FRAME_THEME');bar=None;streams=[]
        def run(*args):return subprocess.check_output(list(map(str,args)),env=env,text=True,stderr=subprocess.DEVNULL,timeout=15).strip()
        def words():
            connection.sync();prop=root.get_full_property(atom,Xatom.CARDINAL)
            return list(prop.value) if prop else None
        def colour():return int(json.loads(run(tools/'decthemectl','get','--json'))['colors']['background'][1:],16)
        def sentinel():root.change_property(atom,Xatom.CARDINAL,32,[1,0x112233]);connection.sync()
        try:
            sentinel();run(tools/'decbar','--app','Fixture','--png',OUT/'offscreen.png');assert words()==[1,0x112233], 'offscreen PNG published a frame colour'
            root.delete_property(atom);connection.sync()
            f=(OUT/'bar.log').open('w');streams.append(f);bar=subprocess.Popen([str(tools/'decbar'),'--app','Fixture'],env=env,stdout=f,stderr=f)
            expected=colour();wait(lambda:words()==[1,expected],'bar startup did not publish')
            doc=json.loads(cfg.read_text());doc['theme']['palette']='gruvbox';cfg.write_text(json.dumps(doc));expected=colour();wait(lambda:words()==[1,expected],'live palette generation did not publish')
            bar.terminate();bar.wait(timeout=4);bar=None
            run(tools/'decthemectl','palette','rose-pine');expected=colour();wait(lambda:words()==[1,expected],'committed palette with no bar did not publish')
            root.delete_property(atom);connection.sync();run(tools/'decthemectl','palette','rose-pine');wait(lambda:words()==[1,expected],'no-op apply did not repair missing property')
            previous=expected;run(tools/'decthemectl','theme','undo');expected=colour();assert expected!=previous;wait(lambda:words()==[1,expected],'undo did not restore publication')
            custom=base/'custom';custom.mkdir();(custom/'private-guard.toml').write_text('name="Private guard"\n[dark]\nbackground="#cc3377"\n')
            private=custom/'private.json';private.write_text(json.dumps({'theme':{'name':'private-guard','mode':'dark'}}))
            sentinel();assert run(PROBE,private,custom)=='#cc3377';assert words()==[1,0x112233],'custom Source leaked publication'
            result={'bar_startup':True,'live_generation':True,'bar_absent_apply':True,'no_op_apply':True,'undo':True,'png_isolated':True,'custom_source_isolated':True,'binaries':{name:hashlib.sha256((BINS/name).read_bytes()).hexdigest() for name in ['decbar','decthemectl']}}
            (OUT/'result.json').write_text(json.dumps(result,indent=2)+'\n');print('PASS real bar/apply/undo frame colour and offscreen/custom Source isolation')
        finally:
            if bar and bar.poll() is None:bar.terminate();bar.wait(timeout=4)
            connection.close();xvfb.terminate();xvfb.wait(timeout=4);log.close()
            for f in streams:f.close()
