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
CHANGED_PASSWORD = '  Changed demo password with spaces  '
FINAL_PASSWORD = 'Changed-demo-test-password!'


def run(args, success=True, input=None):
    result = subprocess.run([str(x) for x in args], cwd=ROOT, env=ENV, text=True, input=input, capture_output=True, timeout=200)
    diagnostic = result.stderr[-2000:].replace(CHANGED_PASSWORD, '[redacted]').replace(FINAL_PASSWORD, '[redacted]')
    assert (result.returncode == 0) == success, f'{args[0]} returned {result.returncode}; stderr: {diagnostic}'
    return result


def project(*args, success=True, input=None):
    return run([ROOT / 'project', *args], success, input)


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
sys.argv = ['demo.py', *sys.argv[2:]]
try:
    demo.main()
except demo.host.OperationError as error:
    assert str(error) == 'controlled checkpoint interruption'
    sys.exit(3)
'''
    result = run([sys.executable, '-c', code, ROOT, 'create', directory, PROJECT, port, IMAGE], success=False)
    assert result.returncode == 3
    marker_file = directory / 'demo-state.json'
    marker = json.loads(marker_file.read_text())
    assert marker_file.stat().st_mode & 0o077 == 0 and marker['pending'] is None and not marker['complete']
    # The unchanged default password remains usable on a later resume.
    assert run([sys.executable, '-c', code, ROOT, 'resume', directory], success=False).returncode == 3
    client = Client(port)
    client.call('/auth/login', {'email': 'demo@example.com', 'password': 'BeweftDemo123!'})
    original_ids = sorted(b['id'] for b in client.call('/brands'))
    assert len(original_ids) == 2 and len(client.call('/channels')) == 4
    added = client.call('/brands', {'name': 'Practice change before resume'})
    client.call('/auth/password', {'current_password': 'BeweftDemo123!', 'new_password': CHANGED_PASSWORD})
    marker_before = marker_file.read_bytes()
    failed = project('demo', 'resume', directory, success=False)
    assert '--password-stdin' in failed.stderr and marker_file.read_bytes() == marker_before
    client.call('/auth/login', {'email': 'demo@example.com', 'password': CHANGED_PASSWORD})
    assert sorted(b['id'] for b in client.call('/brands')) == sorted(original_ids + [added['id']])
    empty = project('demo', 'resume', directory, '--password-stdin', input='', success=False)
    assert 'non-empty' in empty.stderr and marker_file.read_bytes() == marker_before
    oversized = project('demo', 'resume', directory, '--password-stdin', input='x' * 1025 + '\n', success=False)
    assert '1024 bytes' in oversized.stderr and marker_file.read_bytes() == marker_before
    stalled = subprocess.Popen([str(ROOT / 'project'), 'demo', 'resume', str(directory), '--password-stdin'],
                               cwd=ROOT, env=ENV, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        assert stalled.wait(timeout=15) != 0
        assert b'Timed out' in stalled.stderr.read()
        assert marker_file.read_bytes() == marker_before
    finally:
        if stalled.poll() is None:
            stalled.kill()
            stalled.wait()
        for stream in (stalled.stdin, stalled.stdout, stalled.stderr):
            stream.close()
    resumed = project('demo', 'resume', directory, '--password-stdin', input=CHANGED_PASSWORD + '\n')
    assert CHANGED_PASSWORD not in resumed.stdout + resumed.stderr
    assert CHANGED_PASSWORD not in marker_file.read_text()
    assert CHANGED_PASSWORD not in (directory / 'installation.env').read_text()
    assert CHANGED_PASSWORD not in (directory / 'installation-state.json').read_text()
    client.call('/auth/login', {'email': 'demo@example.com', 'password': 'BeweftDemo123!'}, expected=401)
    client.call('/auth/login', {'email': 'demo@example.com', 'password': CHANGED_PASSWORD})
    brands, channels = client.call('/brands'), client.call('/channels')
    assert sorted(b['id'] for b in brands) == sorted(original_ids + [added['id']])
    assert len(channels) == 4 and all(c['provider'] == 'manual' for c in channels)
    assert len(client.call('/media')) == 3 and len(client.call('/ideas')) == 5 and len(client.call('/templates')) == 2
    pubs = client.call('/publications')
    assert sorted(p['state'] for p in pubs) == ['manual_required', 'published', 'queued', 'queued', 'queued', 'queued']
    done = next(p for p in pubs if p['state'] == 'published')
    attempts = client.call('/publications/' + done['id'] + '/attempts')
    receipt = next(a['receipt'] for a in attempts if a.get('receipt', {}).get('verification') == 'owner_attestation')
    assert 'no public post made' in receipt['detail']['note']
    assert client.call('/analytics')['availability'] == 'unavailable'
    client.call('/brands', {'name': 'Practice change after resume'})
    client.call('/auth/password', {'current_password': CHANGED_PASSWORD, 'new_password': FINAL_PASSWORD})
    project('demo', 'resume', directory)
    # Even an explicitly requested stdin credential is never read after completion.
    completed = subprocess.Popen([str(ROOT / 'project'), 'demo', 'resume', str(directory), '--password-stdin'],
                                 cwd=ROOT, env=ENV, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        assert completed.wait(timeout=5) == 0
    finally:
        if completed.poll() is None:
            completed.kill()
            completed.wait()
        for stream in (completed.stdin, completed.stdout, completed.stderr):
            stream.close()
    client.call('/auth/login', {'email': 'demo@example.com', 'password': CHANGED_PASSWORD}, expected=401)
    client.call('/auth/login', {'email': 'demo@example.com', 'password': FINAL_PASSWORD})
    after = client.call('/brands')
    assert len(after) == 4 and all(item in [b['id'] for b in after] for item in original_ids + [added['id']])
    # Uncertain writes and mismatched markers stop rather than add or replace data.
    marker = json.loads(marker_file.read_text())
    marker['complete'] = False
    marker['pending'] = 'simulated-unknown-write'
    marker_file.write_text(json.dumps(marker))
    assert 'uncertain outcome' in project('demo', 'resume', directory, success=False).stderr
    assert len(client.call('/brands')) == 4
    marker['pending'] = None
    marker['project'] = 'beweft-demo-wrong-owner'
    marker_file.write_text(json.dumps(marker))
    assert 'does not match' in project('demo', 'resume', directory, success=False).stderr
    assert len(client.call('/brands')) == 4
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
    print('PASS: interrupted seed resumed with privately supplied changed password; spaces and owner data preserved; completed resume does not read stdin; manual seed/history, uncertain writes, ownership and loopback port guards')


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
