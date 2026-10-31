"""Private application registry for the Open suites. Everything lives under the Session's own
XDG_DATA_HOME / XDG_CONFIG_HOME (mkdtemp under /var/tmp, mode 0700): three fixture MIME types,
two fake openers (A is the registered default, B is only associated), one opener whose program
does not exist, and a type with no handler. Nothing here touches the live MIME database,
mimeapps.list, default handlers or applications; the real system registry stays visible only
read-only through the unchanged XDG_DATA_DIRS."""
from pathlib import Path
import hashlib,json,os,subprocess,sys

MIME='application/x-decfiles-%s'
XML='''<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
%s</mime-info>
'''
TYPE='  <mime-type type="%s"><comment>Decfiles %s fixture</comment><glob pattern="*.%s"/></mime-type>\n'

class Registry:
    def __init__(self,session):
        s=self.session=session
        data,config=Path(s.env['XDG_DATA_HOME']),Path(s.env['XDG_CONFIG_HOME'])
        assert str(data).startswith('/var/tmp/') and str(config).startswith('/var/tmp/'),'registry must be private'
        self.data,self.config=data,config
        self.log=s.root/'opener-log';self.log.mkdir()
        self.apps=data/'applications';self.apps.mkdir()
        packages=data/'mime'/'packages';packages.mkdir(parents=True)
        (packages/'decfiles-open-test.xml').write_text(XML%''.join(TYPE%(MIME%kind,kind,ext) for kind,ext in (('open','dfopen'),('fail','dffail'),('none','dfnone'))))
        subprocess.run(['update-mime-database',str(data/'mime')],env=s.env,check=True,capture_output=True,timeout=30)
        fake=Path(__file__).resolve().parent/'fake-opener.py'
        def app(file,name,exec_line,kind):
            (self.apps/file).write_text(f'[Desktop Entry]\nType=Application\nName={name}\nExec={exec_line}\nMimeType={MIME%kind};\nTerminal=false\n')
        recorder=f'{sys.executable} {fake} {self.log}'
        app('decfiles-fake-a.desktop','Decfiles Fake Opener A',recorder+' A %f','open')
        app('decfiles-fake-b.desktop','Decfiles Fake Opener B',recorder+' B %f','open')
        app('decfiles-fake-c.desktop','Decfiles Missing Opener C','/nonexistent-decfiles-opener %f','fail')
        (config/'mimeapps.list').write_text(f'[Default Applications]\n{MIME%"open"}=decfiles-fake-a.desktop;\n{MIME%"fail"}=decfiles-fake-c.desktop;\n')
        self.baseline=self.snapshot()

    def snapshot(self):
        """Hashes of every registry file: opening must never change a default or association."""
        out={}
        files=[self.config/'mimeapps.list']
        for base in (self.data/'applications',self.data/'mime'):files+=[p for p in sorted(base.rglob('*')) if p.is_file()]
        for p in files:out[str(p.relative_to(self.data.parent))]=hashlib.sha256(p.read_bytes()).hexdigest() if p.exists() else None
        return out

    def unchanged(self):
        now=self.snapshot();assert now==self.baseline,{k:(self.baseline.get(k),now.get(k)) for k in set(now)|set(self.baseline) if now.get(k)!=self.baseline.get(k)}

    def records(self):
        return [json.loads(p.read_text()) for p in sorted(self.log.glob('*.json'))]

    @staticmethod
    def argv(record):
        return [bytes.fromhex(h) for h in record['argv_hex']]
