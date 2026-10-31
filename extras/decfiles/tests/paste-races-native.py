#!/usr/bin/env python3
"""Fixture-delay binary only: clipboard-read and clipboard-owner races through actual GTK
controls. A capped DECFILES_FIXTURE_CLIPBOARD_DELAY_MS holds the read open so navigation
attempts and owner changes are deterministic. Private Xvfb/bus/HOME/XDG and a private
/dev/shm folder (second filesystem) only."""
import os,shutil,sys,time
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from harness import Session
binary=os.environ.get('DECFILES_FIXTURE_BIN','/var/tmp/decfiles-transfer-artifacts/fixture-delay/decfiles')
out=os.environ.get('DECFILES_PASTE_RACES_EVIDENCE')
cases=[];other=Path('/dev/shm')/f'decfiles-paste-races-{os.getpid()}'
s=Session(binary,out,'decfiles-paste-races-',{'DECFILES_FIXTURE_CLIPBOARD_DELAY_MS':'1500','DECFILES_FIXTURE_TRANSFER_DELAY_MS':'150'})
try:
    base=s.home/'Documents';base.mkdir();(base/'Target').mkdir();(base/'Other').mkdir()
    (base/'note.txt').write_bytes(b'note bytes')
    payload=bytes((i*31)%251 for i in range(12*256*1024));(base/'big.bin').write_bytes(payload)
    other.mkdir();other.chmod(0o700)
    assert os.stat(other).st_dev!=os.stat(base).st_dev,'needs /dev/shm on a different filesystem'
    s.start_x();s.launch(folder=base)
    # Navigation attempts while the clipboard is read are refused; the paste lands where it began.
    s.select_item(base/'note.txt');s.key('ctrl+c');s.address(base/'Target')
    previous=s.last_id();s.button('Paste');s.wait(lambda:s.op().get('paste_reading'),'paste did not start reading')
    s.key('alt+Up');s.key('alt+Left');s.click(92,141)
    time.sleep(.2);assert s.at(base/'Target') and s.op().get('paste_reading'),'navigation was not blocked during the clipboard read'
    assert 'Reading the clipboard' in s.state().get('status',''),s.state().get('status')
    s.finished(previous,'success')
    assert (base/'Target'/'note.txt').read_bytes()==b'note bytes' and not any((base/'Other').iterdir());assert s.at(base/'Target')
    s.cases.append('navigation is blocked while the clipboard is read; paste lands in the original folder')
    # Our own cut, moved across filesystems; another owner takes the clipboard mid-transfer.
    s.address(base);s.select_item(base/'big.bin');s.button('Cut');s.address(other)
    previous=s.last_id();s.button('Paste')
    s.wait(lambda:((s.op().get('transfer') or {}).get('progress') or {}).get('bytes_done',0)>0 and s.op().get('busy'),'cross-device move did not report progress')
    owner=s.offer('text/plain','newer external clipboard text')
    s.wait(lambda:s.op().get('clipboard') is None,'local cut state not dropped after owner change')
    s.finished(previous,'success',40)
    assert (other/'big.bin').read_bytes()==payload and not (base/'big.bin').exists(),'verified cross-device move'
    assert s.clipboard('text/plain')==b'newer external clipboard text','newer clipboard content was cleared'
    owner.terminate();owner.wait()
    s.cases.append('cross-device cut moves verified bytes and never clears a newer external clipboard owner')
    s.finish({'features':['fixture-delay'],'fixture_clipboard_delay_ms':1500,'fixture_transfer_delay_ms':150,'scope':'Controlled fixture delays only'})
    cases+=s.cases
finally:
    s.close()
# A clipboard read that does not finish within the 5 s limit fails and releases navigation.
t=Session(binary,(out+'-timeout') if out else None,'decfiles-paste-timeout-',{'DECFILES_FIXTURE_CLIPBOARD_DELAY_MS':'7000'})
try:
    base=t.home/'Documents';base.mkdir();(base/'Target').mkdir();(base/'note.txt').write_bytes(b'x')
    t.start_x();t.launch(folder=base);t.select_item(base/'note.txt');t.key('ctrl+c');t.address(base/'Target')
    t.button('Paste');t.wait(lambda:t.op().get('paste_reading'),'paste did not start reading')
    t.wait(lambda:not t.op().get('paste_reading'),'clipboard timeout did not release the paste',9)
    assert 'did not respond in time' in t.state().get('status',''),t.state().get('status');assert not any((base/'Target').iterdir())
    t.address(base);t.cases.append('a clipboard read beyond 5 s times out, pastes nothing and releases navigation')
    t.finish({'features':['fixture-delay'],'fixture_clipboard_delay_ms':7000,'scope':'Controlled fixture delay only'})
    cases+=t.cases
finally:
    t.close()
    if other.exists():shutil.rmtree(other)
print('paste races passed:',cases)
