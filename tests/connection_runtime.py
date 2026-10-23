#!/usr/bin/env python3
"""Isolated deployment/lifecycle acceptance. Never reads Keychain or runs a Tunnel.

The prepared runtime's declared helper artifact is replaced with a fixture;
the real Rust owner and IPC run against the existing App Server protocol fixture.
"""
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time
import uuid

REPO = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get('LOCAL_MCP_TEST_BINARY', REPO/'target/debug/local-mcp')).resolve()
BACKEND = REPO/'tests/fixtures/app_server.py'


def wait_for(predicate, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(.025)
    raise AssertionError('timed out waiting for fixture state')


def gone(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return True
    return False


with tempfile.TemporaryDirectory(prefix="local-mcp runtime ' ") as directory:
    base = Path(directory)
    root = base/'runtime'
    project = base/'project'
    project.mkdir()
    client = base/'client-fixture'
    client.write_text('#!/bin/sh\nexit 0\n')
    client.chmod(0o700)
    tunnel = 'tunnel_' + uuid.uuid4().hex
    prepare = [str(BINARY), 'prepare', '--destination', str(root), '--tunnel-id', tunnel,
               '--tunnel-client', str(client), '--codex', str(BACKEND)]
    result = subprocess.run(prepare, capture_output=True, text=True)
    assert result.returncode == 0, result.stderr
    manifest = json.loads((root/'runtime.json').read_text())
    for relative, field in [('bin/local-mcp', 'local_mcp_sha256'), ('client/tunnel-client', 'tunnel_client_sha256')]:
        assert hashlib.sha256((root/relative).read_bytes()).hexdigest() == manifest[field]
        assert (root/relative).stat().st_mode & 0o777 == 0o700
    for relative in ['runtime.json', 'profiles/local-mcp.yaml']:
        assert (root/relative).stat().st_mode & 0o777 == 0o600
    assert root.stat().st_mode & 0o777 == 0o700
    snapshot = {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() for p in root.rglob('*') if p.is_file()}
    assert subprocess.run(prepare, capture_output=True).returncode != 0
    assert snapshot == {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() for p in root.rglob('*') if p.is_file()}
    alias = base/'alias'
    alias.symlink_to(root, target_is_directory=True)
    redirected = prepare.copy()
    redirected[redirected.index('--destination')+1] = str(alias)
    assert subprocess.run(redirected, capture_output=True).returncode != 0
    subprocess.run(['/bin/sh', '-n', str(root/'start-local.command')], check=True)
    assert '-u CONTROL_PLANE_API_KEY -u OPENAI_API_KEY' in (root/'profiles/local-mcp.yaml').read_text()
    print('PASS prepare, artifact hashes, private permissions, overwrite/symlink refusal, launcher syntax', flush=True)

    # Trusted fixture artifact: exits/crashes without invoking security or a network.
    helper = root/'bin/local-mcp'
    helper.write_text('''#!/usr/bin/python3
import json, os, pathlib, signal, subprocess, sys, time
root = pathlib.Path(sys.argv[sys.argv.index('--runtime')+1])
session = sys.argv[sys.argv.index('--session')+1]
assert 'CONTROL_PLANE_API_KEY' not in os.environ and 'OPENAI_API_KEY' not in os.environ
mode = (root/'state/mode').read_text()
child = subprocess.Popen([sys.executable,'-c', 'import signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); print("ready",flush=True); time.sleep(60)'], stdout=subprocess.PIPE) if mode != 'auth' else None
if child: assert child.stdout.readline().strip() == b'ready'
with (root/'state/observed.jsonl').open('a') as f:
 f.write(json.dumps({'pid':os.getpid(),'child':child.pid if child else None,'session':session})+'\\n')
if mode != 'run': sys.exit(78 if mode == 'auth' else 1)
signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
while True: time.sleep(.1)
''')
    helper.chmod(0o700)
    manifest['local_mcp_sha256'] = hashlib.sha256(helper.read_bytes()).hexdigest()
    (root/'runtime.json').write_text(json.dumps(manifest))
    (root/'state/mode').write_text('run')

    def observed():
        path = root/'state/observed.jsonl'
        if not path.exists():
            return []
        # Append is short but may still be observed before its final newline.
        return [json.loads(line) for line in path.read_text().splitlines(keepends=True) if line.endswith('\n')]

    for ending in ['interrupt', 'eof', 'terminate', 'hangup']:
        (root/'state/observed.jsonl').unlink(missing_ok=True)
        (root/'state/mode').write_text('run')
        log_path = base/f'{ending}.log'
        with log_path.open('w') as log:
            owner = subprocess.Popen([str(BINARY), 'connect', str(project), '--runtime', str(root)],
                                     stdin=subprocess.PIPE, stdout=log, stderr=log, start_new_session=True)
        session = None
        try:
            wait_for(lambda: 'Session ready:' in log_path.read_text())
            session = next(line.split(': ',1)[1] for line in log_path.read_text().splitlines() if line.startswith('Session ready:'))
            socket_path = Path('/tmp')/f'local-mcp-{os.geteuid()}'/(session+'.sock')

            def call(name, **arguments):
                with socket.socket(socket.AF_UNIX) as connection:
                    connection.settimeout(5)
                    connection.connect(str(socket_path))
                    connection.sendall(json.dumps({'name':name, 'arguments':{'session_id':session, **arguments}}).encode()+b'\n')
                    with connection.makefile('rb') as reply:
                        return json.loads(reply.readline())['structuredContent']

            info = call('session_info')
            runtime_id = info['runtime_id']
            wait_for(lambda: observed())
            if ending == 'interrupt':
                job = call('exec_command', runtime_id=runtime_id, request_id='retained-job', argv=['hold'], wait_ms=0)
                duplicate = subprocess.run([str(BINARY), 'connect', str(project), '--runtime', str(root)], capture_output=True, timeout=5)
                assert duplicate.returncode != 0 and b'already owned' in duplicate.stderr
                (root/'state/mode').write_text('fail')
                # Crash only the leader: surviving same-group children must be
                # collected before retry, including ones that ignore SIGTERM.
                os.kill(observed()[-1]['pid'], signal.SIGKILL)
                wait_for(lambda: 'Tunnel restart limit reached' in log_path.read_text(), seconds=20)
                assert len(observed()) == 4, observed()
                for record in observed():
                    if record['child']:
                        wait_for(lambda pid=record['child']: gone(pid), seconds=5)
                assert owner.poll() is None
                assert call('session_info')['runtime_id'] == runtime_id
                current = call('command_status', runtime_id=runtime_id, job_id=job['job_id'])
                assert current['state'] == 'running', current
                (root/'state/mode').write_text('auth')
                owner.stdin.write(b'/tunnel-restart\n'); owner.stdin.flush()
                wait_for(lambda: 'Tunnel credential/startup failure' in log_path.read_text())
                count = len(observed())
                time.sleep(1.3)
                assert len(observed()) == count
                (root/'state/mode').write_text('run')
                owner.stdin.write(b'/tunnel-restart\n'); owner.stdin.flush()
                wait_for(lambda: len(observed()) == count+1)
                assert call('session_info')['runtime_id'] == runtime_id
                print('PASS exclusive Tunnel, finite retries, auth failure no retry, explicit restart preserves runtime/job', flush=True)
            if ending == 'eof':
                owner.stdin.close()
            else:
                os.kill(owner.pid, {'interrupt':signal.SIGINT,'terminate':signal.SIGTERM,'hangup':signal.SIGHUP}[ending])
            owner.wait(timeout=12)
            assert owner.returncode == 0, log_path.read_text()
            assert not socket_path.exists() and not socket_path.with_suffix('.lock').exists()
            for record in observed():
                for pid in [record['pid'], record['child']]:
                    if pid:
                        wait_for(lambda pid=pid: gone(pid), seconds=5)
            assert not (root/'state/health.url').exists() and not (root/'state/tunnel.pid').exists()
            diagnostics = next(line.split(': ',1)[1] for line in log_path.read_text().splitlines() if line.startswith('diagnostics (may contain command bodies):'))
            assert not Path(diagnostics).parent.exists()
            print('PASS', ending, 'owned client, child, backend and diagnostics cleaned up', flush=True)
        finally:
            if owner.poll() is None:
                owner.kill(); owner.wait(timeout=5)
                for record in observed():
                    if not gone(record['pid']):
                        os.killpg(record['pid'], signal.SIGKILL)
