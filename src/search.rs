//! Bounded, read-only ripgrep adapters. No shell, index, or persisted results.
use crate::{jobs::Jobs, protocol::*};
use rmcp::model::CallToolResult;
use serde_json::{Value, json};
use std::{io, process::Stdio, sync::Arc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

async fn available(jobs: &Jobs, executable: &std::path::Path) -> bool {
    let mut child = match tokio::process::Command::new(executable)
        .args(["--no-config", "--version"])
        .current_dir(&jobs.session.cwd)
        .env_clear()
        .envs(&jobs.session.environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let stdout = child.stdout.take().unwrap();
    let probe = async {
        let mut bytes = Vec::new();
        if stdout.take(1025).read_to_end(&mut bytes).await.is_err() || bytes.len() > 1024 {
            return false;
        }
        child.wait().await.is_ok_and(|status| status.success()) && bytes.starts_with(b"ripgrep ")
    };
    let ok = tokio::select! {
        result=tokio::time::timeout(Duration::from_millis(jobs.session.settings.rpc_timeout_ms),probe) => result.unwrap_or(false),
        _=jobs.until_closing()=>false,
    };
    if !ok {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    ok
}

pub async fn list(jobs: Arc<Jobs>, args: ListFilesArgs) -> CallToolResult {
    run(jobs, args, None).await
}
pub async fn search(jobs: Arc<Jobs>, args: SearchArgs) -> CallToolResult {
    let options = ListFilesArgs {
        session_id: args.session_id,
        runtime_id: args.runtime_id,
        path: args.path,
        glob: args.glob,
        hidden: args.hidden,
        respect_ignore: args.respect_ignore,
        limit: args.limit,
    };
    run(
        jobs,
        options,
        Some((args.pattern, args.regex.unwrap_or(false))),
    )
    .await
}

async fn frame(
    reader: &mut BufReader<tokio::process::ChildStdout>,
    delimiter: u8,
    cap: usize,
) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    let count = reader
        .take((cap + 1) as u64)
        .read_until(delimiter, &mut bytes)
        .await?;
    if count == 0 {
        return Ok(None);
    }
    if count > cap {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "search frame limit",
        ));
    }
    if bytes.last() == Some(&delimiter) {
        bytes.pop();
    }
    Ok(Some(bytes))
}

async fn run(
    jobs: Arc<Jobs>,
    args: ListFilesArgs,
    query: Option<(String, bool)>,
) -> CallToolResult {
    if let Err(error) = jobs.validate_scope(&args.session_id, &args.runtime_id) {
        return failure("scope_mismatch", error.to_string());
    }
    let limit = args.limit.unwrap_or(50);
    if !(1..=500).contains(&limit) {
        return invalid_argument("limit", "1..=500 results");
    }
    if args
        .path
        .as_ref()
        .is_some_and(|p| p.len() > 4096 || p.contains('\0'))
    {
        return invalid_argument("path", "a path of at most 4096 bytes without NUL");
    }
    if args
        .glob
        .as_ref()
        .is_some_and(|p| p.len() > 4096 || p.contains('\0'))
    {
        return invalid_argument("glob", "a ripgrep glob of at most 4096 bytes without NUL");
    }
    if query.as_ref().is_some_and(|(pattern, _)| {
        pattern.is_empty() || pattern.len() > 4096 || pattern.contains(['\0', '\r', '\n'])
    }) {
        return invalid_argument(
            "pattern",
            "a single-line pattern of 1..=4096 bytes without NUL",
        );
    }
    let Some(executable) = &jobs.session.search_executable else {
        return failure(
            "feature_unavailable",
            "no ripgrep executable outside the session write and expansion roots is available",
        );
    };
    if !available(&jobs, executable).await {
        return failure(
            "feature_unavailable",
            "the trusted search executable did not pass the bounded ripgrep capability check",
        );
    }
    let path = jobs.session.cwd.join(args.path.as_deref().unwrap_or("."));
    let path = match tokio::fs::canonicalize(path).await {
        Ok(path) => path,
        Err(_) => return failure("search_failed", "cannot resolve the requested search path"),
    };
    if path.to_str().is_none() {
        return failure(
            "search_failed",
            "the resolved search path cannot be represented as UTF-8",
        );
    }
    let mut command = tokio::process::Command::new(executable);
    command
        .args(["--no-config", "--color", "never", "--no-follow"])
        .current_dir(&jobs.session.cwd)
        .env_clear()
        .envs(&jobs.session.environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if args.hidden.unwrap_or(false) {
        command.arg("--hidden");
    }
    if !args.respect_ignore.unwrap_or(true) {
        command.arg("--no-ignore");
    }
    if let Some(glob) = &args.glob {
        command.arg("--glob").arg(glob);
    }
    if let Some((pattern, regex)) = &query {
        command.args(["--json", "--line-number"]);
        if !regex {
            command.arg("--fixed-strings");
        }
        command.arg("--").arg(pattern).arg(&path);
    } else {
        command.args(["--files", "--null", "--"]).arg(&path);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            return failure(
                "feature_unavailable",
                "the selected ripgrep executable could not start",
            );
        }
    };
    let mut reader = BufReader::new(child.stdout.take().expect("piped stdout"));
    let mut stderr = child.stderr.take().expect("piped stderr");
    // Drain diagnostics to avoid pipe deadlocks, retaining a small prefix only.
    let errors = tokio::spawn(async move {
        let mut prefix = Vec::new();
        let mut buffer = [0u8; 1024];
        while let Ok(count) = stderr.read(&mut buffer).await {
            if count == 0 {
                break;
            }
            let retain = count.min(4096usize.saturating_sub(prefix.len()));
            prefix.extend_from_slice(&buffer[..retain]);
        }
        prefix
    });
    let cap = 65536.min(jobs.session.settings.max_message_bytes / 32);
    let frame_cap = jobs.session.settings.max_message_bytes / 16;
    let mut results = Vec::new();
    let mut used = 0;
    let mut skipped = 0usize;
    let deadline =
        tokio::time::Instant::now() + Duration::from_millis(jobs.session.settings.rpc_timeout_ms);
    let consume = async {
        loop {
            let bytes = match frame(
                &mut reader,
                if query.is_some() { b'\n' } else { 0 },
                frame_cap,
            )
            .await
            {
                Ok(Some(bytes)) => bytes,
                Ok(None) => return None,
                Err(_) => return Some("frame_limit"),
            };
            let item = if query.is_some() {
                let event: Value = match serde_json::from_slice(&bytes) {
                    Ok(event) => event,
                    Err(_) => return Some("invalid_backend_output"),
                };
                if event["type"] != "match" {
                    continue;
                }
                let data = &event["data"];
                let (Some(path), Some(text), Some(line)) = (
                    data["path"]["text"].as_str(),
                    data["lines"]["text"].as_str(),
                    data["line_number"].as_u64(),
                ) else {
                    skipped += 1;
                    continue;
                };
                json!({"path":path,"line":line,"text":text})
            } else {
                match String::from_utf8(bytes) {
                    Ok(path) => json!(path),
                    Err(_) => {
                        skipped += 1;
                        continue;
                    }
                }
            };
            let bytes = serde_json::to_vec(&item).expect("search item").len();
            if results.len() == limit {
                return Some("result_limit");
            }
            if used + bytes > cap {
                return Some("output_limit");
            }
            used += bytes;
            results.push(item);
        }
    };
    let mut reason = tokio::select! {
        reason = consume => reason,
        _ = tokio::time::sleep_until(deadline) => Some("time_limit"),
        _ = jobs.until_closing() => Some("session_closing"),
    };
    if reason.is_some() {
        let _ = child.start_kill();
    }
    let status = match tokio::time::timeout_at(
        deadline.max(tokio::time::Instant::now() + Duration::from_millis(100)),
        child.wait(),
    )
    .await
    {
        Ok(Ok(status)) => Some(status),
        _ => {
            reason = Some("time_limit");
            let _ = child.kill().await;
            let _ = child.wait().await;
            None
        }
    };
    let stderr = errors.await.unwrap_or_default();
    if reason.is_none() && status.is_some_and(|s| s.code() != Some(0) && s.code() != Some(1)) {
        let diagnostic = String::from_utf8_lossy(&stderr);
        if diagnostic.contains("regex parse error") {
            return invalid_argument("pattern", "a valid ripgrep regular expression");
        }
        if diagnostic.contains("error parsing glob") {
            return invalid_argument("glob", "a valid ripgrep glob");
        }
        if results.is_empty() {
            return failure(
                "search_failed",
                "ripgrep could not read the requested targets or rejected the search options",
            );
        }
        reason = Some("read_errors");
    }
    let mut value = json!({"session_id":jobs.session.id,"runtime_id":jobs.session.runtime_id,
        "path":path,"truncated":reason.is_some(),"truncation_reason":reason,
        "skipped_non_utf8":skipped,"partial":reason.is_some() || !stderr.is_empty() || skipped>0});
    value[if query.is_some() { "matches" } else { "files" }] = json!(results);
    success(value)
}
