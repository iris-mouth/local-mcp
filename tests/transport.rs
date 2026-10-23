#![cfg(unix)]

use local_mcp::protocol::read_frame;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

struct Owner {
    child: Child,
    session: String,
    _directory: tempfile::TempDir,
}
impl Owner {
    async fn start(capacity: usize) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("settings.json");
        std::fs::write(&config, json!({"max_rpc_inflight":capacity}).to_string()).unwrap();
        let session = format!("transport-{}", uuid::Uuid::new_v4());
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/app_server.py");
        let mut child = Command::new(env!("CARGO_BIN_EXE_local-mcp"))
            .args(["start", &session, "--config"])
            .arg(config)
            .arg("--codex")
            .arg(fixture)
            .current_dir(directory.path())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut errors = BufReader::new(child.stderr.take().unwrap()).lines();
        tokio::time::timeout(Duration::from_secs(10), async {
            let mut log = String::new();
            loop {
                let line = errors
                    .next_line()
                    .await
                    .unwrap()
                    .unwrap_or_else(|| panic!("owner exited before ready: {log}"));
                log.push_str(&line);
                log.push('\n');
                if line.starts_with("Session ready:") {
                    break;
                }
            }
        })
        .await
        .expect("owner startup timed out");
        Self {
            child,
            session,
            _directory: directory,
        }
    }
    async fn shutdown(mut self) {
        drop(self.child.stdin.take());
        let exit = tokio::time::timeout(Duration::from_secs(5), self.child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(exit.success());
    }
}

struct McpClient {
    _child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    session: String,
}
impl McpClient {
    async fn start(session: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_local-mcp"))
            .arg("mcp")
            .env("LOCAL_MCP_SESSION", session)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut client = Self {
            _child: child,
            input,
            output,
            session: session.into(),
        };
        client.send(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"transport-test","version":"1"}
        }})).await;
        assert!(client.receive().await.get("result").is_some());
        client
            .send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
        client
    }
    async fn send(&mut self, value: Value) {
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        self.input.write_all(&bytes).await.unwrap();
        self.input.flush().await.unwrap();
    }
    async fn call(&mut self, id: u64, name: &str, mut arguments: Value) {
        arguments["session_id"] = json!(self.session);
        self.send(
            json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{
                "name":name,"arguments":arguments
            }}),
        )
        .await;
    }
    async fn receive(&mut self) -> Value {
        let bytes = tokio::time::timeout(
            Duration::from_secs(5),
            read_frame(&mut self.output, 1024 * 1024),
        )
        .await
        .expect("MCP response timed out")
        .unwrap()
        .expect("MCP exited");
        serde_json::from_slice(&bytes).unwrap()
    }
    async fn result(&mut self, id: u64) -> Value {
        let result = self.receive().await;
        assert_eq!(result["id"], id, "unexpected response: {result}");
        assert!(result.get("error").is_none(), "{result}");
        result["result"]["structuredContent"].clone()
    }
    async fn runtime(&mut self) -> String {
        self.call(1, "session_info", json!({})).await;
        self.result(1).await["runtime_id"].as_str().unwrap().into()
    }
}

#[tokio::test]
async fn one_owner_rpc_slot_keeps_control_available_during_exec_wait() {
    let owner = Owner::start(1).await;
    let session = owner.session.clone();
    let result = tokio::spawn(async move {
    let mut client = McpClient::start(&session).await;
    let runtime = client.runtime().await;
    client.call(2, "exec_command", json!({"runtime_id":runtime,"request_id":"waiting-exec", "argv":["hang"],"input_mode":"pipe","wait_ms":30000})).await;
    let state = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            client.call(3, "command_status", json!({"runtime_id":runtime,"request_id":"waiting-exec"})).await;
            let state = client.result(3).await;
            if state["error"]["code"] == "not_found" {
                tokio::task::yield_now().await;
                continue;
            }
            assert_eq!(state["ok"], true, "{state}");
            break state;
        }
    }).await.unwrap();
    let job = &state["job_id"];
    client.call(4, "session_info", json!({})).await;
    assert_eq!(client.result(4).await["ok"], true);
    client.call(5, "write_stdin", json!({"runtime_id":runtime,"job_id":job,"request_id":"input","data":"one"})).await;
    assert_eq!(client.result(5).await["delivery"], "accepted");
    client.call(6, "stop_command", json!({"runtime_id":runtime,"job_id":job})).await;
    assert_eq!(client.result(6).await["stop_ack"]["delivery"], "accepted");
    let done = client.result(2).await;
    assert_eq!(done["state"], "finished");
    assert_eq!(done["exit_code"], 143);
    drop(client);
    }).await;
    owner.shutdown().await;
    result.unwrap();
}

#[tokio::test]
async fn saturated_mcp_and_owner_waits_keep_control_capacity_reserved() {
    let owner = Owner::start(64).await;
    let session = owner.session.clone();
    let result = tokio::spawn(async move {
    let mut client = McpClient::start(&session).await;
    let runtime = client.runtime().await;
    client.call(2, "exec_command", json!({"runtime_id":runtime,"request_id":"active","argv":["hang"],"input_mode":"pipe","wait_ms":0})).await;
    let job = client.result(2).await["job_id"].clone();
    let cursor = loop {
        client.call(3, "command_status", json!({"runtime_id":runtime,"job_id":job})).await;
        let state = client.result(3).await;
        if state["stdout"]["text"] == "ready\n" { break state["next_cursor"].clone(); }
        tokio::task::yield_now().await;
    };
    // One overflow response is a barrier: the other 64 calls still hold their wait slots.
    for id in 10..75 {
        client.call(id, "command_status", json!({"runtime_id":runtime,"job_id":job,"cursor":cursor,"wait_ms":30000})).await;
    }
    let rejected = client.receive().await;
    assert_eq!(rejected["result"]["structuredContent"]["error"]["code"], "rpc_capacity");
    client.call(100, "session_info", json!({})).await;
    assert_eq!(client.result(100).await["ok"], true);
    client.call(101, "command_status", json!({"runtime_id":runtime,"job_id":job})).await;
    assert_eq!(client.result(101).await["state"], "running");
    client.call(102, "stop_command", json!({"runtime_id":runtime,"job_id":job})).await;
    assert_eq!(client.result(102).await["stop_ack"]["delivery"], "accepted");
    for _ in 0..64 {
        let response = client.receive().await;
        assert_eq!(response["result"]["structuredContent"]["state"], "finished");
    }
    drop(client);
    }).await;
    owner.shutdown().await;
    result.unwrap();
}
