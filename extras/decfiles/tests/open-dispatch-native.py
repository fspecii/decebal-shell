#!/usr/bin/env python3
"""DRAFT, unrun. Keep real fake-opener completion in flight; verify cross-request dedup/outcomes.
Private display, buses and registry through Session/Registry; fixture-delay binary only.
The delay holds the callback, not dispatch: the fake opener really records the exact path.
"""
import os
import sys
import time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
from harness import Session
from openfix import Registry

binary = os.environ.get('DECFILES_FIXTURE_BIN', '/var/tmp/decfiles-operations-artifacts/fixture-delay/decfiles')
s = Session(binary, os.environ.get('DECFILES_OPEN_DISPATCH_EVIDENCE'), 'decfiles-open-dispatch-',
            {'DECFILES_FIXTURE_LAUNCH_DELAY_MS': '6000'})
opened = lambda: s.state().get('open', {})
last = lambda: opened().get('last') or {}


def launching(path):
    p = opened().get('pending')
    return p if p and p['path'] == str(path) and p['stage'] == 'Launching' else None


try:
    s.start_x()
    reg = Registry(s)
    base = s.home / 'Documents'
    base.mkdir()
    a, b = base / 'a.dfopen', base / 'b.dfopen'
    a.write_text('first')
    b.write_text('second')
    s.launch(folder=base)
    s.select_item(a)
    s.key('Return')
    first = s.wait(lambda: launching(a), 'A never reached actual launch dispatch')
    s.wait(lambda: len(reg.records()) == 1, 'fake opener did not receive A')
    assert reg.argv(reg.records()[0]) == [os.fsencode(a)]
    # Separate callback deadlines so A can complete while B remains pending.
    time.sleep(1)
    s.select_item(b)
    s.key('Return')
    second = s.wait(lambda: launching(b), 'B never reached actual launch dispatch')
    s.wait(lambda: len(reg.records()) == 2, 'fake opener did not receive B')
    assert reg.argv(reg.records()[1]) == [os.fsencode(b)]
    s.select_item(a)
    before = opened()['requests']
    s.key('Return')
    assert opened()['requests'] == before, 'A was dispatched again while its callback was outstanding'
    assert {row['id'] for row in opened()['dispatched']} == {first['id'], second['id']}
    s.wait(lambda: last().get('id') == first['id'] and last().get('status') == 'launched',
           'superseded A completion disappeared', 10)
    assert launching(b), 'A completion consumed the current B request'
    s.wait(lambda: last().get('id') == second['id'] and last().get('status') == 'launched',
           'B completion disappeared', 10)
    assert not opened()['dispatched'] and opened()['pending'] is None
    deadline = time.monotonic() + .5
    while time.monotonic() < deadline:
        assert len(reg.records()) == 2, 'duplicate launch after callback completion'
        time.sleep(.02)
    reg.unchanged()
    s.cases.append('A and B both dispatch once; repeated A is refused while in flight; each completion is published and B survives A completion')
    s.result({'features': ['fixture-delay'], 'scope': 'Private native GTK dispatch/callback overlap with fake handlers; no real apps'})
finally:
    s.close()
