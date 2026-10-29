#!/usr/bin/env python3
"""Run real DB provider fixture regressions in an isolated disposable database."""
import os
from pathlib import Path
import subprocess
import sys
import uuid
sys.dont_write_bytecode = True
from fixture_test_support import enter_loopback_sandbox, loopback_only
enter_loopback_sandbox(allow_docker=True)
root = Path(__file__).resolve().parents[3]
container = os.environ.get('BEWEFT_TEST_POSTGRES_CONTAINER', 'beweft-dev-postgres')
user = os.environ.get('BEWEFT_TEST_DB_USER', 'beweft')
password = os.environ.get('BEWEFT_TEST_DB_PASSWORD', 'beweft-local-development')
port = os.environ.get('BEWEFT_TEST_DB_PORT', '55439')
db = 'beweft_provider_test_' + uuid.uuid4().hex[:12]
created = False
try:
    subprocess.run(['docker', 'exec', container, 'createdb', '-U', user, db], check=True)
    created = True
    env = dict(os.environ, BEWEFT_REQUIRE_PROVIDER_FIXTURES='true', BEWEFT_TEST_DATABASE_URL=f'postgres://{user}:{password}@localhost:{port}/{db}')
    subprocess.run(loopback_only(['cargo', 'test', '-p', 'beweft-server', '--features', 'provider-fixtures', '--test', 'provider_integration', '--', '--ignored', '--nocapture', '--test-threads=1']), cwd=root, env=env, check=True)
finally:
    if created:
        subprocess.run(['docker', 'exec', container, 'dropdb', '-U', user, '--force', db], check=True)
