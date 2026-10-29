"""Fail-closed network boundary for provider fixture subprocesses."""
from pathlib import Path
import errno
import json
import os
import socket
import sys

PROFILE = '(version 1) (allow default) (deny network-outbound) (allow network-outbound (remote ip "localhost:*"))'


def loopback_only(command, allow_docker=False):
    # Children inherit the verified parent sandbox; applying a second, different
    # profile is not supported on all macOS versions.
    if os.environ.get('BEWEFT_PROVIDER_TEST_SANDBOX') == '1':
        return list(map(str, command))
    sandbox = Path('/usr/bin/sandbox-exec')
    if sys.platform != 'darwin' or not sandbox.is_file():
        raise RuntimeError('Provider fixture tests require a verified loopback-only subprocess sandbox on this host')
    profile = PROFILE
    if allow_docker:
        docker_socket = Path.home() / '.docker/run/docker.sock'
        if not docker_socket.exists():
            raise RuntimeError('Provider fixture tests require the local Docker socket')
        profile += f' (allow network-outbound (remote unix-socket (path-literal {json.dumps(str(docker_socket.resolve()))})))'
    return [str(sandbox), '-p', profile, *map(str, command)]


def enter_loopback_sandbox(allow_docker=False):
    os.environ['BEWEFT_REQUIRE_PROVIDER_FIXTURES'] = 'true'
    if os.environ.get('BEWEFT_PROVIDER_TEST_SANDBOX') != '1':
        env = dict(os.environ, BEWEFT_PROVIDER_TEST_SANDBOX='1')
        command = loopback_only([sys.executable, *sys.argv], allow_docker=allow_docker)
        os.execvpe(command[0], command, env)
    # Even a pre-set marker cannot bypass the actual network boundary.
    with socket.socket() as probe:
        probe.settimeout(1)
        if probe.connect_ex(('192.0.2.1', 9)) not in (errno.EPERM, errno.EACCES):
            raise RuntimeError('Provider fixture process is not in a loopback-only sandbox')
