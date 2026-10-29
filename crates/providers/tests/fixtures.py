#!/usr/bin/env python3
"""Run direct adapter fixture tests with a loopback-only network boundary."""
import os
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
root = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(root / 'crates/server/tests'))
from fixture_test_support import enter_loopback_sandbox

enter_loopback_sandbox()
subprocess.run(['cargo', 'test', '-p', 'beweft-providers', '--tests'], cwd=root, env=os.environ, check=True)
