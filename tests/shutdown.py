#!/usr/bin/env python3
"""Terminal shutdown regression; no Tunnel, credentials, or deployed changes.

LOCAL_MCP_TEST_BINARY=target/release/local-mcp python3 tests/shutdown.py [codex]
Defaults to the protocol fixture. Pass the official CLI for real-backend tests.
"""
import json
import os
import pathlib
import pty
import select
import signal
import socket
import subprocess
import sys
import tempfile
import time
import uuid

REPO = pathlib.Path(__file__).resolve().parents[1]
BINARY = pathlib.Path(os.environ.get('LOCAL_MCP_TEST_BINARY', REPO/'target/debug/local-mcp')).resolve()
CODEX = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else REPO/'tests/fixtures/app_server.py').resolve()


def read_available(master, output, timeout):
    if select.select([master], [], [], timeout)[0]:
        try:
            chunk = os.read(master, 65536)
            if not chunk:
                return False
            output.extend(chunk)
        except OSError:
            return False
    return True


def probe(mode):
    with tempfile.TemporaryDirectory(prefix='local-mcp-shutdown-') as directory:
        master, slave = pty.openpty()
        session = 'shutdown-'+str(uuid.uuid4())
        socket_path = pathlib.Path('/tmp')/('local-mcp-'+str(os.geteuid()))/(session+'.sock')
        owner = subprocess.Popen(
            [str(BINARY), 'start', session, '--codex', str(CODEX)],
            stdin=slave, stdout=slave, stderr=slave, start_new_session=True, cwd=directory,
        )
        os.close(slave)
        output = bytearray()
        try:
            deadline = time.monotonic()+20
            while b'Session ready:' not in output and time.monotonic() < deadline:
                if not read_available(master, output, .1):
                    break
            assert b'Session ready:' in output, output.decode(errors='replace')
            backend_ids = subprocess.check_output(['/usr/bin/pgrep', '-P', str(owner.pid)], text=True).split()
            job_pid = None
            if mode == 'group-active':
                def call(name, **arguments):
                    arguments['session_id'] = session
                    with socket.socket(socket.AF_UNIX) as connection:
                        connection.settimeout(5)
                        connection.connect(str(socket_path))
                        connection.sendall(json.dumps({'name': name, 'arguments': arguments}).encode()+b'\n')
                        with connection.makefile('rb') as reply:
                            return json.loads(reply.readline())['structuredContent']
                info = call('session_info')
                fixture = CODEX == (REPO/'tests/fixtures/app_server.py').resolve()
                argv = ['hold'] if fixture else ['/bin/sh', '-c', 'echo $$ > job.pid; exec /bin/sleep 60']
                job = call('exec_command', runtime_id=info['runtime_id'], request_id='shutdown-active', argv=argv, wait_ms=0)
                assert job['state'] == 'running', job
                if not fixture:
                    deadline = time.monotonic()+5
                    pid_file = pathlib.Path(directory)/'job.pid'
                    while not pid_file.exists() and time.monotonic() < deadline:
                        time.sleep(.01)
                    job_pid = int(pid_file.read_text())
            # No delay: readiness must also mean that Ctrl-C can be handled.
            started = time.monotonic()
            if mode.startswith('group'):
                os.killpg(owner.pid, signal.SIGINT)
            elif mode == 'owner':
                os.kill(owner.pid, signal.SIGINT)
            else:
                os.write(master, b'\x04')
            while owner.poll() is None and time.monotonic()-started < 13:
                if not read_available(master, output, .1):
                    break
            hung = owner.poll() is None
            while read_available(master, output, 0):
                if not select.select([master], [], [], 0)[0]:
                    break
            text = output.decode(errors='replace')
            warning = 'Backend exit was not confirmed' in text
            print(json.dumps({'mode': mode, 'hung': hung, 'exit': owner.poll(),
                              'warning': warning, 'elapsed': round(time.monotonic()-started, 2)}), flush=True)
            assert not hung and owner.returncode == 0 and not warning, text
            diagnostics = next(line.split('diagnostics (may contain command bodies): ', 1)[1].strip()
                               for line in text.splitlines() if line.startswith('diagnostics (may contain command bodies): '))
            assert not pathlib.Path(diagnostics).parent.exists(), 'owned diagnostics remain'
            assert not socket_path.exists() and not socket_path.with_suffix('.lock').exists()
            for pid in backend_ids+([str(job_pid)] if job_pid else []):
                try:
                    os.kill(int(pid), 0)
                except ProcessLookupError:
                    continue
                raise AssertionError('owned process remains: '+pid)
            print('PASS', mode, 'owner exited cleanly and owned diagnostics removed', flush=True)
        finally:
            if owner.poll() is None:
                os.killpg(owner.pid, signal.SIGTERM)
                owner.wait(timeout=5)
            os.close(master)


def probe_startup():
    # Stall after initialize is received. Ctrl-C must close and reap this
    # isolated child rather than waiting for the normal RPC timeout.
    with tempfile.TemporaryDirectory(prefix='local-mcp-startup-') as directory:
        root = pathlib.Path(directory)
        pid_file = root/'backend.pid'
        fixture = root/'delayed.py'
        fixture.write_text('#!/usr/bin/python3\nimport os,sys,pathlib\n'
                           f'fixture={str(REPO/"tests/fixtures/app_server.py")!r}\n'
                           'if "generate-json-schema" in sys.argv:\n'
                           ' os.execv(fixture,[fixture]+sys.argv[1:])\n'
                           f'pathlib.Path({str(pid_file)!r}).write_text(str(os.getpid()))\n'
                           'for line in sys.stdin: pass\n')
        fixture.chmod(0o700)
        owner = subprocess.Popen([str(BINARY), 'start', 'startup-'+str(uuid.uuid4()), '--codex', str(fixture)],
                                 stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
                                 stderr=subprocess.PIPE, cwd=root, start_new_session=True)
        backend_pid = None
        try:
            deadline = time.monotonic()+5
            while not pid_file.exists() and time.monotonic() < deadline:
                time.sleep(.01)
            backend_pid = int(pid_file.read_text())
            started = time.monotonic()
            os.killpg(owner.pid, signal.SIGINT)
            owner.wait(timeout=2)
            text = owner.stderr.read().decode()
            assert owner.returncode != 0 and 'startup interrupted' in text, text
            try:
                os.kill(backend_pid, 0)
            except ProcessLookupError:
                pass
            else:
                raise AssertionError('initializing backend remains')
            print('PASS interrupted initialization reaps backend in', round(time.monotonic()-started, 2), 'seconds', flush=True)
        finally:
            if owner.poll() is None:
                os.killpg(owner.pid, signal.SIGTERM)
                owner.wait(timeout=5)
            if backend_pid:
                try:
                    os.kill(backend_pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
            owner.stdin.close()
            owner.stderr.close()


if __name__ == '__main__':
    probe_startup()
    for mode in ('group', 'owner', 'eof', 'group-active'):
        probe(mode)
