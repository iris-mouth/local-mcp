#!/usr/bin/env python3
"""CLI acceptance against isolated protocol/HTTP fixtures. No Keychain or OpenAI calls.

Uses a private HOME and replaces the prepared credential helper with a declared,
hashed test artifact, just like connection_runtime.py. PTY input is exercised as
a user would enter it. No Python is needed by a deployed runtime.
"""
import hashlib
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import socket
import subprocess
import sys
import tempfile
import time
import uuid

REPO = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get('LOCAL_MCP_TEST_BINARY', REPO/'target/debug/local-mcp')).resolve()
BACKEND = REPO/'tests/fixtures/app_server.py'


class Terminal:
    def __init__(self, args, env, cwd):
        self.master, slave = pty.openpty()
        self.child = subprocess.Popen([str(BINARY), *map(str, args)], stdin=slave,
                                      stdout=slave, stderr=slave, env=env,
                                      cwd=cwd, start_new_session=True)
        os.close(slave)
        self.output = bytearray()
        self.cursor = 0

    def read(self, wait=.05):
        if select.select([self.master], [], [], wait)[0]:
            try:
                chunk = os.read(self.master, 65536)
                self.output.extend(chunk)
            except OSError:
                pass

    def expect(self, text, timeout=15):
        value = text.encode()
        deadline = time.monotonic()+timeout
        while time.monotonic() < deadline:
            index = self.output.find(value, self.cursor)
            if index != -1:
                self.cursor = index+len(value)
                return
            self.read()
        raise AssertionError(f'missing {text!r}\n{self.output.decode(errors="replace")}')

    def send(self, line):
        os.write(self.master, (str(line)+'\n').encode())

    def finish(self, expected=0):
        deadline = time.monotonic()+15
        while self.child.poll() is None and time.monotonic() < deadline:
            self.read()
        self.child.wait(timeout=1)
        self.read(0)
        assert self.child.returncode == expected, self.output.decode(errors='replace')

    def close(self):
        if self.child.poll() is None:
            self.child.terminate()
            try:
                self.child.wait(timeout=12)
            except subprocess.TimeoutExpired:
                self.child.kill()
                self.child.wait()
        os.close(self.master)


def wait_for(predicate, seconds=8):
    deadline = time.monotonic()+seconds
    while time.monotonic() < deadline:
        result = predicate()
        if result:
            return result
        time.sleep(.025)
    raise AssertionError('fixture state timed out')


def call(session, name, **args):
    path = Path('/tmp')/f'local-mcp-{os.geteuid()}'/(session+'.sock')
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(3)
        client.connect(str(path))
        client.sendall(json.dumps({'name': name, 'arguments': {'session_id': session, **args}}).encode()+b'\n')
        with client.makefile('rb') as response:
            return json.loads(response.readline())['structuredContent']


def setup(env, base, destination, client=None, old=None, config=None, expected=0, backend=BACKEND, default_entry=False):
    args = [] if default_entry else ['setup']
    if old:
        args += ['--from-runtime', old]
    if config:
        args += ['--config', config]
    terminal = Terminal(args, env, base)
    try:
        for label, value in [
            ('Tunnel ID', '' if old else 'tunnel_'+uuid.uuid4().hex),
            ('公式Codexの実行ファイル', '' if old else backend),
            ('公式Tunnel clientの実行ファイル', '' if old else client),
            ('Keychain service', ''), ('Keychain account', ''),
            ('新しい配置先', destination),
        ]:
            terminal.expect(label)
            terminal.send(value)
        if default_entry:
            terminal.expect('作業先のパス')
            terminal.send('')  # Cancel before any credential helper starts.
            terminal.finish(1)
        else:
            terminal.finish(expected)
        return terminal.output.decode(errors='replace')
    finally:
        terminal.close()


def install_helper(runtime, server):
    # Only tunnel-run is replaced. Every other CLI entry still executes Rust.
    helper = runtime/'bin/local-mcp'
    helper.write_text('#!/usr/bin/python3\nimport os,sys\n'
                      f'binary={str(BINARY)!r}\nserver={str(server)!r}\n'
                      'if sys.argv[1:2] == ["tunnel-run"]:\n'
                      ' os.execv(sys.executable,[sys.executable,server,*sys.argv[2:]])\n'
                      'os.execv(binary,[binary,*sys.argv[1:]])\n')
    manifest = json.loads((runtime/'runtime.json').read_text())
    manifest['local_mcp_sha256'] = hashlib.sha256(helper.read_bytes()).hexdigest()
    (runtime/'runtime.json').write_text(json.dumps(manifest))


with tempfile.TemporaryDirectory(prefix="local-mcp-cli ' ") as directory:
    base = Path(directory).resolve()
    home = base/'home'
    home.mkdir()
    env = {**os.environ, 'HOME': str(home), 'USER': 'cli-fixture'}
    prefs_file = home/'Library/Application Support/local-mcp/preferences.json'
    config = base/'settings.json'
    config.write_text(json.dumps({'max_jobs': 2}))
    failed_doctor = base/'fail-doctor'
    client = base/'client-fixture'
    client.write_text('#!/usr/bin/python3\nimport os,sys,pathlib\n'
                      'assert sys.argv[1] == "doctor", "fixture cannot connect"\n'
                      'assert os.environ["CONTROL_PLANE_API_KEY"] == "sk-local-mcp-doctor-placeholder-not-a-credential"\n'
                      f'sys.exit(1 if pathlib.Path({str(failed_doctor)!r}).exists() else 0)\n')
    client.chmod(0o700)
    runtime = base/'runtime-one'
    setup(env, base, runtime, client=client, config=config)
    prefs = json.loads(prefs_file.read_text())
    assert prefs['runtime'] == str(runtime) and prefs['settings']['max_jobs'] == 2
    assert prefs['recent_workspaces'] == []
    assert prefs_file.stat().st_mode & 0o777 == 0o600
    assert prefs_file.parent.stat().st_mode & 0o777 == 0o700
    assert not (runtime/'state/health.url').exists()
    print('PASS guided setup, private preferences, imported Settings, no connection or credential access', flush=True)

    server = base/'health-fixture.py'
    server.write_text('''import http.server,pathlib,sys,time,signal
root=pathlib.Path(sys.argv[sys.argv.index('--runtime')+1])
signal.signal(signal.SIGTERM,lambda *_: None if (root/'state/slow-stop').exists() else sys.exit(0))
class Handler(http.server.BaseHTTPRequestHandler):
 def do_GET(self):
  mode=root/'state/readiness'
  mode=mode.read_text() if mode.exists() else '200'
  if mode == 'slow':
   (root/'state/probe-entered').touch()
   time.sleep(3)
  self.send_response(200 if mode == 'slow' else int(mode))
  self.end_headers()
 def log_message(self,*args): pass
httpd=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler)
(root/'state/health.url').write_text('http://127.0.0.1:'+str(httpd.server_port))
httpd.serve_forever()
''')
    install_helper(runtime, server)
    project = base/'workspace'
    project.mkdir()
    terminal = Terminal(['connect', project], env, base)
    try:
        terminal.expect('Session ready:')
        terminal.expect('接続準備完了')
        session = re.search(rb'local-mcp session: ([^\r\n]+)', terminal.output).group(1).decode()
        info = call(session, 'session_info')
        rid = info['runtime_id']
        assert info['session']['cwd'] == str(project)
        assert info['session']['settings']['max_jobs'] == 2
        assert json.loads(prefs_file.read_text())['recent_workspaces'] == [str(project)]
        job = call(session, 'exec_command', runtime_id=rid, request_id='first', argv=['hold'], wait_ms=0)
        terminal.send('/jobs')
        terminal.expect(job['job_id'])
        terminal.expect('実行中')

        # Confirmation leaves IPC responsive and admits work until consent.
        terminal.send('/quit')
        terminal.expect('[y/N]')
        second = call(session, 'exec_command', runtime_id=rid, request_id='second', argv=['hold'], wait_ms=0)
        assert second['ok'] and not call(session, 'session_info')['closing']
        terminal.send('n')
        terminal.expect('終了を取り消しました')
        assert terminal.child.poll() is None

        # Active defaults cannot be switched, even by a guided update.
        locked = Terminal(['setup', '--from-runtime', runtime], env, base)
        try:
            locked.expect('Tunnel ID')
            locked.send('')
            locked.finish(1)
            assert b'already owned' in locked.output
        finally:
            locked.close()

        # A slow probe must not hold up the CLI or existing IPC work.
        (runtime/'state/readiness').write_text('slow')
        wait_for(lambda: (runtime/'state/probe-entered').exists())
        terminal.send('/jobs')
        terminal.expect('JOB_ID', timeout=.75)
        assert call(session, 'session_info')['runtime_id'] == rid
        (runtime/'state/readiness').write_text('503')
        terminal.expect('接続準備未完了', timeout=8)
        (runtime/'state/readiness').write_text('200')
        (runtime/'state/slow-stop').touch()
        terminal.send('/tunnel-restart')
        terminal.expect('Tunnel再接続中')
        assert call(session, 'command_status', runtime_id=rid, job_id=job['job_id'])['state'] == 'running'
        terminal.send('/jobs')
        terminal.expect('JOB_ID', timeout=.75)
        (runtime/'state/slow-stop').unlink()
        terminal.expect('接続準備完了')
        assert call(session, 'session_info')['runtime_id'] == rid

        terminal.send('/stop '+job['job_id'])
        terminal.expect('停止要求（実行終了は /jobs で確認）')
        wait_for(lambda: call(session, 'command_status', runtime_id=rid, job_id=job['job_id'])['state'] == 'finished')
        terminal.send('/quit')
        terminal.expect('[y/N]')
        terminal.send('y')
        terminal.finish()
        assert not (runtime/'state/health.url').exists()
        assert not (Path('/tmp')/f'local-mcp-{os.geteuid()}'/(session+'.sock')).exists()
        print('PASS jobs, stop, quit cancellation/consent, concurrent IPC, slow readiness, restart preserves runtime/job', flush=True)
    finally:
        terminal.close()

    terminal = Terminal(['connect', project], env, base)
    try:
        terminal.expect('Session ready:')
        terminal.expect('接続準備完了')
        (runtime/'state/slow-stop').touch()
        terminal.send('/tunnel-restart')
        terminal.expect('Tunnel再接続中')
        terminal.send('/quit')
        terminal.finish()
        assert not (runtime/'state/health.url').exists()
        assert not (runtime/'state/tunnel.pid').exists()
        (runtime/'state/slow-stop').unlink()
        print('PASS shutdown during a slow reconnect reaps the client and removes owned state', flush=True)
    finally:
        terminal.close()

    # Default entry point selects recent workspaces; config override is temporary.
    override = base/'override.json'
    override.write_text(json.dumps({'max_jobs': 1}))
    terminal = Terminal(['connect', '--config', override], env, base)
    try:
        terminal.expect('作業先のパス')
        terminal.send('1')
        terminal.expect('Session ready:')
        session = re.search(rb'local-mcp session: ([^\r\n]+)', terminal.output).group(1).decode()
        info = call(session, 'session_info')
        assert info['session']['settings']['max_jobs'] == 1
        assert json.loads(prefs_file.read_text())['settings']['max_jobs'] == 2
        # Backend loss remains distinguishable from successful exit in /jobs.
        job = call(session, 'exec_command', runtime_id=info['runtime_id'], request_id='loss', argv=['lose'], wait_ms=0)
        wait_for(lambda: not call(session, 'session_info')['backend_available'])
        terminal.send('/jobs')
        terminal.expect(job['job_id'])
        terminal.expect('不明')
        os.kill(terminal.child.pid, signal.SIGINT)
        terminal.expect('[y/N]')
        os.kill(terminal.child.pid, signal.SIGINT)
        terminal.finish()
        print('PASS recent selection, temporary config override, unknown job state, double Ctrl-C shutdown', flush=True)
    finally:
        terminal.close()

    terminal = Terminal([], env, base)
    try:
        terminal.expect('作業先のパス')
        terminal.send(project)
        terminal.expect('Session ready:')
        terminal.send('/quit')
        terminal.finish()
    finally:
        terminal.close()

    # Neither absent nor deleted projects silently select another workspace.
    result = subprocess.run([str(BINARY), 'connect'], env=env, cwd=base, input=b'', capture_output=True)
    assert result.returncode != 0 and b'PROJECT is required' in result.stderr
    project.rmdir()
    terminal = Terminal([], env, base)
    try:
        terminal.expect('作業先のパス')
        terminal.send('1')
        terminal.finish(1)
    finally:
        terminal.close()

    newer = base/'runtime-two'
    setup(env, base, newer, old=runtime)
    updated = json.loads(prefs_file.read_text())
    assert updated['runtime'] == str(newer) and updated['settings']['max_jobs'] == 2
    assert updated['recent_workspaces'] == [str(project)]
    assert (runtime/'runtime.json').exists()
    failed_doctor.touch()
    setup(env, base, base/'runtime-rejected', old=newer, expected=1)
    assert json.loads(prefs_file.read_text()) == updated
    print('PASS default entry, deleted/noninteractive workspace rejection, update/rollback keeps settings and old runtime', flush=True)

    prefs_file.write_text('broken')
    result = subprocess.run([str(BINARY), 'connect', str(base), '--runtime', str(runtime)],
                            env=env, cwd=base, input=b'', capture_output=True)
    assert result.returncode != 0 and b'invalid preferences' in result.stderr
    assert prefs_file.read_text() == 'broken'
    print('PASS corrupt preferences refuse connection without fallback or overwrite', flush=True)

    fresh_home = base/'fresh-home'
    fresh_home.mkdir()
    failed_doctor.unlink()
    setup({**env, 'HOME':str(fresh_home)}, base, base/'fresh-runtime', client=client, default_entry=True)
    assert (fresh_home/'Library/Application Support/local-mcp/preferences.json').exists()
    assert not (base/'fresh-runtime/state/health.url').exists()
    print('PASS argument-free first launch sets up and reaches workspace selection without starting credentials', flush=True)

    if len(sys.argv) == 3:
        real_home = base/'real-dependency-home'
        real_home.mkdir()
        setup({**env, 'HOME':str(real_home)}, base, base/'real-dependency-runtime',
              client=Path(sys.argv[2]).resolve(), backend=Path(sys.argv[1]).resolve())
        print('PASS guided setup with official Codex schemas and official Tunnel doctor; no connection or real key', flush=True)
