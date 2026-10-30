#!/usr/bin/env python3
"""Create a separate, loopback-only installation with example data."""
import argparse
import datetime as dt
import errno
import http.cookiejar
import json
import os
from pathlib import Path
import select
import socket
import stat
import struct
import sys
import tempfile
import time
import urllib.error
import urllib.request
import zlib

import self_host as host

EMAIL = 'demo@example.com'
PASSWORD = 'BeweftDemo123!'
MARKER = 'demo-state.json'
MAX_PASSWORD_BYTES = 1024


def save_marker(path, marker, *, create=False):
    if create:
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        name = None
    else:
        fd, name = tempfile.mkstemp(prefix='.demo-state-', dir=path.parent)
    try:
        with os.fdopen(fd, 'w', encoding='utf-8') as file:
            json.dump(marker, file, separators=(',', ':'))
            file.write('\n')
            file.flush()
            os.fsync(file.fileno())
        if name is not None:
            os.replace(name, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if name and os.path.exists(name):
            os.unlink(name)


def read_marker(install):
    path = install.path / MARKER
    try:
        info = path.lstat()
        host.require(stat.S_ISREG(info.st_mode) and info.st_mode & 0o077 == 0 and info.st_size <= 256 * 1024,
                     'Demo marker must be a private regular file.')
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
        with os.fdopen(fd, encoding='utf-8') as file:
            value = json.load(file, object_pairs_hook=host.unique_object)
    except (FileNotFoundError, OSError, ValueError) as error:
        raise host.OperationError('No valid demo marker; refusing to adopt this installation.') from error
    host.require(isinstance(value, dict) and set(value) == {'format', 'installation', 'project', 'port', 'image', 'steps', 'pending', 'complete'}
                 and value['format'] == 1 and type(value['complete']) is bool and isinstance(value['steps'], dict)
                 and (value['pending'] is None or isinstance(value['pending'], str)), 'Invalid demo marker; inspect before proceeding.')
    settings = install.values
    host.require(value['installation'] == settings['BEWEFT_INSTALLATION_ID'] and value['project'] == install.project
                 and value['port'] == settings['BEWEFT_PORT'] and value['image'] == settings['BEWEFT_IMAGE'],
                 'Demo marker does not match the installation identity or image; no action was taken.')
    host.require(value['project'].startswith('beweft-demo') and
                 settings['BEWEFT_BIND_IP'] == '127.0.0.1' and
                 settings['BEWEFT_PUBLIC_URL'] == 'http://localhost:' + value['port'] and
                 value['port'].isdigit() and 1024 <= int(value['port']) <= 65535 and value['port'] != '3000',
                 'Demo must use its dedicated loopback hostname and a non-3000 port.')
    return value


def set_localhost_origin(install, port):
    old = 'BEWEFT_PUBLIC_URL=http://127.0.0.1:' + str(port)
    new = 'BEWEFT_PUBLIC_URL=http://localhost:' + str(port)
    text = install.envfile.read_text(encoding='utf-8')
    host.require(text.splitlines().count(old) == 1, 'Unexpected initial demo origin; no changes made.')
    fd, name = tempfile.mkstemp(prefix='.demo-env-', dir=install.path)
    try:
        with os.fdopen(fd, 'w', encoding='utf-8') as file:
            file.write(text.replace(old, new))
            file.flush()
            os.fsync(file.fileno())
        os.replace(name, install.envfile)
        directory = os.open(install.path, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.exists(name):
            os.unlink(name)
    install.read_settings()


def verify_api_binding(install, port):
    name = install.project + '-api-1'
    host.require(host.named_resource_exists('container', name), 'Demo API is not running; use the documented managed startup, then resume.')
    fields = json.loads(host.output('container', 'inspect', '--format',
        '{{json .NetworkSettings.Ports}}', name, timeout=15))
    host.require(fields.get('3000/tcp') == [{'HostIp': '127.0.0.1', 'HostPort': port}],
                 'Demo API is not bound exclusively to the expected loopback port.')
    host.require(host.output('container', 'inspect', '--format', '{{.Image}}', name, timeout=15) == install.values['BEWEFT_IMAGE'] and
                 host.output('container', 'inspect', '--format', '{{.State.Running}}', name, timeout=15) == 'true',
                 'Demo API image or running state differs from the owned installation.')


def require_free_loopback_port(port):
    # Browsers can prefer IPv6 for localhost even though Compose binds IPv4.
    # Refuse either listener so the demo URL cannot reach another local app.
    for family, address in ((socket.AF_INET, '127.0.0.1'), (socket.AF_INET6, '::1')):
        try:
            with socket.socket(family, socket.SOCK_STREAM) as sock:
                if family == socket.AF_INET6:
                    sock.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
                sock.bind((address, port))
        except OSError as error:
            if family == socket.AF_INET6 and error.errno in (errno.EAFNOSUPPORT, errno.EADDRNOTAVAIL, errno.EPROTONOSUPPORT):
                continue
            raise host.OperationError('The requested loopback port is unavailable; choose another.') from error


class Client:
    def __init__(self, port):
        self.url = 'http://127.0.0.1:' + port
        self.origin = 'http://localhost:' + port
        self.client = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect(),
                                                  urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

    def request(self, path, body=None, method=None, multipart=None):
        headers = {'Host': self.origin[len('http://'):], 'Origin': self.origin, 'X-Beweft-Request': '1'}
        data = None
        if multipart is not None:
            filename, image = multipart
            boundary = 'beweft-demo-image'
            headers['Content-Type'] = 'multipart/form-data; boundary=' + boundary
            data = (f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="{filename}"\r\n'
                    'Content-Type: image/png\r\n\r\n').encode() + image + f'\r\n--{boundary}--\r\n'.encode()
        elif body is not None:
            headers['Content-Type'] = 'application/json'
            data = json.dumps(body).encode()
        request = urllib.request.Request(self.url + '/api/v1' + path, data=data, headers=headers,
                                         method=method or ('POST' if data is not None else 'GET'))
        try:
            with self.client.open(request, timeout=15) as response:
                return json.load(response)['data']
        except urllib.error.HTTPError as error:
            if path == '/auth/login' and error.code == 401:
                raise host.OperationError('Demo login was rejected. If you changed the demo password, pipe the current password into demo resume with --password-stdin.') from None
            raise host.OperationError(f'Demo API request {path} failed (HTTP {error.code}); inspect before retrying.') from None
        except (OSError, urllib.error.URLError, ValueError, KeyError):
            raise host.OperationError(f'Demo API request {path} failed; inspect before retrying.') from None


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def password_from_stdin():
    host.require(not sys.stdin.isatty(), '--password-stdin requires a private non-interactive input stream.')
    fd = sys.stdin.fileno()
    data = bytearray()
    deadline = time.monotonic() + 10
    while len(data) <= MAX_PASSWORD_BYTES + 1:
        remaining = deadline - time.monotonic()
        host.require(remaining > 0 and select.select([fd], [], [], remaining)[0],
                     'Timed out waiting for a password line on stdin.')
        chunk = os.read(fd, MAX_PASSWORD_BYTES + 2 - len(data))
        if not chunk:
            break
        data.extend(chunk)
        if b'\n' in chunk:
            del data[data.index(b'\n') + 1:]
            break
    if data.endswith(b'\n'):
        del data[-1:]
        if data.endswith(b'\r'):
            del data[-1:]
    host.require(0 < len(data) <= MAX_PASSWORD_BYTES, 'Password input must be one non-empty UTF-8 line of at most 1024 bytes.')
    try:
        return data.decode('utf-8')
    except UnicodeDecodeError:
        raise host.OperationError('Password input must be UTF-8.') from None


def card(kind):
    size = 320
    rows = []
    for y in range(size):
        row = bytearray([0])
        for x in range(size):
            if kind == 0:
                distance = (x - 160) ** 2 + (y - 160) ** 2
                color = (119, 70, 47) if distance < 68 ** 2 else (239, 211, 178) if distance < 93 ** 2 else (60, 83, 67)
            elif kind == 1:
                color = (87, 104, 76) if 75 < x < 245 and 100 < y < 220 else (249, 232, 199) if (y // 40) % 2 == 0 else (214, 157, 105)
            else:
                color = ((215, 161, 147) if ((x + y) // 20) % 2 == 0 else (181, 126, 126)) if 45 < x < 275 and 56 < y < 265 else (244, 240, 225)
            row.extend(color)
        rows.append(row)
    def chunk(tag, data):
        return struct.pack('!I', len(data)) + tag + data + struct.pack('!I', zlib.crc32(tag + data) & 0xffffffff)
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('!2I5B', size, size, 8, 2, 0, 0, 0)) +
            chunk(b'IDAT', zlib.compress(b''.join(rows))) + chunk(b'IEND', b''))


class Seed:
    def __init__(self, install, marker):
        self.install, self.marker = install, marker
        self.path = install.path / MARKER
        self.client = Client(marker['port'])

    def action(self, name, path, body=None, method=None, multipart=None):
        if name in self.marker['steps']:
            return self.marker['steps'][name]
        self.marker['pending'] = name
        save_marker(self.path, self.marker)
        result = self.client.request(path, body, method, multipart)
        # Only store the identifiers and version needed by later steps, never a cookie or secret.
        if isinstance(result, list):
            result = result[0]
        saved = {key: result[key] for key in ('id', 'version', 'metadata_version') if key in result}
        self.marker['steps'][name] = saved
        self.marker['pending'] = None
        save_marker(self.path, self.marker)
        return saved

    def run(self, password_stdin=False):
        marker, call, action = self.marker, self.client.request, self.action
        host.require(marker['pending'] is None,
                     'The previous demo write has an uncertain outcome. Inspect the demo and marker; automatic retry is withheld.')
        if marker['complete']:
            print('Demo seed is already complete; no changes made.')
            return
        verify_api_binding(self.install, marker['port'])
        status = call('/auth/status')
        creating_owner = 'owner' not in marker['steps']
        if creating_owner:
            host.require(not password_stdin, '--password-stdin applies after the demo owner exists; resume without it to create the example owner.')
            host.require(status['initialized'] is False, 'Demo already has an owner; refusing to adopt or seed it.')
            action('owner', '/auth/bootstrap', {'setup_secret': self.install.values['BEWEFT_SETUP_SECRET'], 'email': EMAIL, 'password': PASSWORD})
        else:
            host.require(status['initialized'] is True, 'Demo owner is missing; inspect before proceeding.')
            password = password_from_stdin() if password_stdin else PASSWORD
            me = call('/auth/login', {'email': EMAIL, 'password': password})
            host.require(me['email'] == EMAIL, 'Unexpected demo owner; no seed data was written.')
        coffee = action('brand-coffee', '/brands', {'name': 'DEMO · Cloudline Coffee', 'color': '#566B4D'})
        paper = action('brand-paper', '/brands', {'name': 'DEMO · Paperbird Studio', 'color': '#B76E6C'})
        def channel(key, brand, title, platform):
            return action('channel-' + key, '/channels', {'brand_id': brand['id'], 'name': 'DEMO · ' + title + ' (manual practice)',
                          'provider': 'manual', 'target_platform': platform, 'timezone': 'America/Chicago'})
        x = channel('x', coffee, 'Cloudline X', 'x')
        instagram = channel('instagram', coffee, 'Cloudline Instagram', 'instagram')
        youtube = channel('youtube', coffee, 'Cloudline YouTube', 'youtube')
        studio = channel('studio', paper, 'Paperbird Instagram', 'instagram')
        for key, ch, days, hour in [('x', x, [1, 3, 5], '09:00'), ('instagram', instagram, [2, 4, 6], '10:30'),
                                     ('youtube', youtube, [5], '14:00'), ('studio', studio, [1, 4], '11:00')]:
            action('schedule-' + key, '/schedules', {'channel_id': ch['id'], 'days': days, 'time': hour, 'timezone': 'America/Chicago'})
        media = []
        for key, brand, name, kind, alt in [
            ('cup', coffee, 'demo-coffee-cup.png', 0, 'DEMO illustration: a dark coffee circle on a cream and green background.'),
            ('cafe', coffee, 'demo-coffee-card.png', 1, 'DEMO illustration: a green rectangular card against warm horizontal stripes.'),
            ('paper', paper, 'demo-paper-pattern.png', 2, 'DEMO illustration: an abstract rose-colored woven paper pattern.')]:
            item = action('media-' + key, '/media?brand_id=' + brand['id'], multipart=(name, card(kind)))
            action('description-' + key, '/media/' + item['id'], {'alt_text': alt, 'metadata_version': item['metadata_version']}, 'PATCH')
            media.append(item)
        cup, cafe, pattern = media
        coffee_group = action('group-coffee', '/idea-groups', {'brand_id': coffee['id'], 'name': 'DEMO · Cafe stories', 'color': 'lavender'})
        paper_group = action('group-paper', '/idea-groups', {'brand_id': paper['id'], 'name': 'DEMO · Studio notes', 'color': 'lavender'})
        ideas = [
            (coffee, coffee_group, 'A morning ritual', 'Ask the team about a favorite morning coffee ritual.', 'in_progress', ['community', 'coffee']),
            (coffee, coffee_group, 'Meet the beans', 'Share a short origin story for an imaginary seasonal blend.', 'ready', ['coffee', 'story']),
            (coffee, coffee_group, 'Behind the counter', 'A little look at the work behind a welcoming cafe.', 'inbox', ['people']),
            (paper, paper_group, 'Sketchbook tour', 'Turn sketches into an imaginary studio story.', 'ready', ['design', 'process']),
            (paper, paper_group, 'Color of the month', 'Collect a few shades from the studio palette.', 'inbox', ['color'])]
        for index, (brand, group, title, text, state, tags) in enumerate(ideas):
            action('idea-' + str(index), '/ideas', {'brand_id': brand['id'], 'group_id': group['id'], 'title': 'DEMO · ' + title, 'text': text, 'status': state, 'tags': tags})
        action('template-coffee', '/templates', {'brand_id': coffee['id'], 'name': 'DEMO · Weekly cafe update', 'title': 'A note from {{brand}}',
               'text': 'This week at {{brand}}: {{update}}.', 'media_ids': [cafe['id']], 'variants': [{'channel_id': x['id'], 'text': 'A little update from {{brand}}: {{update}}.'}]})
        action('template-paper', '/templates', {'brand_id': paper['id'], 'name': 'DEMO · Studio share', 'title': 'From the studio',
               'text': 'A peek at {{project}} from {{brand}}.', 'media_ids': [pattern['id']]})
        action('draft-coffee', '/posts', {'brand_id': coffee['id'], 'title': 'DEMO · Spring blend announcement',
               'text': 'A little warmth for your morning. Meet our imaginary spring blend.', 'media_ids': [cup['id']],
               'variants': [{'channel_id': x['id'], 'text': 'An imaginary spring blend, made for slow mornings. ☕'},
                            {'channel_id': instagram['id'], 'text': 'Slow mornings, warm cups, and a little spring color. This is an example draft.'}]})
        action('draft-question', '/posts', {'brand_id': coffee['id'], 'title': 'DEMO · Barista question', 'text': 'What makes your favorite cup feel like a ritual? Example copy for practice.'})
        action('draft-paper', '/posts', {'brand_id': paper['id'], 'title': 'DEMO · Paper textures', 'text': 'A small study in texture and color. Example content for practice.',
               'media_ids': [pattern['id']], 'variants': [{'channel_id': studio['id'], 'text': 'A little color and texture from the imaginary studio. Example draft.'}]})
        now = dt.datetime.now(dt.timezone.utc)
        examples = [('queue-x', coffee, x, 'Coffee break', 'DEMO practice: take a moment for a coffee break.', [cup], None),
                    ('queue-instagram', coffee, instagram, 'Behind the counter', 'DEMO practice: a look behind the counter.', [cafe], None),
                    ('queue-paper', paper, studio, 'Palette study', 'DEMO practice: a paper palette study.', [pattern], None),
                    ('exact', coffee, x, 'Weekend reminder', 'DEMO practice: an exact calendar appointment.', [], now + dt.timedelta(days=4, hours=2)),
                    ('waiting', coffee, instagram, 'Manual handoff', 'DEMO example awaiting manual handoff; no public post made.', [], now),
                    ('history', paper, studio, 'Example handoff history', 'DEMO example only; no public post made.', [], now + dt.timedelta(seconds=1))]
        jobs = {}
        for key, brand, ch, title, text, attachments, when in examples:
            post = action('post-' + key, '/posts', {'brand_id': brand['id'], 'title': 'DEMO · ' + title, 'text': text, 'media_ids': [m['id'] for m in attachments]})
            body = {'version': post['version'], 'channel_ids': [ch['id']]}
            if when:
                body['scheduled_at'] = when.isoformat()
            jobs[key] = action('publication-' + key, '/posts/' + post['id'] + '/schedule', body)
        if 'history-completed' not in marker['steps']:
            for _ in range(80):
                publications = {item['id']: item for item in call('/publications')}
                if all(publications.get(jobs[key]['id'], {}).get('state') == 'manual_required' for key in ('waiting', 'history')):
                    break
                time.sleep(.25)
            else:
                raise host.OperationError('Demo worker has not prepared both manual handoffs; inspect and resume.')
            history = publications[jobs['history']['id']]
            host.require(history['channel_id'] == studio['id'] and history['payload']['text'] == 'DEMO example only; no public post made.',
                         'History example changed; refusing to attest to it.')
            action('history-completed', '/publications/' + history['id'] + '/manual-complete',
                   {'note': 'DEMO owner attestation: example only; no public post made.'})
        marker['complete'] = True
        save_marker(self.path, marker)
        print('Demo ready: http://localhost:' + marker['port'])
        if creating_owner:
            print('Initial example login: ' + EMAIL + ' / ' + PASSWORD)
        else:
            print('Demo owner: ' + EMAIL + ' (use its current password).')


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    p = commands.add_parser('create')
    p.add_argument('directory')
    p.add_argument('project')
    p.add_argument('port', type=int)
    p.add_argument('image', help='trusted, already-local application image')
    p.add_argument('--postgres-image', default='postgres:17-alpine')
    p = commands.add_parser('resume')
    p.add_argument('directory')
    p.add_argument('--password-stdin', action='store_true', help='read the current demo password from a private input stream')
    args = parser.parse_args()
    if args.command == 'create':
        host.require(args.project.startswith('beweft-demo'), 'Use a unique Compose project starting with beweft-demo.')
        host.require(1024 <= args.port <= 65535 and args.port != 3000, 'Choose a non-3000 port between 1024 and 65535.')
        # Fail before reserving anything if a listener already occupies the requested port.
        require_free_loopback_port(args.port)
        host.require(host.IMAGE_PATTERN.fullmatch(args.image) is not None, 'Invalid local application image.')
        image_id = host.output('image', 'inspect', '--format', '{{.Id}}', args.image, timeout=15)
        host.require(host.IMAGE_ID.fullmatch(image_id) is not None, 'A trusted local application image is required.')
        host.setup(args.directory, args.project, args.port, args.postgres_image, False)
        install = host.Installation(args.directory)
        with install.locked():
            set_localhost_origin(install, args.port)
            host.select_image(install, image_id)
            marker = {'format': 1, 'installation': install.values['BEWEFT_INSTALLATION_ID'], 'project': install.project,
                      'port': str(args.port), 'image': image_id, 'steps': {}, 'pending': None, 'complete': False}
            save_marker(install.path / MARKER, marker, create=True)
            install.compose('up', '-d', '--wait', '--wait-timeout', '120', '--no-build', '--pull', 'never', capture=False, timeout=150)
            Seed(install, marker).run()
    else:
        install = host.Installation(args.directory)
        # Refuse an unmarked installation before even creating its lock file.
        # Recheck under the lock in case anything changed between reads.
        read_marker(install)
        with install.locked():
            marker = read_marker(install)
            Seed(install, marker).run(password_stdin=args.password_stdin)


if __name__ == '__main__':
    try:
        main()
    except (host.OperationError, OSError, ValueError, KeyError, TypeError) as error:
        print('Demo setup stopped: ' + str(error), file=sys.stderr)
        sys.exit(1)
