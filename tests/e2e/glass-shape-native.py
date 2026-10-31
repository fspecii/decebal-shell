#!/usr/bin/env python3
"""Run the generic-window glass input/bounds regression on an owned private X server.

Usage: python3 -I tests/e2e/glass-shape-native.py UNIT_TEST_BINARY NEW_OUTPUT_DIR
This checks X Shape state and the blur hint, not compositor blur pixels.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from private_x11 import close, listeners_owned, start_private_xvfb


def main():
    binary = Path(sys.argv[1]).resolve(strict=True)
    out = Path(sys.argv[2]).absolute()
    out.mkdir(mode=0o700)
    home = out / 'home'
    home.mkdir(mode=0o700)
    env = {
        'PATH': '/usr/bin:/bin', 'HOME': str(home), 'LANG': 'C.UTF-8',
        'XDG_CONFIG_HOME': str(home / '.config'),
        'XDG_DATA_HOME': str(home / '.local/share'),
        'XDG_CACHE_HOME': str(home / '.cache'),
    }
    result = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'scope': 'X Shape input/bounding regions and hint; no compositor pixel claim'}
    with (out / 'xvfb.log').open('wb') as log:
        server, display = start_private_xvfb(env=env, log=log)
        try:
            assert listeners_owned(server, int(display[1:]))
            env.update(DISPLAY=display, DECEBAL_GLASS_SHAPE_TEST_DISPLAY=display,
                       DECEBAL_TEST_OWNED_XVFB_PID=str(server.pid))
            argv = [str(binary), 'backend::x11_window::tests::glass_preserves_passive_input_and_disabling_restores_resize_bounds',
                    '--exact', '--ignored', '--nocapture', '--test-threads=1']
            run = subprocess.run(argv, env=env, capture_output=True, text=True, timeout=30)
            (out / 'test.log').write_text(run.stdout + run.stderr)
            result.update(display=display, server_pid=server.pid, exit=run.returncode,
                          passed=run.returncode == 0 and '1 passed' in run.stdout)
        finally:
            close(server)
            result['server_reaped'] = server.poll() is not None
            (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))
    return 0 if result.get('passed') else 1


if __name__ == '__main__':
    raise SystemExit(main())
