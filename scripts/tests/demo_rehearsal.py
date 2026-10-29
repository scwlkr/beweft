#!/usr/bin/env python3
"""Exercise the demo command with isolated Docker resources and a local image.

Set BEWEFT_DEMO_TEST_IMAGE to a trusted local application image. No image is
built or downloaded and no provider account or public network is contacted.
"""
import hashlib
import http.cookiejar
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[2]
IMAGE = os.environ.get('BEWEFT_DEMO_TEST_IMAGE', 'beweft:release-bb359e3')
ENV = {key: value for key, value in os.environ.items() if not key.startswith(('BEWEFT_', 'COMPOSE_')) and key != 'DATABASE_URL'}
IDENT = uuid.uuid4().hex[:12]
PROJECT = 'beweft-demo-test-' + IDENT
OTHER = 'beweft-demo-unmarked-' + IDENT


def run(args, success=True):
    result = subprocess.run([str(x) for x in args], cwd=ROOT, env=ENV, text=True, capture_output=True, timeout=200)
    assert (result.returncode == 0) == success, f'{args[0]} returned {result.returncode}; stderr: {result.stderr[-2000:]}'
    return result


def project(*args, success=True):
    return run([ROOT / 'project', *args], success)


def available_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


class Client:
    def __init__(self, port):
        self.origin = 'http://localhost:' + str(port)
        self.url = 'http://127.0.0.1:' + str(port)
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

    def call(self, path, body=None, method=None, expected=200):
        headers = {'Origin': self.origin, 'Host': self.origin[len('http://'):], 'X-Beweft-Request': '1'}
        if body is not None:
            headers['Content-Type'] = 'application/json'
        req = urllib.request.Request(self.url + '/api/v1' + path, data=None if body is None else json.dumps(body).encode(),
                                     headers=headers, method=method or ('POST' if body is not None else 'GET'))
        try:
            with self.opener.open(req, timeout=15) as response:
                code, data = response.status, response.read()
        except urllib.error.HTTPError as error:
            code, data = error.code, error.read()
        assert code == expected, (path, code, expected)
        return json.loads(data).get('data') if code == 200 else None


def main(temp):
    directory = temp / 'demo'
    other = temp / 'unmarked'
    port = available_port()
    # An interruption before the first idea write leaves a clean checkpoint.
    # Resume must continue without duplicating already-created brands and channels.
    code = '''import sys
sys.path.insert(0, sys.argv[1] + '/scripts')
import demo
original = demo.Seed.action
def interrupted(self, name, *args, **kwargs):
    if name == 'idea-0':
        raise demo.host.OperationError('controlled checkpoint interruption')
    return original(self, name, *args, **kwargs)
demo.Seed.action = interrupted
sys.argv = ['demo.py', 'create', *sys.argv[2:]]
try:
    demo.main()
except demo.host.OperationError as error:
    assert str(error) == 'controlled checkpoint interruption'
    sys.exit(3)
'''
    result = run([sys.executable, '-c', code, ROOT, directory, PROJECT, port, IMAGE], success=False)
    assert result.returncode == 3
    marker_file = directory / 'demo-state.json'
    marker = json.loads(marker_file.read_text())
    assert marker_file.stat().st_mode & 0o077 == 0 and marker['pending'] is None and not marker['complete']
    project('demo', 'resume', directory)
    client = Client(port)
    client.call('/auth/login', {'email': 'demo@example.com', 'password': 'BeweftDemo123!'})
    brands, channels = client.call('/brands'), client.call('/channels')
    assert len(brands) == 2 and len(channels) == 4 and all(c['provider'] == 'manual' for c in channels)
    assert len(client.call('/media')) == 3 and len(client.call('/ideas')) == 5 and len(client.call('/templates')) == 2
    pubs = client.call('/publications')
    assert sorted(p['state'] for p in pubs) == ['manual_required', 'published', 'queued', 'queued', 'queued', 'queued']
    done = next(p for p in pubs if p['state'] == 'published')
    attempts = client.call('/publications/' + done['id'] + '/attempts')
    receipt = next(a['receipt'] for a in attempts if a.get('receipt', {}).get('verification') == 'owner_attestation')
    assert 'no public post made' in receipt['detail']['note']
    assert client.call('/analytics')['availability'] == 'unavailable'
    original_ids = sorted(b['id'] for b in brands)
    client.call('/brands', {'name': 'Practice change'})
    client.call('/auth/password', {'current_password': 'BeweftDemo123!', 'new_password': 'Changed-demo-test-password!'})
    project('demo', 'resume', directory)
    client.call('/auth/login', {'email': 'demo@example.com', 'password': 'BeweftDemo123!'}, expected=401)
    client.call('/auth/login', {'email': 'demo@example.com', 'password': 'Changed-demo-test-password!'})
    after = client.call('/brands')
    assert len(after) == 3 and all(item in [b['id'] for b in after] for item in original_ids)
    # Uncertain writes and mismatched markers stop rather than add or replace data.
    marker = json.loads(marker_file.read_text())
    marker['complete'] = False
    marker['pending'] = 'simulated-unknown-write'
    marker_file.write_text(json.dumps(marker))
    assert 'uncertain outcome' in project('demo', 'resume', directory, success=False).stderr
    assert len(client.call('/brands')) == 3
    marker['pending'] = None
    marker['project'] = 'beweft-demo-wrong-owner'
    marker_file.write_text(json.dumps(marker))
    assert 'does not match' in project('demo', 'resume', directory, success=False).stderr
    assert len(client.call('/brands')) == 3
    project('setup', other, OTHER, available_port())
    before = hashlib.sha256((other / 'installation.env').read_bytes()).digest()
    assert not (other / '.beweft.lock').exists()
    assert 'refusing to adopt' in project('demo', 'resume', other, success=False).stderr
    assert hashlib.sha256((other / 'installation.env').read_bytes()).digest() == before
    assert not (other / '.beweft.lock').exists()
    # Port safety checks happen before a directory or reservation is created.
    blocked = temp / 'blocked'
    assert 'non-3000' in project('demo', 'create', blocked, PROJECT + '-blocked', 3000, IMAGE, success=False).stderr
    assert not blocked.exists()
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        sock.listen()
        assert 'port is unavailable' in project('demo', 'create', blocked, PROJECT + '-blocked', sock.getsockname()[1], IMAGE, success=False).stderr
    assert not blocked.exists()
    if socket.has_ipv6:
        sock = None
        try:
            sock = socket.socket(socket.AF_INET6, socket.SOCK_STREAM)
            sock.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
            sock.bind(('::1', 0))
            sock.listen()
        except OSError:
            if sock is not None:
                sock.close()
        else:
            with sock:
                assert 'port is unavailable' in project('demo', 'create', blocked, PROJECT + '-blocked', sock.getsockname()[1], IMAGE, success=False).stderr
            assert not blocked.exists()
    print('PASS: interrupted checkpoint and resume; isolated manual seed and honest history; changed password/data preserved; uncertain write, foreign marker, unmarked installation and occupied port refused')


if __name__ == '__main__':
    temporary = Path(tempfile.mkdtemp(prefix='beweft-demo-test-'))
    failures = []
    try:
        main(temporary)
    finally:
        for directory, name in ((temporary / 'demo', PROJECT), (temporary / 'unmarked', OTHER)):
            if (directory / 'installation.env').exists():
                try:
                    project('compose', directory, 'down', '--volumes', '--remove-orphans')
                    project('release', directory)
                except Exception:
                    failures.append(name)
        if failures:
            print('Could not remove owned test resources; preserve and inspect ' + str(temporary) + ': ' + ', '.join(failures), file=sys.stderr)
        else:
            shutil.rmtree(temporary)
        if failures:
            sys.exit(1)
