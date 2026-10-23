use crate::{config::Session, protocol::read_frame};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::Path,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Semaphore, mpsc, oneshot},
};

#[derive(Debug)]
pub enum Event {
    Output(Value),
    Finished(String, std::result::Result<Value, RpcError>),
    Lost,
}
#[derive(Clone, Debug)]
pub struct RpcError {
    pub uncertain: bool,
    pub code: &'static str,
}
enum Pending {
    Command(String),
    Control(oneshot::Sender<std::result::Result<Value, RpcError>>),
}
pub struct AppServer {
    writer: tokio::sync::Mutex<Option<ChildStdin>>,
    pending: Mutex<HashMap<u64, Pending>>,
    next_id: AtomicU64,
    alive: AtomicBool,
    controls: Arc<Semaphore>,
    child: tokio::sync::Mutex<Child>,
    timeout: Duration,
    events: mpsc::Sender<Event>,
}
impl AppServer {
    pub async fn spawn(
        executable: &Path,
        session: &Session,
    ) -> Result<(Arc<Self>, mpsc::Receiver<Event>)> {
        let mut command = Command::new(executable);
        // Terminal Ctrl-C belongs to the owner. Keep the backend alive while
        // managed jobs are stopped and the original stdio is closed normally.
        #[cfg(unix)]
        command.process_group(0);
        command
            .arg("app-server")
            .args(["--listen", "stdio://"])
            .args([
                "-c",
                "shell_environment_policy.inherit=\"none\"",
                "-c",
                "history.persistence=\"none\"",
                "-c",
                "analytics.enabled=false",
                "-c",
                "otel.exporter=\"none\"",
                "-c",
                "otel.trace_exporter=\"none\"",
                "-c",
                "feedback.enabled=false",
            ])
            .arg("-c")
            .arg(format!(
                "sqlite_home={}",
                serde_json::to_string(&session.diagnostics_directory)?
            ))
            .env_clear()
            .envs(&session.environment)
            .env("CODEX_HOME", &session.diagnostics_directory)
            .env("RUST_LOG", "off")
            .current_dir(&session.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Diagnostics are owned by the official backend, not an adapter transcript.
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .context("cannot start official Codex App Server")?;
        let stdout = child.stdout.take().context("missing App Server stdout")?;
        let writer = child.stdin.take();
        let (tx, rx) = mpsc::channel(64);
        let server = Arc::new(Self {
            writer: tokio::sync::Mutex::new(writer),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            alive: AtomicBool::new(true),
            controls: Arc::new(Semaphore::new(session.settings.max_rpc_inflight)),
            child: tokio::sync::Mutex::new(child),
            timeout: Duration::from_millis(session.settings.rpc_timeout_ms),
            events: tx.clone(),
        });
        let reader_server = server.clone();
        let cap = session.settings.max_message_bytes;
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(line)) = read_frame(&mut reader, cap).await {
                let Ok(value) = serde_json::from_slice::<Value>(&line) else {
                    break;
                };
                if let Some(id) = value.get("id").and_then(Value::as_u64) {
                    let pending = reader_server.pending.lock().unwrap().remove(&id);
                    let result = if let Some(result) = value.get("result") {
                        Ok(result.clone())
                    } else {
                        Err(RpcError {
                            uncertain: false,
                            code: "backend_rejected",
                        })
                    };
                    match pending {
                        Some(Pending::Command(job)) => {
                            if tx.send(Event::Finished(job, result)).await.is_err() {
                                break;
                            }
                        }
                        Some(Pending::Control(reply)) => {
                            let _ = reply.send(result);
                        }
                        None => {}
                    }
                } else if value["method"] == "command/exec/outputDelta" {
                    if tx
                        .send(Event::Output(value["params"].clone()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                } else if value.get("id").is_some() {
                    // No model turns or approval delegation are part of this interface.
                    break;
                }
            }
            reader_server.alive.store(false, Ordering::SeqCst);
            for (_, p) in reader_server.pending.lock().unwrap().drain() {
                if let Pending::Control(reply) = p {
                    let _ = reply.send(Err(RpcError {
                        uncertain: true,
                        code: "backend_unavailable",
                    }));
                }
            }
            let _ = tx.send(Event::Lost).await;
        });
        server.control("initialize", json!({"clientInfo":{"name":"local_mcp","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}})).await
            .map_err(|e|anyhow::anyhow!("App Server initialization failed: {}",e.code))?;
        server
            .send(json!({"method":"initialized","params":{}}))
            .await?;
        Ok((server, rx))
    }
    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
    async fn send(&self, message: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&message)?;
        bytes.push(b'\n');
        tokio::time::timeout(self.timeout, async {
            let mut writer = self.writer.lock().await;
            writer
                .as_mut()
                .context("backend connection closed")?
                .write_all(&bytes)
                .await?;
            writer.as_mut().unwrap().flush().await?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("backend write acknowledgement unknown")??;
        Ok(())
    }
    pub async fn exec(&self, job: String, params: Value) -> std::result::Result<(), RpcError> {
        if !self.alive() {
            return Err(RpcError {
                uncertain: false,
                code: "backend_unavailable",
            });
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.pending
            .lock()
            .unwrap()
            .insert(id, Pending::Command(job));
        if self
            .send(json!({"id":id,"method":"command/exec","params":params}))
            .await
            .is_err()
        {
            self.alive.store(false, Ordering::SeqCst);
            let _ = self.events.send(Event::Lost).await;
            self.pending.lock().unwrap().remove(&id);
            return Err(RpcError {
                uncertain: true,
                code: "backend_unavailable",
            });
        }
        Ok(())
    }
    pub async fn control(
        &self,
        method: &str,
        params: Value,
    ) -> std::result::Result<Value, RpcError> {
        let _permit = self
            .controls
            .clone()
            .try_acquire_owned()
            .map_err(|_| RpcError {
                uncertain: false,
                code: "rpc_capacity",
            })?;
        if !self.alive() {
            return Err(RpcError {
                uncertain: false,
                code: "backend_unavailable",
            });
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap()
            .insert(id, Pending::Control(tx));
        if self
            .send(json!({"id":id,"method":method,"params":params}))
            .await
            .is_err()
        {
            self.pending.lock().unwrap().remove(&id);
            self.alive.store(false, Ordering::SeqCst);
            let _ = self.events.send(Event::Lost).await;
            return Err(RpcError {
                uncertain: true,
                code: "backend_unavailable",
            });
        }
        match tokio::time::timeout(
            if method == "initialize" {
                self.timeout.max(Duration::from_secs(10))
            } else {
                self.timeout
            },
            rx,
        )
        .await
        {
            Ok(Ok(value)) => value,
            _ => {
                self.pending.lock().unwrap().remove(&id);
                Err(RpcError {
                    uncertain: true,
                    code: "ack_unknown",
                })
            }
        }
    }
    pub async fn shutdown(&self, timeout: Duration) -> bool {
        // Closing the original stdio is also the official connection-scoped process cleanup path.
        let normal_requested = self.alive();
        self.writer.lock().await.take();
        let mut child = self.child.lock().await;
        if let Ok(Ok(status)) = tokio::time::timeout(timeout, child.wait()).await {
            return normal_requested && status.success();
        }
        let _ = child.start_kill();
        let _ = tokio::time::timeout(timeout, child.wait()).await;
        false
    }
}

/// Verify required fields against a schema generated by the actual executable.
/// This is not an exact-version allowlist and never falls back to internal crates.
pub async fn check_capabilities(executable: &Path, session: &Session) -> Result<()> {
    let dir = session.diagnostics_directory.join("schema-check");
    let output = Command::new(executable)
        .args(["app-server", "generate-json-schema", "--out"])
        .arg(&dir)
        .env_clear()
        .envs(&session.environment)
        .env("CODEX_HOME", &session.diagnostics_directory)
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .kill_on_drop(true)
        .status();
    ensure!(
        tokio::time::timeout(Duration::from_secs(30), output)
            .await??
            .success(),
        "App Server schema generation failed"
    );
    for (file, fields) in [
        (
            "CommandExecParams.json",
            vec![
                "disableTimeout",
                "disableOutputCap",
                "streamStdoutStderr",
                "streamStdin",
                "env",
                "processId",
                "sandboxPolicy",
                "tty",
                "size",
            ],
        ),
        (
            "CommandExecWriteParams.json",
            vec!["deltaBase64", "closeStdin", "processId"],
        ),
        ("CommandExecResizeParams.json", vec!["size", "processId"]),
        ("CommandExecTerminateParams.json", vec!["processId"]),
        (
            "CommandExecOutputDeltaNotification.json",
            vec!["processId", "stream", "deltaBase64", "capReached"],
        ),
    ] {
        let bytes = tokio::fs::read(dir.join("v2").join(file))
            .await
            .context("required App Server schema missing")?;
        let schema: Value = serde_json::from_slice(&bytes)?;
        for field in fields {
            ensure!(
                schema["properties"].get(field).is_some(),
                "required App Server field missing: {file}:{field}"
            );
        }
    }
    let requests = tokio::fs::read_to_string(dir.join("ClientRequest.json")).await?;
    for method in [
        "initialize",
        "command/exec",
        "command/exec/write",
        "command/exec/resize",
        "command/exec/terminate",
    ] {
        ensure!(
            requests.contains(&format!("\"{method}\"")),
            "required App Server method missing: {method}"
        );
    }
    let notifications = tokio::fs::read_to_string(dir.join("ServerNotification.json")).await?;
    ensure!(
        notifications.contains("\"command/exec/outputDelta\""),
        "required output notification missing"
    );
    tokio::fs::remove_dir_all(dir).await?;
    Ok(())
}
