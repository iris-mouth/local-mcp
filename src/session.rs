use crate::{
    app_server::{self, AppServer},
    config::{self, Settings},
    jobs::Jobs,
    protocol::*,
    rpc_capacity::RpcCapacity,
};
use anyhow::{Context, Result, ensure};
use rmcp::model::CallToolResult;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::io::{AsyncWriteExt, BufReader};

fn parse<T: DeserializeOwned>(args: Value) -> Result<T> {
    Ok(serde_json::from_value(args)?)
}
pub async fn dispatch(jobs: Arc<Jobs>, call: ToolCall) -> CallToolResult {
    let image_response = call.name == "view_image";
    let identifiers = call.arguments.clone();
    let mut result = dispatch_inner(jobs, call).await;
    if let Some(Value::Object(payload)) = &mut result.structured_content {
        for key in ["session_id", "runtime_id", "job_id", "request_id"] {
            if let Some(value) = identifiers.get(key).and_then(Value::as_str)
                && value.len() <= 128
            {
                payload.insert(key.into(), Value::String(value.into()));
            }
        }
        result.content[0] =
            rmcp::model::ContentBlock::text(serde_json::to_string(payload).unwrap());
    }
    // Some hosts expose structuredContent alone and discard sibling image blocks.
    // Keep the scoped JSON receipt in content, alongside the native image.
    if image_response && result.is_error != Some(true) {
        result.structured_content = None;
    }
    result
}
async fn dispatch_inner(jobs: Arc<Jobs>, call: ToolCall) -> CallToolResult {
    macro_rules! parsed {
        ($ty:ty) => {
            match parse::<$ty>(call.arguments) {
                Ok(v) => v,
                Err(_) => return failure("invalid_arguments", "invalid or unknown tool fields"),
            }
        };
    }
    match call.name.as_str() {
        "session_info" => {
            let args = parsed!(InfoArgs);
            if args.session_id != jobs.session.id {
                return failure("session_mismatch", "wrong session_id");
            }
            jobs.info()
        }
        "exec_command" => jobs.execute(parsed!(ExecArgs), None, None).await,
        "exec_command_expanded" => {
            let args = parsed!(ExpandedArgs);
            jobs.execute(args.exec(), Some(args.access), Some(args.reason))
                .await
        }
        "command_status" => jobs.status(parsed!(StatusArgs)).await,
        "write_stdin" => jobs.input(parsed!(InputArgs)).await,
        "stop_command" => jobs.stop(parsed!(StopArgs)).await,
        "apply_diff" => jobs.apply_diff(parsed!(DiffArgs)).await,
        "read_file" => {
            let args = parsed!(ReadArgs);
            if let Err(e) = jobs.validate_scope(&args.session_id, &args.runtime_id) {
                return failure("scope_mismatch", e.to_string());
            }
            match crate::files::read(&jobs.session, args).await {
                Ok(v) => v,
                Err(e) => failure("read_failed", e.to_string()),
            }
        }
        "view_image" => {
            let args = parsed!(ImageArgs);
            if let Err(e) = jobs.validate_scope(&args.session_id, &args.runtime_id) {
                return failure("scope_mismatch", e.to_string());
            }
            match crate::files::image(&jobs.session, args).await {
                Ok(v) => v,
                Err(e) => failure("image_failed", e.to_string()),
            }
        }
        _ => failure("unknown_tool", "unknown tool name"),
    }
}

#[cfg(unix)]
struct SocketGuard {
    socket: PathBuf,
    lock: PathBuf,
    _file: std::fs::File,
}
#[cfg(unix)]
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_file(&self.lock);
    }
}
#[cfg(unix)]
fn bind(id: &str) -> Result<(tokio::net::UnixListener, SocketGuard)> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    let directory = config::socket_directory();
    match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let meta = std::fs::symlink_metadata(&directory)?;
    ensure!(
        meta.is_dir()
            && !meta.file_type().is_symlink()
            && meta.uid() == unsafe { libc::geteuid() }
            && meta.mode() & 0o077 == 0,
        "insecure session socket directory"
    );
    let path = config::socket_path(id)?;
    let lock = path.with_extension("lock");
    let file=std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&lock)
        .context("session already running or stale lock exists; inspect the old start before removing its lock/socket")?;
    let listener = match tokio::net::UnixListener::bind(&path) {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_file(&lock);
            return Err(e).context("session socket exists; it was not overwritten");
        }
    };
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok((
        listener,
        SocketGuard {
            socket: path,
            lock,
            _file: file,
        },
    ))
}

#[cfg(unix)]
pub async fn start(id: Option<String>, settings: Settings, codex: PathBuf) -> Result<()> {
    start_inner(id, settings, codex, std::env::current_dir()?, None).await
}

#[cfg(unix)]
pub async fn start_connected(
    id: Option<String>,
    settings: Settings,
    codex: PathBuf,
    cwd: PathBuf,
    connection: crate::connection::Connection,
) -> Result<()> {
    start_inner(id, settings, codex, cwd, Some(connection)).await
}

#[cfg(unix)]
async fn start_inner(
    id: Option<String>,
    settings: Settings,
    codex: PathBuf,
    cwd: PathBuf,
    mut connection: Option<crate::connection::Connection>,
) -> Result<()> {
    settings.validate()?;
    let id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    config::validate_session_id(&id)?;
    let (listener, _socket) = bind(&id)?;
    let directory = tempfile::Builder::new()
        .prefix("local-mcp-runtime-")
        .tempdir()?
        .keep();
    let session = config::Session::new(id, cwd, &directory, settings)?;
    eprintln!(
        "local-mcp session: {}\nruntime: {}\ncwd: {}\nwrite roots: {:?}\nGit metadata candidates: {:?}\nexpanded ceiling: roots={:?}, network={}, full={}\ndiagnostics (may contain command bodies): {}\nmax_jobs: {}\nPress Ctrl-C or close stdin to stop. /status prints current state.",
        session.id,
        session.runtime_id,
        session.cwd.display(),
        session.roots,
        session.git_metadata_candidates,
        session.extra_roots,
        session.settings.allow_network,
        session.settings.allow_full_access,
        session.diagnostics_directory.display(),
        session.settings.max_jobs
    );
    if let Err(error) = app_server::check_capabilities(&codex, &session).await {
        eprintln!(
            "Startup failed; dedicated diagnostic residue: {}",
            directory.display()
        );
        return Err(error);
    }
    // Schema checking retains default terminal signal handling. Once the backend
    // is isolated, handle interrupts even while its initialization is pending.
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let (backend, events) = AppServer::spawn_interruptible(&codex, &session, async {
        tokio::select! {
            _=interrupt.recv()=>{},
            _=terminate.recv()=>{},
            _=hangup.recv()=>{},
        }
    })
    .await?;
    let jobs = Jobs::new(session, backend, events);
    if let Some(connection) = &mut connection
        && let Err(error) = connection.start(&jobs.session.id)
    {
        // The owner can still observe/stop existing work while connectivity is down.
        eprintln!("Tunnel startup failed: {error}; use /tunnel-restart after resolving it.");
    }
    eprintln!("Session ready: {}", jobs.session.id);
    let readers = Arc::new(tokio::sync::Semaphore::new(
        jobs.session.settings.max_rpc_inflight,
    ));
    let requests = RpcCapacity::new(jobs.session.settings.max_rpc_inflight);
    // Tokio stdin uses a blocking runtime task that cannot be cancelled. An open
    // terminal would keep runtime shutdown waiting even after the owner returned.
    // A dedicated thread may remain blocked until process exit without pinning it.
    let (input, mut stdin) = tokio::sync::mpsc::channel(8);
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::stdin().lock().lines() {
            if input.blocking_send(line).is_err() {
                break;
            }
        }
    });
    let mut tunnel_poll = tokio::time::interval(Duration::from_millis(250));
    let result: Result<()> = loop {
        tokio::select! {
            _=tunnel_poll.tick(), if connection.is_some()=>{
                if let Some(connection)=&mut connection && let Err(error)=connection.tick(&jobs.session.id) {
                    eprintln!("Tunnel supervision failed: {error}. Inspect connectivity with /status.");
                }
            }
            _=interrupt.recv()=>{break Ok(());}
            _=terminate.recv()=>{break Ok(());}
            _=hangup.recv()=>{break Ok(());}
            line=stdin.recv()=>{match line {
                None=>break Ok(()),
                Some(Ok(line)) if line.trim()=="/status"=>{
                    eprintln!("{}",jobs.info().structured_content.unwrap());
                    if let Some(connection)=&connection { connection.status(); }
                },
                Some(Ok(line)) if line.trim()=="/tunnel-restart" && connection.is_some()=>{
                    if let Err(error)=connection.as_mut().unwrap().restart(&jobs.session.id).await { eprintln!("Tunnel restart failed: {error}"); }
                },
                Some(Ok(_))=>eprintln!("Use startup --config for trusted permission ceilings. /status or Ctrl-C."),
                Some(Err(e))=>break Err(e.into())
            }}
            connection=async {
                let permit=readers.clone().acquire_owned().await.expect("IPC reader capacity closed");
                listener.accept().await.map(|(stream,_)|(stream,permit))
            }=>{
                let (stream,permit)=match connection{Ok(c)=>c,Err(e)=>break Err(e.into())};
                let jobs=jobs.clone();
                let requests=requests.clone();
                tokio::spawn(async move {
                    // Peer identity supplements the existing per-user 0700 directory and 0600 socket.
                    if !stream.peer_cred().is_ok_and(|cred|cred.uid()==unsafe{libc::geteuid()}){return;}
                    let mut reader=BufReader::new(stream);
                    let cap=jobs.session.settings.max_message_bytes;
                    let Ok(Ok(Some(line)))=tokio::time::timeout(Duration::from_secs(5),read_frame(&mut reader,cap)).await else{return;};
                    let call=serde_json::from_slice::<ToolCall>(&line);
                    let request_permit=call.as_ref().ok().and_then(|call|requests.acquire(call));
                    // Keep either admission or request capacity until the bounded reply completes.
                    let _reader_permit=if request_permit.is_some(){drop(permit);None}else{Some(permit)};
                    let result=match (call,&request_permit){
                        (Ok(call),Some(_))=>dispatch(jobs,call).await,
                        (Ok(_),None)=>failure("rpc_capacity","session request capacity reached"),
                        (Err(_),_)=>failure("invalid_arguments","invalid session message")
                    };
                    let Ok(mut bytes)=serde_json::to_vec(&result)else{return;};
                    if bytes.len()>cap {bytes=serde_json::to_vec(&failure("delivery_limit","result exceeds session transport budget")).unwrap();}
                    bytes.push(b'\n');
                    let _=tokio::time::timeout(Duration::from_secs(5),reader.get_mut().write_all(&bytes)).await;
                });
            }
        }
    };
    let tunnel_result = match &mut connection {
        Some(connection) => connection.stop().await,
        None => Ok(()),
    };
    let confirmed = jobs.shutdown().await;
    if confirmed {
        tokio::fs::remove_dir_all(&directory)
            .await
            .context("cannot remove owned runtime directory")?;
    } else {
        eprintln!(
            "Backend exit was not confirmed during normal shutdown. Residue: {}",
            directory.display()
        );
    }
    result.and(tunnel_result)
}
#[cfg(not(unix))]
pub async fn start(_: Option<String>, _: Settings, _: PathBuf) -> Result<()> {
    anyhow::bail!("this adapter is qualified on macOS only")
}

pub async fn forward(call: ToolCall) -> CallToolResult {
    let Some(id) = call.arguments["session_id"].as_str() else {
        return failure("invalid_arguments", "session_id required");
    };
    if let Ok(expected) = std::env::var("LOCAL_MCP_SESSION")
        && expected != id
    {
        return failure(
            "session_mismatch",
            "session differs from trusted startup selection",
        );
    }
    let path = match config::socket_path(id) {
        Ok(p) => p,
        Err(_) => return failure("invalid_arguments", "invalid session_id"),
    };
    #[cfg(unix)]
    {
        let stream = match tokio::net::UnixStream::connect(path).await {
            Ok(v) => v,
            Err(_) => {
                return failure(
                    "session_unavailable",
                    "start the requested local-mcp session",
                );
            }
        };
        if !stream
            .peer_cred()
            .is_ok_and(|cred| cred.uid() == unsafe { libc::geteuid() })
        {
            return failure("session_unavailable", "unexpected session owner");
        }
        let mut bytes = match serde_json::to_vec(&call) {
            Ok(v) => v,
            Err(_) => return failure("invalid_arguments", "invalid arguments"),
        };
        if bytes.len() > 64 * 1024 * 1024 {
            return failure("delivery_limit", "request exceeds transport limit");
        }
        bytes.push(b'\n');
        let mut reader = BufReader::new(stream);
        if reader.get_mut().write_all(&bytes).await.is_err() {
            return failure(
                "dispatch_unknown",
                "session connection failed; inspect the original request_id before another change",
            );
        }
        match read_frame(&mut reader, 64 * 1024 * 1024).await {
            Ok(Some(bytes)) => serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                failure(
                    "result_unknown",
                    "invalid session result; inspect original request_id",
                )
            }),
            _ => failure(
                "result_unknown",
                "session connection lost; inspect original request_id; execution may have started",
            ),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        failure("unsupported_platform", "macOS/Unix required")
    }
}
