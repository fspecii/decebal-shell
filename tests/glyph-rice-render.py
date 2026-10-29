#!/usr/bin/env python3
"""Compatibility wrapper: render all six real Glyph cards with private config.
Evidence now lives in docs/theme-ports/glyph-rice; requires Pillow.
"""
import os
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='glyph-six-render-') as tmp:
    env = dict(os.environ, DECEBAL_CONFIG=tmp, DECEBAL_SYSTEM_AUTOMATIC='0',
               DECEBAL_THEME_EVIDENCE=str(root / 'docs/theme-ports'))
    subprocess.run(['cargo', 'test', '--locked', '--offline', '--test', 'theme_ports',
                    'native_port_widgets_fit_and_keep_absent_sources_honest'], cwd=root, env=env, check=True)
subprocess.run(['python', str(root / 'tests/theme-ports-compose.py'), 'glyph-rice'], cwd=root, check=True)
