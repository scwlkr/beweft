"""Real-API workflow regressions; invoked inside integration.py's isolated database/processes."""
import base64
import concurrent.futures
import datetime as dt
import http.cookiejar
import json
import subprocess
import time
import urllib.error
import urllib.request


def exercise(call, client, origin, sql):
    brand = call('/brands', {'name': 'Workflow laboratory'})
    other = call('/brands', {'name': 'Separate workflow brand'})
    brand = call(f"/brands/{brand['id']}", {'name': 'Workflow studio', 'color': '#a57bce'}, 'PATCH')
    assert brand['name'] == 'Workflow studio' and brand['archived_at'] is None
    call(f"/brands/{brand['id']}", {'color': 'not-a-color'}, 'PATCH', expected=400)
    platform = next(p for p in call('/manual-platforms') if p['id'] == 'instagram')
    assert platform['compose_url'] == 'https://www.instagram.com'
    channel = call('/channels', {'brand_id': brand['id'], 'name': 'Manual Instagram', 'provider': 'manual', 'target_platform': 'instagram'})
    assert channel['manual_guidance']['id'] == 'instagram'
    call('/channels', {'brand_id': brand['id'], 'name': 'Untrusted URL', 'provider': 'manual', 'target_platform': 'https://evil.example'}, expected=400)
    call('/channels', {'brand_id': brand['id'], 'name': 'Wrong provider', 'provider': 'fixture', 'target_platform': 'x'}, expected=400)
    channel = call(f"/channels/{channel['id']}", {'target_platform': 'x'}, 'PATCH')
    assert channel['manual_guidance']['compose_url'] == 'https://x.com/compose/post'
    group = call('/idea-groups', {'brand_id': brand['id'], 'name': 'Launch notes', 'position': 2})
    foreign = call('/idea-groups', {'brand_id': other['id'], 'name': 'Other group'})
    idea = call('/ideas', {'brand_id': brand['id'], 'title': 'A useful lesson', 'status': 'in_progress', 'group_id': group['id'], 'tags': ['Learning', ' learning ', 'Weekly']})
    assert idea['tags'] == ['Learning', 'Weekly'] and idea['group_id'] == group['id']
    assert call(f"/ideas?brand_id={brand['id']}&status=in_progress&tag=Weekly&group_id={group['id']}")[0]['id'] == idea['id']
    call(f"/ideas/{idea['id']}", {'group_id': foreign['id']}, 'PATCH', expected=400)
    idea = call(f"/ideas/{idea['id']}", {'group_id': None, 'status': 'ready'}, 'PATCH')
    assert idea['group_id'] is None and idea['status'] == 'ready'
    call(f"/ideas/{idea['id']}", {'group_id': group['id']}, 'PATCH')
    call(f"/idea-groups/{group['id']}", {'name': 'Renamed group', 'position': 1}, 'PATCH')
    call(f"/idea-groups/{group['id']}", method='DELETE')
    assert next(i for i in call('/ideas?brand_id=' + brand['id']) if i['id'] == idea['id'])['group_id'] is None
    call('/ideas', {'brand_id': brand['id'], 'title': 'Bad status', 'status': 'made_up'}, expected=422)
    assert len(call('/template-starters')) >= 4
    template = call('/templates', {'brand_id': brand['id'], 'name': 'Reusable update', 'title': '{{brand}} update', 'text': 'Hello {{person}}; {{missing}} stays editable.', 'variants': [{'channel_id': channel['id'], 'text': 'For {{person}}'}]})
    template = call(f"/templates/{template['id']}", {'version': template['version'], 'name': 'Reusable announcement'}, 'PATCH')
    call(f"/templates/{template['id']}", {'version': 1, 'name': 'Stale'}, 'PATCH', expected=409)
    draft = call(f"/templates/{template['id']}/apply", {'variables': {'person': '{{brand}}'}})
    assert draft['title'] == 'Workflow studio update' and draft['text'] == 'Hello {{brand}}; {{missing}} stays editable.'
    assert draft['variants'][0]['text'] == 'For {{brand}}' and draft['status'] == 'draft'
    assert call('/publications?brand_id=' + brand['id']) == [], 'Applying a template must never schedule'
    restricted = call('/tokens', {'name': 'Workflow brand draft access', 'scopes': ['drafts:write'], 'brand_ids': [other['id']]})
    call(f"/templates/{template['id']}/apply", {}, token=restricted['token'], expected=403)
    call('/idea-groups?brand_id=' + brand['id'], token=restricted['token'], expected=403)
    # Per-media descriptions persist independently and are frozen into each scheduled snapshot.
    png = base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=')
    boundary = 'workflow-pixel'
    multipart = f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="workflow.png"\r\nContent-Type: image/png\r\n\r\n'.encode() + png + f'\r\n--{boundary}--\r\n'.encode()
    request = urllib.request.Request(origin + '/api/v1/media?brand_id=' + brand['id'], data=multipart, headers={'Content-Type': 'multipart/form-data; boundary=' + boundary, 'X-Beweft-Request': '1', 'Origin': origin})
    with client.open(request) as response:
        media = json.load(response)['data']
    assert media['metadata_version'] == 1
    assert call(f"/media/{media['id']}")['metadata_version'] == 1
    call(f"/media/{media['id']}", {'alt_text': 'No version'}, 'PATCH', expected=422)
    call(f"/media/{media['id']}", {'alt_text': 'Before\x00After', 'metadata_version': 1}, 'PATCH', expected=400)
    assert call(f"/media/{media['id']}")['metadata_version'] == 1
    media = call(f"/media/{media['id']}", {'alt_text': 'A small white test pixel', 'metadata_version': 1}, 'PATCH')
    assert media['metadata_version'] == 2
    call(f"/media/{media['id']}", {'alt_text': 'Stale overwrite', 'metadata_version': 1}, 'PATCH', expected=409)
    assert call(f"/media/{media['id']}")['alt_text'] == 'A small white test pixel'
    call(f"/media/{media['id']}", token=restricted['token'], expected=403)
    media_template = call('/templates', {'brand_id': brand['id'], 'name': 'Saved media', 'text': '', 'media_ids': [media['id']]})
    call(f"/media/{media['id']}", method='DELETE', expected=409)
    call(f"/templates/{media_template['id']}", method='DELETE')
    post = call('/posts', {'brand_id': brand['id'], 'title': 'First revision', 'text': 'Original content', 'media_ids': [media['id']]})
    when = (dt.datetime.now(dt.timezone.utc) + dt.timedelta(hours=3)).isoformat()
    publication = call(f"/posts/{post['id']}/schedule", {'version': post['version'], 'channel_ids': [channel['id']], 'scheduled_at': when})[0]
    assert publication['payload']['media_metadata'] == [{'id': media['id'], 'alt_text': 'A small white test pixel'}]
    call(f"/media/{media['id']}", {'alt_text': 'Updated description', 'metadata_version': media['metadata_version']}, 'PATCH')
    post = call(f"/posts/{post['id']}", {'version': post['version'], 'text': 'Revised content'}, 'PATCH')
    replacement_body = {'post_version': post['version'], 'publication_post_version': publication['post_version'], 'scheduled_at': publication['scheduled_at']}
    call(f"/publications/{publication['id']}/replace", dict(replacement_body, post_version=1), expected=409)
    assert next(p for p in call('/publications') if p['id'] == publication['id'])['state'] == 'queued'
    replacement = call(f"/publications/{publication['id']}/replace", replacement_body)['publication']
    assert replacement['id'] != publication['id'] and replacement['scheduled_at'] == publication['scheduled_at'] and replacement['scheduling_mode'] == 'exact'
    assert replacement['payload']['text'] == 'Revised content' and replacement['payload']['media_metadata'][0]['alt_text'] == 'Updated description'
    old = next(p for p in call('/publications') if p['id'] == publication['id'])
    assert old['state'] == 'canceled' and old['payload']['text'] == 'Original content' and old['payload']['media_metadata'][0]['alt_text'] == 'A small white test pixel'
    call(f"/publications/{publication['id']}/replace", replacement_body, expected=409)
    # Actual queue priority reuses queue slots and leaves the exact date above unchanged.
    call('/schedules', {'channel_id': channel['id'], 'days': [1, 2, 3, 4, 5, 6, 7], 'time': '08:00', 'timezone': 'UTC'})
    slots = []
    for text in ['Earlier queue slot', 'Later queue slot']:
        item = call('/posts', {'brand_id': brand['id'], 'text': text})
        slots.append(call(f"/posts/{item['id']}/schedule", {'version': item['version'], 'channel_ids': [channel['id']]})[0])
    prioritized = call(f"/publications/{slots[1]['id']}/prioritize", {'scheduled_at': slots[1]['scheduled_at']})
    assert prioritized[0]['id'] == slots[1]['id'] and prioritized[0]['scheduled_at'] == slots[0]['scheduled_at']
    assert next(p for p in call('/publications') if p['id'] == replacement['id'])['scheduled_at'] == replacement['scheduled_at']
    call(f"/publications/{replacement['id']}/prioritize", {'scheduled_at': replacement['scheduled_at']}, expected=409)
    # Archive blocks all claims even if a legacy/external change leaves a channel unpaused.
    brand = call(f"/brands/{brand['id']}", {'archived': True}, 'PATCH')
    assert brand['archived_at'] is not None
    assert brand['id'] not in [b['id'] for b in call('/brands')]
    assert brand['id'] in [b['id'] for b in call('/brands?include_archived=true')]
    call(f"/channels/{channel['id']}", {'paused': False}, 'PATCH', expected=409)
    call(f"/posts/{post['id']}/schedule", {'version': post['version'], 'channel_ids': [channel['id']]}, expected=409)
    call('/channels', {'brand_id': brand['id'], 'name': 'Archived channel', 'provider': 'manual'}, expected=409)
    sql(f"UPDATE channels SET paused=false WHERE id='{channel['id']}'; UPDATE publications SET scheduled_at=now() WHERE id='{replacement['id']}';")
    time.sleep(2.5)
    assert next(p for p in call('/publications') if p['id'] == replacement['id'])['state'] == 'queued'
    sql(f"UPDATE channels SET paused=true WHERE id='{channel['id']}';")
    call(f"/brands/{brand['id']}", {'archived': False}, 'PATCH')
    assert next(c for c in call('/channels?brand_id=' + brand['id']) if c['id'] == channel['id'])['paused'] is True
    call(f"/publications/{replacement['id']}/cancel", {})
    # In-flight records cannot be replaced or hidden through archiving.
    fixture = call('/channels', {'brand_id': brand['id'], 'name': 'In-flight workflow fixture', 'provider': 'fixture'})
    flight = call('/posts', {'brand_id': brand['id'], 'text': 'In-flight immutable content'})
    flight_job = call(f"/posts/{flight['id']}/schedule", {'version': flight['version'], 'channel_ids': [fixture['id']], 'scheduled_at': dt.datetime.now(dt.timezone.utc).isoformat()})[0]
    for _ in range(50):
        current = next(p for p in call('/publications') if p['id'] == flight_job['id'])
        if current['state'] == 'sending':
            break
        time.sleep(0.1)
    assert current['state'] == 'sending'
    call(f"/publications/{flight_job['id']}/replace", {'post_version': flight['version'], 'publication_post_version': flight_job['post_version'], 'scheduled_at': flight_job['scheduled_at']}, expected=409)
    call(f"/brands/{brand['id']}", {'archived': True}, 'PATCH', expected=409)
    # Session controls expose opaque identifiers, revoke selected sessions, and rotate the password atomically.
    def second_session(password):
        other_client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        req = urllib.request.Request(origin + '/api/v1/auth/login', data=json.dumps({'email': 'test@example.com', 'password': password}).encode(), headers={'Content-Type': 'application/json', 'X-Beweft-Request': '1', 'Origin': origin})
        with other_client.open(req) as response:
            assert response.status == 200
        return other_client
    def rejected(other_client):
        try:
            other_client.open(origin + '/api/v1/auth/me')
            raise AssertionError('Revoked session still authenticated')
        except urllib.error.HTTPError as error:
            assert error.code == 401
    another = second_session('Isolated-integration-password!')
    sessions = call('/auth/sessions')
    assert all(set(s) == {'id', 'created_at', 'expires_at', 'current'} for s in sessions)
    other_session = next(s for s in sessions if not s['current'])
    call(f"/auth/sessions/{other_session['id']}", method='DELETE')
    rejected(another)
    another = second_session('Isolated-integration-password!')
    call('/auth/sessions/revoke-others', {})
    rejected(another)
    security_token = call('/tokens', {'name': 'Password does not revoke explicit API tokens', 'scopes': ['admin']})['token']
    call('/auth/sessions', token=security_token, expected=403)
    call('/auth/password', {'current_password': 'Isolated-integration-password!', 'new_password': '🦄🦄🦄🦄'}, expected=400)
    call('/auth/password', {'current_password': 'Wrong password', 'new_password': ' New-integration-password! '}, expected=401)
    # Hold the owner row, queue rotation first, then let the old password finish
    # Argon2 verification and reach session issuance. Release only when both real
    # API requests are blocked in PostgreSQL: no timing assumption about hashing.
    holder_command = sql("SELECT 1").args[:-2] + ["-Atq"]
    holder_command.insert(2, "-i")  # Keep psql stdin open through docker exec.
    holder = subprocess.Popen(holder_command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    def wait_for_owner_lock(predicate):
        for _ in range(200):
            count = int(sql("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND pid<>pg_backend_pid() AND wait_event_type='Lock' AND (" + predicate + ");").stdout.splitlines()[2].strip())
            if count:
                return
            time.sleep(0.05)
        raise AssertionError('Expected authentication request did not reach owner lock')
    def stale_login():
        try:
            return second_session('Isolated-integration-password!')
        except urllib.error.HTTPError as error:
            assert error.code == 401
            return None
    try:
        holder.stdin.write("BEGIN; SELECT id FROM owners FOR UPDATE; SELECT 'LOCKED';\n")
        holder.stdin.flush()
        while holder.stdout.readline().strip() != 'LOCKED':
            assert holder.poll() is None
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            rotation = pool.submit(call, '/auth/password', {'current_password': 'Isolated-integration-password!', 'new_password': ' New-integration-password! '})
            try:
                wait_for_owner_lock("query LIKE 'SELECT id FROM owners WHERE id=% FOR UPDATE' OR query LIKE 'UPDATE owners SET password_hash=%'")
                old_login = pool.submit(stale_login)
                wait_for_owner_lock("query LIKE 'SELECT password_hash FROM owners WHERE id=% FOR UPDATE' OR query LIKE 'INSERT INTO sessions%'")
                assert not rotation.done() and not old_login.done()
            finally:
                holder.stdin.write("COMMIT;\n")
                holder.stdin.flush()
            assert rotation.result(timeout=15)['ok'] is True
            stale_client = old_login.result(timeout=15)
            if stale_client is not None:
                rejected(stale_client)
            assert stale_client is None, 'Old verified password issued a session after rotation'
            assert int(sql('SELECT count(*) FROM sessions;').stdout.splitlines()[2].strip()) == 0
    finally:
        if holder.poll() is None:
            holder.stdin.close()
            holder.wait(timeout=10)
    print('PASS: password rotation fences an already-verified concurrent old-password login')
    call('/auth/me', expected=401)
    call('/auth/login', {'email': 'test@example.com', 'password': 'Isolated-integration-password!'}, expected=401)
    call('/auth/login', {'email': 'test@example.com', 'password': ' New-integration-password! '})
    assert call('/brands', token=security_token)
    current_session = next(s for s in call('/auth/sessions') if s['current'])
    call(f"/auth/sessions/{current_session['id']}", method='DELETE')
    call('/auth/me', expected=401)
    call('/auth/login', {'email': 'test@example.com', 'password': ' New-integration-password! '})
    for _ in range(100):
        if next(p for p in call('/publications') if p['id'] == flight_job['id'])['state'] == 'fixture_complete':
            break
        time.sleep(0.2)
    else:
        raise AssertionError('In-flight fixture did not finish before cleanup')
    print('PASS: idea board/groups/tags, original templates/draft-only substitution, manual allowlist, media alt snapshots, atomic queued replacement/history, queue priority, archived-brand claims, in-flight protection, owner sessions/password')
