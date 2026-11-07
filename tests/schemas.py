#!/usr/bin/env python3
"""Validate real MCP responses against the published tool schemas.

Uses disposable files and the fault backend, never real Keychain/Tunnel/model
connections. Install tests/requirements-schemas.txt in a development venv.
"""
import base64
import json
import os
from pathlib import Path
import select
import subprocess
import sys
import tempfile
import time
import uuid

from jsonschema import Draft202012Validator

REPO = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get("LOCAL_MCP_TEST_BINARY", REPO / "target/debug/local-mcp")).resolve()
FIXTURE = REPO / "tests/fixtures/app_server.py"
BUFFERS = {}


def receive(process, stream):
    buffer = BUFFERS.setdefault(stream, bytearray())
    deadline = time.monotonic() + 10
    while b"\n" not in buffer:
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not select.select([stream], [], [], remaining)[0]:
            raise TimeoutError("MCP/owner response timed out")
        chunk = os.read(stream.fileno(), 65536)
        if not chunk:
            raise RuntimeError(f"process exited: {process.poll()}")
        buffer.extend(chunk)
        assert len(buffer) <= 32 * 1024 * 1024, "response frame exceeded test budget"
    end = buffer.index(b"\n") + 1
    line = bytes(buffer[:end])
    del buffer[:end]
    return line


class Client:
    def __init__(self, session):
        self.session = session
        self.runtime = None
        self.sequence = 0
        self.process = subprocess.Popen(
            [str(BINARY), "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, env={**os.environ, "LOCAL_MCP_SESSION": session})
        self.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                   "clientInfo": {"name": "schema-contracts", "version": "1"}})
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        self.tools = {tool["name"]: tool for tool in self.request("tools/list", {})["tools"]}
        assert set(self.tools) == {
            "session_info", "read_file", "view_image", "apply_diff", "exec_command",
            "exec_command_expanded", "command_status", "write_stdin", "stop_command",
            "list_jobs", "list_files", "search_files"}
        self.validators = {}
        for name, tool in self.tools.items():
            Draft202012Validator.check_schema(tool["inputSchema"])
            for field, schema in tool["inputSchema"]["properties"].items():
                assert schema.get("description"), (name, field)
            if name == "view_image":
                assert "outputSchema" not in tool
            else:
                Draft202012Validator.check_schema(tool["outputSchema"])
                self.validators[name] = Draft202012Validator(tool["outputSchema"])
        self.samples = []
        self.runtime = self.call("session_info")["runtime_id"]

    def send(self, value):
        self.process.stdin.write(json.dumps(value).encode() + b"\n")
        self.process.stdin.flush()

    def request(self, method, params):
        self.sequence += 1
        self.send({"jsonrpc": "2.0", "id": self.sequence, "method": method, "params": params})
        result = json.loads(receive(self.process, self.process.stdout))
        assert result["id"] == self.sequence and "error" not in result, result
        return result["result"]

    def call(self, name, invalid=False, **arguments):
        arguments.setdefault("session_id", self.session)
        if name != "session_info":
            arguments.setdefault("runtime_id", self.runtime)
        if not invalid:
            Draft202012Validator(self.tools[name]["inputSchema"]).validate(arguments)
        response = self.request("tools/call", {"name": name, "arguments": arguments})
        if name == "view_image" and not response.get("isError"):
            assert "structuredContent" not in response
            assert any(block["type"] == "image" for block in response["content"])
            return json.loads(response["content"][0]["text"])
        value = response["structuredContent"]
        if name in self.validators:
            self.validators[name].validate(value)
            self.samples.append((name, value))
        if name == "session_info" and self.runtime is None:
            self.runtime = value["runtime_id"]
        return value

    def finish(self, job):
        deadline = time.monotonic() + 5
        cursor = None
        while time.monotonic() < deadline:
            arguments = {"job_id": job, "wait_ms": 100}
            if cursor is not None:
                arguments["cursor"] = cursor
            value = self.call("command_status", **arguments)
            if value["state"] in ("finished", "unknown"):
                return value
            cursor = value["next_cursor"]
        raise TimeoutError("job did not terminate")

    def close(self):
        self.process.stdin.close()
        self.process.wait(timeout=5)
        assert self.process.returncode == 0


def main():
    with tempfile.TemporaryDirectory(prefix="local-mcp-schema-") as directory:
        root = Path(directory).resolve()
        project = root / "project"
        project.mkdir()
        (project / "text.txt").write_bytes(b"before\r\n")
        (project / "many.txt").write_text("needle\n" * 250)
        config = root / "settings.json"
        config.write_text(json.dumps({"max_jobs": 4, "rpc_timeout_ms": 3000}))
        session = "schemas-" + uuid.uuid4().hex
        owner = subprocess.Popen(
            [str(BINARY), "start", session, "--config", str(config), "--codex", str(FIXTURE)],
            cwd=project, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        client = None
        try:
            while not receive(owner, owner.stderr).startswith(b"Session ready:"):
                pass
            client = Client(session)
            client.call("read_file", path="text.txt")
            page = client.call("read_file", path="many.txt")
            assert page["next_line"] == 201
            client.call("read_file", path="many.txt", start_line=201)
            client.call("read_file", invalid=True, path="many.txt", max_bytes=0)
            client.call("list_files", glob="*.txt", limit=1)
            client.call("search_files", pattern="needle", limit=2)
            client.call("search_files", pattern="missing")
            client.call("search_files", pattern="[", regex=True)

            started = client.call("exec_command", request_id="zero", argv=["exit", "7"], wait_ms=0)
            done = client.finish(started["job_id"])
            assert done["ok"] and done["exit_code"] == 7
            client.call("command_status", request_id="zero", max_output_bytes=8)
            client.call("command_status", job_id=started["job_id"], cursor="wrong")
            client.call("command_status", invalid=True, job_id=started["job_id"], request_id="zero")
            client.call("exec_command", invalid=True, request_id="bad", argv="private-sentinel")
            client.call("exec_command", request_id="zero", argv=["exit", "0"], wait_ms=0)

            expanded = client.call("exec_command_expanded", request_id="expanded",
                                   argv=["exit", "0"], wait_ms=0, reason="fixture test",
                                   access={"mode": "limited", "roots": [], "network": False})
            client.finish(expanded["job_id"])
            client.call("exec_command_expanded", request_id="denied", argv=["exit", "0"],
                        reason="fixture denial", access={"mode": "full"})

            pipe = client.call("exec_command", request_id="pipe", argv=["hang"],
                               input_mode="pipe", wait_ms=0)
            client.call("write_stdin", request_id="input", job_id=pipe["job_id"], data="test\n")
            client.call("write_stdin", request_id="eof", job_id=pipe["job_id"], eof=True)
            client.finish(pipe["job_id"])
            stop = client.call("exec_command", request_id="stop", argv=["hang"], wait_ms=0)
            client.call("stop_command", job_id=stop["job_id"])
            client.call("stop_command", job_id=stop["job_id"])
            client.finish(stop["job_id"])
            client.call("stop_command", job_id=stop["job_id"])

            patch = "--- a/text.txt\n+++ b/text.txt\n@@ -1 +1 @@\n-before\r\n+after\r\n"
            edited = client.call("apply_diff", request_id="edit", diff=patch)
            assert edited["edits"][0]["status"] == "applied"
            assert (project / "text.txt").read_bytes() == b"after\r\n"
            client.call("apply_diff", request_id="edit", diff=patch)
            client.call("apply_diff", request_id="bad-context", diff=patch)
            listed = client.call("list_jobs", limit=2)
            client.call("list_jobs", limit=2, cursor=listed["next_cursor"])
            client.call("list_jobs", runtime_id="old-runtime")

            if sys.platform == "darwin":
                image = "UklGRiIAAABXRUJQVlA4IBYAAAAwAQCdASoBAAEADsD+JaQAA3AAAAAA"
                (project / "pixel.webp").write_bytes(base64.b64decode(image))
                client.call("view_image", path="pixel.webp")

            for name, value in client.samples:
                assert "private-sentinel" not in json.dumps(value)
                if value.get("ok") and "state" in value:
                    corrupt = {**value, "state": "invented-state"}
                    assert not client.validators[name].is_valid(corrupt)
                if name == "read_file" and value.get("ok"):
                    assert not client.validators[name].is_valid({**value, "text": 123})
            names = {name for name, _ in client.samples}
            assert names == set(client.validators), names
            print(f"PASS all 12 tool definitions and {len(client.samples)} actual MCP responses; native image preserved")
        finally:
            if client:
                client.close()
            owner.stdin.close()
            owner.wait(timeout=10)
            if owner.returncode != 0:
                raise RuntimeError(owner.stderr.read().decode())


if __name__ == "__main__":
    main()
