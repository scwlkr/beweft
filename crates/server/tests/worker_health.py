#!/usr/bin/env python3
"""Exercise real worker processes against a uniquely named disposable database.

Build: cargo build --locked -p beweft-server --bin beweft-worker
Run: python3 crates/server/tests/worker_health.py
Only the configured beweft PostgreSQL container is used; this script removes
its own database, processes and temporary files, and never contacts providers.
"""
import json
import os
from pathlib import Path
import signal
import socket
import struct
import subprocess
import tempfile
import threading
import time
import urllib.parse
import uuid

ROOT = Path(__file__).resolve().parents[3]
BIN = Path(os.environ.get("BEWEFT_TEST_BIN_DIR", ROOT / "target/debug"))
CONTAINER = os.environ.get("BEWEFT_TEST_POSTGRES_CONTAINER", "beweft-dev-postgres")
DB_USER = os.environ.get("BEWEFT_TEST_DB_USER", "beweft")
DB_PASSWORD = os.environ.get("BEWEFT_TEST_DB_PASSWORD", "beweft-local-development")
DB_PORT = os.environ.get("BEWEFT_TEST_DB_PORT", "55439")
RUN_ID = uuid.uuid4().hex[:12]
DB = "beweft_worker_health_test_" + RUN_ID
DATABASE_URL = f"postgres://{DB_USER}:{DB_PASSWORD}@127.0.0.1:{DB_PORT}/{DB}"


def sql(statement):
    result = subprocess.run(["docker", "exec", "-i", CONTAINER, "psql", "-qAt", "-U", DB_USER,
                             "-d", DB, "-v", "ON_ERROR_STOP=1"], input=statement,
                            text=True, capture_output=True, check=True, timeout=10)
    return result.stdout.strip()


def wait_until(predicate, detail, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.1)
    raise AssertionError(detail)


def stop(process):
    if process.poll() is None:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)


def check(env, *args, success, timeout=6):
    result = subprocess.run([str(BIN / "beweft-worker"), *args], cwd=ROOT, env=env,
                            capture_output=True, text=True, timeout=timeout)
    assert (result.returncode == 0) == success, (args, result.returncode, result.stderr)
    assert DB_PASSWORD not in result.stdout + result.stderr
    return result


def connection_deadline(env):
    """Accept a connection but never complete PostgreSQL negotiation."""
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    listener.settimeout(5)
    accepted, release = threading.Event(), threading.Event()
    def hold_connection():
        try:
            conn, _ = listener.accept()
            with conn:
                accepted.set()
                release.wait(6)
        finally:
            listener.close()
    thread = threading.Thread(target=hold_connection, daemon=True)
    thread.start()
    fake_password = "isolated-health-secret-marker"
    stalled = dict(env, DATABASE_URL=f"postgres://fixture:{fake_password}@127.0.0.1:{listener.getsockname()[1]}/fixture")
    started = time.monotonic()
    try:
        result = check(stalled, "--healthcheck", success=False)
        elapsed = time.monotonic() - started
        assert accepted.is_set() and elapsed < 5 and "timed out" in result.stderr.lower(), (elapsed, result.stderr)
        assert fake_password not in result.stdout + result.stderr
    finally:
        release.set()
        thread.join(timeout=6)
        assert not thread.is_alive(), "Stalled connection fixture did not stop"


def query_deadline(env):
    """Block only this fixture's heartbeat table and bound the read query."""
    app_name = "beweft-health-lock-" + RUN_ID
    locker = subprocess.Popen(["docker", "exec", "-e", "PGAPPNAME=" + app_name, CONTAINER,
                               "psql", "-qAt", "-U", DB_USER, "-d", DB,
                               "-v", "ON_ERROR_STOP=1", "-c",
                               "BEGIN; LOCK TABLE worker_heartbeats IN ACCESS EXCLUSIVE MODE; SELECT pg_sleep(8); COMMIT;"],
                              stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    try:
        query = ("SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid "
                 "WHERE a.application_name='" + app_name + "' AND l.relation='worker_heartbeats'::regclass "
                 "AND l.mode='AccessExclusiveLock' AND l.granted")
        wait_until(lambda: sql(query) == "1", "Fixture did not acquire its table lock")
        started = time.monotonic()
        result = check(env, "--healthcheck", success=False)
        elapsed = time.monotonic() - started
        assert elapsed < 5 and "timed out" in result.stderr.lower(), (elapsed, result.stderr)
    finally:
        locker.wait(timeout=12)
        errors = locker.stderr.read()
        locker.stderr.close()
        assert locker.returncode == 0, errors


def tls_certificate_deadline(env, temp):
    """A configured TLS CA FIFO cannot outlive the healthcheck deadline."""
    fifo = temp / "tls-ca-fifo"
    os.mkfifo(fifo, 0o600)
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    listener.settimeout(5)
    port = listener.getsockname()[1]
    observed, release = threading.Event(), threading.Event()
    observations = []
    def fake_postgres():
        try:
            conn, _ = listener.accept()
            with conn:
                conn.settimeout(5)
                request = b""
                while len(request) < 8:
                    chunk = conn.recv(8 - len(request))
                    if not chunk:
                        raise EOFError("Client closed before SSLRequest")
                    request += chunk
                assert struct.unpack("!II", request) == (8, 80877103)
                conn.sendall(b"S")
                conn.settimeout(1)
                try:
                    observations.append(conn.recv(1))
                except socket.timeout:
                    observations.append(None)  # CA loading has not sent ClientHello.
                observed.set()
                release.wait(6)
        except Exception as error:
            observations.append(error)
            observed.set()
        finally:
            listener.close()
    thread = threading.Thread(target=fake_postgres, daemon=True)
    thread.start()
    query = urllib.parse.urlencode({"sslmode": "verify-full", "sslrootcert": str(fifo)})
    url = f"postgres://fixture:isolated-tls-password@127.0.0.1:{port}/fixture?{query}"
    started = time.monotonic()
    try:
        result = check(dict(env, DATABASE_URL=url), "--healthcheck", success=False)
        elapsed = time.monotonic() - started
        assert observed.wait(2) and observations == [None], observations
        assert elapsed < 5 and "timed out" in result.stderr.lower(), (elapsed, result.stderr)
        assert "isolated-tls-password" not in result.stderr
    finally:
        release.set()
        thread.join(timeout=6)
        fifo.unlink()
        assert not thread.is_alive(), "TLS fixture did not stop"


def compose_contract(clean_env):
    fixture_env = dict(clean_env, BEWEFT_DB_PASSWORD="isolated-compose-password",
                       BEWEFT_SETUP_SECRET="isolated-compose-setup", BEWEFT_ENCRYPTION_KEY="1" * 64)
    result = subprocess.run(["docker", "compose", "--env-file", "/dev/null", "-p", "beweft-health-config-" + RUN_ID,
                             "-f", str(ROOT / "compose.yaml"), "config", "--format", "json"], cwd=ROOT,
                            env=fixture_env, capture_output=True, text=True, check=True, timeout=15)
    worker = json.loads(result.stdout)["services"]["worker"]
    assert worker["environment"]["BEWEFT_WORKER_ID"] == "compose-primary"
    assert worker["healthcheck"]["test"] == ["CMD", "beweft-worker", "--healthcheck"]
    assert worker["healthcheck"]["timeout"] == "5s"


def postgres_environment_isolation(env, temp):
    """A passwordless URL must never read ambient PG credentials or files."""
    fifo = temp / "pgpass-fifo"
    os.mkfifo(fifo, 0o600)
    try:
        hostile = dict(env, PGPASSFILE=str(fifo), PGPASSWORD="wrong-ambient-password",
                       PGHOST="unrelated.invalid", PGHOSTADDR="127.0.0.2", PGPORT="1",
                       PGUSER="unrelated-user", PGDATABASE="unrelated-database",
                       PGOPTIONS="-c search_path=unrelated,public", PGSSLMODE="verify-full",
                       PGSSLROOTCERT=str(fifo), PGSSLCERT=str(fifo), PGSSLKEY=str(fifo),
                       PGAPPNAME="unrelated-application")
        started = time.monotonic()
        check(hostile, "--healthcheck", success=True)
        assert time.monotonic() - started < 5

        # A FIFO without a writer reproduces SQLx's former synchronous pgpass
        # hang. Accept success only if the test server permits passwordless auth.
        no_password = f"postgres://{DB_USER}@127.0.0.1:{DB_PORT}/{DB}"
        no_pass_env = dict(env, DATABASE_URL=no_password, PGPASSFILE=str(fifo))
        started = time.monotonic()
        result = subprocess.run([str(BIN / "beweft-worker"), "--healthcheck"], cwd=ROOT,
                                env=no_pass_env, capture_output=True, text=True, timeout=5)
        assert time.monotonic() - started < 5 and result.returncode in (0, 1)
        assert DB_PASSWORD not in result.stdout + result.stderr
        with_ambient_password = dict(no_pass_env, PGPASSWORD=DB_PASSWORD)
        other = subprocess.run([str(BIN / "beweft-worker"), "--healthcheck"], cwd=ROOT,
                               env=with_ambient_password, capture_output=True, text=True, timeout=5)
        assert other.returncode in (0, 1)
        assert (other.returncode == 0) == (result.returncode == 0), "Ambient PGPASSWORD changed authentication"

        # Explicit query-form connection details retain normal URL support.
        query = urllib.parse.urlencode({"host": "127.0.0.1", "port": DB_PORT, "user": DB_USER,
                                        "password": DB_PASSWORD, "dbname": DB, "sslmode": "disable"})
        check(dict(hostile, DATABASE_URL="postgres:///?" + query),
              "--healthcheck", success=True)
        for incomplete in (f"postgres://127.0.0.1:{DB_PORT}/{DB}",
                           f"postgres://{DB_USER}@127.0.0.1:{DB_PORT}/",
                           f"postgres://{DB_USER}@/{DB}"):
            result = check(dict(hostile, DATABASE_URL=incomplete), "--healthcheck", success=False)
            assert "DATABASE_URL" in result.stderr and ("explicitly provide" in result.stderr or "PostgreSQL URL" in result.stderr), result.stderr
    finally:
        fifo.unlink()


def schema_isolation(env):
    """Ambient and explicit alternate schemas cannot spoof public health."""
    sql("""
        CREATE SCHEMA fixture_shadow;
        CREATE TABLE fixture_shadow.worker_heartbeats(id text PRIMARY KEY, last_seen timestamptz NOT NULL);
        INSERT INTO fixture_shadow.worker_heartbeats(id,last_seen) VALUES('named-b',now());
    """)
    poisoned = dict(env, BEWEFT_WORKER_ID="named-b", PGOPTIONS="-c search_path=fixture_shadow,public")
    before_public = sql("SELECT last_seen::text FROM public.worker_heartbeats WHERE id='named-b'")
    before_shadow = sql("SELECT last_seen::text FROM fixture_shadow.worker_heartbeats WHERE id='named-b'")
    result = check(poisoned, "--healthcheck", success=False)
    assert "missing or stale" in result.stderr.lower()
    assert sql("SELECT last_seen::text FROM public.worker_heartbeats WHERE id='named-b'") == before_public
    assert sql("SELECT last_seen::text FROM fixture_shadow.worker_heartbeats WHERE id='named-b'") == before_shadow

    sql("UPDATE public.worker_heartbeats SET last_seen=now() WHERE id='named-b'; "
        "UPDATE fixture_shadow.worker_heartbeats SET last_seen=now()-interval '31 seconds' WHERE id='named-b';")
    check(poisoned, "--healthcheck", success=True)
    shadow_options = urllib.parse.urlencode({"options": "-c search_path=fixture_shadow,public"})
    check(dict(poisoned, DATABASE_URL=DATABASE_URL + "?" + shadow_options), "--healthcheck", success=False)
    public_options = urllib.parse.urlencode({"options": "-c search_path=public"})
    check(dict(poisoned, DATABASE_URL=DATABASE_URL + "?" + public_options), "--healthcheck", success=True)
    sql("UPDATE public.worker_heartbeats SET last_seen=now()-interval '31 seconds' WHERE id='named-b'")


def run(temp):
    temp = Path(temp)
    clean_env = {k: v for k, v in os.environ.items()
                 if not k.startswith(("BEWEFT_", "COMPOSE_", "PG")) and k != "DATABASE_URL"}
    marker = temp / "health-must-not-create-media"
    health_env = dict(clean_env, DATABASE_URL=DATABASE_URL, BEWEFT_WORKER_ID="named-a",
                      BEWEFT_MEDIA_DIR=str(marker), BEWEFT_PUBLIC_URL="not-an-origin",
                      BEWEFT_ENCRYPTION_KEY="not-a-key")
    worker_env = dict(clean_env, DATABASE_URL=DATABASE_URL, BEWEFT_MEDIA_DIR=str(temp / "worker-media"),
                      BEWEFT_PUBLIC_URL="http://127.0.0.1:3000", BEWEFT_ENABLE_FIXTURES="false")
    processes = []
    try:
        # Invalid invocations must fail before creating even the migration table.
        for args in (("--unknown",), ("--healthcheck", "--unknown"), ("--healthcheck=1",)):
            result = check(health_env, *args, success=False)
            assert "usage" in result.stderr.lower()
        absent_id = dict(health_env)
        del absent_id["BEWEFT_WORKER_ID"]
        assert "BEWEFT_WORKER_ID" in check(absent_id, "--healthcheck", success=False).stderr
        absent_url = dict(health_env)
        del absent_url["DATABASE_URL"]
        assert "DATABASE_URL" in check(absent_url, "--healthcheck", success=False).stderr
        for invalid_id in ("", "bad id", "x" * 129, "invalid-private-id!", "ü"):
            for args in ((), ("--healthcheck",)):
                result = check(dict(health_env, BEWEFT_WORKER_ID=invalid_id), *args, success=False)
                assert "BEWEFT_WORKER_ID" in result.stderr
                if invalid_id:
                    assert invalid_id not in result.stdout + result.stderr
        check(health_env, "--healthcheck", success=False)
        assert sql("SELECT to_regclass('public._sqlx_migrations') IS NULL AND to_regclass('public.worker_heartbeats') IS NULL") == "t"
        assert not marker.exists(), "Health or invalid invocation created a media directory"
        compose_contract(clean_env)
        connection_deadline(health_env)
        tls_certificate_deadline(health_env, temp)

        # Two named instances must be independently observable.
        worker_app_name = "beweft-worker-env-" + RUN_ID
        worker_a = subprocess.Popen([str(BIN / "beweft-worker")], cwd=ROOT,
                                    env=dict(worker_env, BEWEFT_WORKER_ID="named-a", PGAPPNAME=worker_app_name),
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        processes.append(worker_a)
        wait_until(lambda: sql("SELECT to_regclass('public.worker_heartbeats') IS NOT NULL") == "t",
                   "Worker did not complete isolated migrations")
        wait_until(lambda: sql("SELECT count(*) FROM worker_heartbeats WHERE id='named-a'") == "1",
                   "Named worker A did not heartbeat")
        assert sql("SELECT count(*) FROM pg_stat_activity WHERE application_name='" + worker_app_name + "'") != "0", \
            "Normal worker no longer honors its PostgreSQL environment"
        worker_b = subprocess.Popen([str(BIN / "beweft-worker")], cwd=ROOT,
                                    env=dict(worker_env, BEWEFT_WORKER_ID="named-b"),
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        processes.append(worker_b)
        wait_until(lambda: sql("SELECT count(*) FROM worker_heartbeats WHERE id='named-b'") == "1",
                   "Named worker B did not heartbeat")
        assert not check(health_env, "--healthcheck", success=True).stdout
        postgres_environment_isolation(health_env, temp)
        check(dict(health_env, BEWEFT_WORKER_ID="named-b"), "--healthcheck", success=True)
        check(dict(health_env, BEWEFT_WORKER_ID="wrong-id"), "--healthcheck", success=False)
        query_deadline(health_env)

        stop(worker_a)
        assert worker_a.returncode == 0
        assert sql("SELECT count(*) FROM worker_heartbeats WHERE id='named-a'") == "0"
        check(health_env, "--healthcheck", success=False)
        check(dict(health_env, BEWEFT_WORKER_ID="named-b"), "--healthcheck", success=True)
        worker_b.kill()  # Crash only this isolated test worker.
        worker_b.wait(timeout=10)
        assert sql("SELECT count(*) FROM worker_heartbeats WHERE id='named-b'") == "1"
        sql("UPDATE worker_heartbeats SET last_seen=now()-interval '31 seconds' WHERE id='named-b'")
        stale_before = sql("SELECT last_seen::text FROM worker_heartbeats WHERE id='named-b'")
        check(dict(health_env, BEWEFT_WORKER_ID="named-b"), "--healthcheck", success=False)
        assert sql("SELECT last_seen::text FROM worker_heartbeats WHERE id='named-b'") == stale_before
        schema_isolation(health_env)

        # A fresh retained heartbeat does not grant the health process permission
        # to start a scheduler or claim a due publication.
        brand, channel, post, publication = [str(uuid.uuid4()) for _ in range(4)]
        sql(f"""
            INSERT INTO brands(id,name) VALUES('{brand}','Isolated health fixture');
            INSERT INTO channels(id,brand_id,name,provider) VALUES('{channel}','{brand}','Manual','manual');
            INSERT INTO posts(id,brand_id,text) VALUES('{post}','{brand}','Health fixture');
            INSERT INTO publications(id,post_id,brand_id,channel_id,post_version,scheduled_at,state,payload)
              VALUES('{publication}','{post}','{brand}','{channel}',1,now()-interval '1 minute','queued',
                     '{{"text":"Health fixture","media_ids":[]}}'::jsonb);
            INSERT INTO worker_heartbeats(id,last_seen) VALUES('named-a',now());
        """)
        before = sql("SELECT id || ':' || last_seen::text FROM worker_heartbeats ORDER BY id")
        migrations = sql("SELECT count(*) FROM _sqlx_migrations")
        for _ in range(3):
            check(health_env, "--healthcheck", success=True)
        assert sql("SELECT id || ':' || last_seen::text FROM worker_heartbeats ORDER BY id") == before
        assert sql("SELECT count(*) FROM _sqlx_migrations") == migrations
        assert sql(f"SELECT state FROM publications WHERE id='{publication}'") == "queued"
        assert sql(f"SELECT count(*) FROM publication_attempts WHERE publication_id='{publication}'") == "0"
        assert not marker.exists()

        # A real restarted worker can still claim and complete the manual job.
        restarted = subprocess.Popen([str(BIN / "beweft-worker")], cwd=ROOT,
                                     env=dict(worker_env, BEWEFT_WORKER_ID="named-a"),
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        processes.append(restarted)
        wait_until(lambda: sql(f"SELECT state FROM publications WHERE id='{publication}'") == "manual_required",
                   "Restarted worker did not process its manual fixture")
        check(health_env, "--healthcheck", success=True)
        assert sql(f"SELECT count(*) FROM publication_attempts WHERE publication_id='{publication}'") == "1"
        stop(restarted)
        assert restarted.returncode == 0

        # Local workers may still choose an automatically generated ID.
        generated = subprocess.Popen([str(BIN / "beweft-worker")], cwd=ROOT, env=worker_env,
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        processes.append(generated)
        wait_until(lambda: sql("SELECT count(*) FROM worker_heartbeats WHERE id LIKE 'worker-%'") == "1",
                   "Worker without an explicit ID did not generate one")
        stop(generated)
        assert generated.returncode == 0
        assert sql("SELECT count(*) FROM worker_heartbeats WHERE id LIKE 'worker-%'") == "0"
        print("PASS: per-instance health, FIFO password-file and TLS deadlines, ambient PostgreSQL isolation, public-schema fencing, no health writes or claims, connection/query deadlines, worker restart, generated ID and Compose wiring")
    finally:
        for process in reversed(processes):
            stop(process)


if __name__ == "__main__":
    subprocess.run(["docker", "exec", CONTAINER, "createdb", "-U", DB_USER, DB], check=True)
    try:
        with tempfile.TemporaryDirectory(prefix="beweft-worker-health-") as temp:
            run(temp)
    finally:
        subprocess.run(["docker", "exec", CONTAINER, "dropdb", "-U", DB_USER, "--force", DB], check=True)
