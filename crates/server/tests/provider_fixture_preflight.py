#!/usr/bin/env python3
"""Verify fixture mode cannot fall through to real provider endpoints.

Build default binaries and run with `default`, then build with
--features provider-fixtures and run with `feature`. No database or provider
network access is required. Processes run in a loopback-only sandbox.
"""
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
sys.dont_write_bytecode = True
from fixture_test_support import enter_loopback_sandbox, loopback_only
enter_loopback_sandbox()

ROOT = Path(__file__).resolve().parents[3]
TARGET = Path(os.environ.get('CARGO_TARGET_DIR', str(ROOT / 'target'))).resolve()
mode = sys.argv[1] if len(sys.argv) == 2 else ''
assert mode in ('default', 'feature'), 'expected default or feature'

with socket.socket() as listener:
    listener.bind(('127.0.0.1', 0))
    listener.listen()
    script = '''import socket,sys
local = socket.create_connection(('127.0.0.1',int(sys.argv[1])),timeout=1)
local.close()
remote = socket.socket()
remote.settimeout(1)
try:
    remote.connect(('192.0.2.1',9))
except OSError as error:
    sys.exit(0 if error.errno in (1,13) else 3)
sys.exit(2)
'''
    probe = subprocess.run(loopback_only([sys.executable, '-c', script, str(listener.getsockname()[1])]),
                           capture_output=True, text=True, timeout=5)
    assert probe.returncode == 0, ('loopback-only sandbox failed', probe.returncode, probe.stderr)

with tempfile.TemporaryDirectory(prefix='beweft-fixture-preflight-') as temp:
    base = dict(os.environ, BEWEFT_MEDIA_DIR=temp, BEWEFT_WORKER_ID='preflight-worker')
    for name in ('DATABASE_URL', 'BEWEFT_REQUIRE_PROVIDER_FIXTURES',
                 'BEWEFT_ENABLE_FIXTURES', 'BEWEFT_PROVIDER_FIXTURE_ORIGIN'):
        base.pop(name, None)
    cases = [
        ({'BEWEFT_REQUIRE_PROVIDER_FIXTURES':'true', 'BEWEFT_ENABLE_FIXTURES':'true'},
         'fixture-enabled build' if mode == 'default' else 'requires a loopback origin'),
        ({'BEWEFT_REQUIRE_PROVIDER_FIXTURES':'true', 'BEWEFT_ENABLE_FIXTURES':'true',
          'BEWEFT_PROVIDER_FIXTURE_ORIGIN':'http://127.0.0.1:9'},
         'fixture-enabled build' if mode == 'default' else 'environment variable not found'),
        ({'BEWEFT_PROVIDER_FIXTURE_ORIGIN':'http://127.0.0.1:9'},
         'fixture-enabled build' if mode == 'default' else 'requires BEWEFT_ENABLE_FIXTURES=true'),
        ({'BEWEFT_REQUIRE_PROVIDER_FIXTURES':'true', 'BEWEFT_ENABLE_FIXTURES':'true',
          'BEWEFT_PROVIDER_FIXTURE_ORIGIN':'https://example.invalid'},
         'fixture-enabled build' if mode == 'default' else 'explicit loopback HTTP origin'),
    ]
    for binary in ('beweft-server', 'beweft-worker'):
        for values, expected in cases:
            result = subprocess.run(loopback_only([TARGET / 'debug' / binary]), cwd=ROOT,
                                    env={**base, **values}, capture_output=True, text=True, timeout=8)
            assert result.returncode != 0 and expected in result.stderr, (binary, values, result.returncode, result.stderr)
    print(f'PASS {mode} API/worker fixture-required startup and loopback-only network boundary')
