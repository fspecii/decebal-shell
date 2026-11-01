#!/usr/bin/env python3
"""Aggregate completed UKUI shard evidence, without launching any applications."""
import argparse
import importlib.util
import json
from pathlib import Path

spec = importlib.util.spec_from_file_location('ukui', Path(__file__).with_name('openkylin-launcher-native.py'))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('evidence', nargs='+', type=Path, help='native-launcher.json files')
a = p.parse_args()
try:
    print(json.dumps(m.verify_shards([json.loads(path.read_text()) for path in a.evidence]), indent=2))
except (ValueError, KeyError, TypeError, OSError) as error:
    p.exit(1, f'FAIL: {error}\n')
