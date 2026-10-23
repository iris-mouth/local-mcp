use crate::{
    app_server::{AppServer, Event},
    config::Session,
    protocol::*,
};
use anyhow::{Result, ensure};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Notify, mpsc};

#[derive(Default)]
struct Output {
    bytes: VecDeque<u8>,
    start: u64,
    end: u64,
    source_truncated: bool,
    expired: bool,
}
impl Output {
    fn push(&mut self, bytes: &[u8], cap: usize) {
        self.expired = false;
        self.end += bytes.len() as u64;
        self.bytes.extend(bytes);
        self.trim(self.bytes.len().saturating_sub(cap));
    }
    fn trim(&mut self, count: usize) {
        let count = count.min(self.bytes.len());
        self.bytes.drain(..count);
        self.start += count as u64;
    }
    fn page(&self, position: u64, cap: usize) -> (Value, u64) {
        let actual = position.max(self.start).min(self.end);
        let bytes: Vec<u8> = self
            .bytes
            .iter()
            .skip((actual - self.start) as usize)
            .take(cap)
            .copied()
            .collect();
        let end = actual + bytes.len() as u64;
        (
            json!({"text":String::from_utf8_lossy(&bytes),"lossy":std::str::from_utf8(&bytes).is_err(),
            "start_byte":actual,"end_byte":end,"source_end_byte":self.end,"retained_start_byte":self.start,
            "gap":position<self.start,"has_more":end<self.end,"source_truncated":self.source_truncated,"expired":self.expired}),
            end,
        )
    }
}
#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum State {
    Accepted,
    Running,
    StopRequested,
    Finished,
    Unknown,
}
struct Job {
    state: State,
    exit_code: Option<i64>,
    error: Option<&'static str>,
    output_error: Option<&'static str>,
    input: InputMode,
    input_closed: bool,
    input_unknown: bool,
    input_busy: bool,
    stop_attempted: bool,
    stop_ack: Option<Value>,
    process_id: String,
    stdout: Output,
    stderr: Output,
    last_output: Instant,
    completed: Option<Instant>,
    diff_apply: Option<Value>,
    diff_input: Option<String>,
    diff_patch: Option<String>,
    effective_access: Value,
}
struct Receipt {
    digest: Vec<u8>,
    job_id: String,
    input_result: Option<Value>,
    input: bool,
}
#[derive(Default)]
struct Store {
    jobs: HashMap<String, Job>,
    receipts: HashMap<String, Receipt>,
    process_jobs: HashMap<String, String>,
    closing: bool,
}
pub struct Jobs {
    pub session: Session,
    pub backend: Arc<AppServer>,
    store: Mutex<Store>,
    changed: Notify,
    diff_lock: tokio::sync::Mutex<()>,
    dispatch_lock: tokio::sync::Mutex<()>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    runtime: String,
    job: String,
    stdout: u64,
    stderr: u64,
}
enum Action {
    Exec(String, Value),
    Input(String, Value),
}
impl Jobs {
    pub fn new(
        session: Session,
        backend: Arc<AppServer>,
        mut events: mpsc::Receiver<Event>,
    ) -> Arc<Self> {
        let jobs = Arc::new(Self {
            session,
            backend,
            store: Mutex::new(Store::default()),
            changed: Notify::new(),
            diff_lock: tokio::sync::Mutex::new(()),
            dispatch_lock: tokio::sync::Mutex::new(()),
        });
        let weak = Arc::downgrade(&jobs);
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                let Some(jobs) = weak.upgrade() else {
                    break;
                };
                let follow = jobs.event(event);
                jobs.changed.notify_waiters();
                if let Some(action) = follow {
                    // Keep draining output while a control acknowledgement is pending.
                    // Otherwise a full notification queue could prevent reading that ACK.
                    tokio::spawn(async move {
                        match action {
                            Action::Exec(id, params) => jobs.dispatch(id, params).await,
                            Action::Input(id, params) => {
                                let _dispatch = jobs.dispatch_lock.lock().await;
                                let send = {
                                    let store = jobs.store.lock().unwrap();
                                    !store.closing
                                        && store
                                            .jobs
                                            .get(&id)
                                            .is_some_and(|j| j.state != State::StopRequested)
                                };
                                if send
                                    && let Err(error) =
                                        jobs.backend.control("command/exec/write", params).await
                                {
                                    jobs.unknown(&id, error.code);
                                }
                            }
                        }
                    });
                }
            }
        });
        let weak = Arc::downgrade(&jobs);
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(Duration::from_secs(1));
            loop {
                timer.tick().await;
                let Some(jobs) = weak.upgrade() else {
                    break;
                };
                jobs.expire(&mut jobs.store.lock().unwrap());
            }
        });
        jobs
    }
    async fn wait_for_exit(&self, id: &str, millis: u64) {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(millis);
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let done = self
                .store
                .lock()
                .unwrap()
                .jobs
                .get(id)
                .is_none_or(|job| job.state == State::Finished || job.state == State::Unknown);
            if done || tokio::time::Instant::now() >= deadline {
                return;
            }
            let _ = tokio::time::timeout_at(deadline, changed).await;
        }
    }
    fn unknown(&self, id: &str, code: &'static str) {
        if let Some(job) = self.store.lock().unwrap().jobs.get_mut(id)
            && job.state != State::Finished
        {
            job.state = State::Unknown;
            job.error = Some(code);
        }
        self.changed.notify_waiters();
    }
    fn event(&self, event: Event) -> Option<Action> {
        let mut store = self.store.lock().unwrap();
        match event {
            Event::Output(params) => {
                let process = params["processId"].as_str()?;
                let id = store.process_jobs.get(process).cloned()?;
                let job = store.jobs.get_mut(&id)?;
                let Some(encoded) = params["deltaBase64"].as_str() else {
                    job.output_error = Some("invalid_backend_output");
                    return None;
                };
                let Ok(bytes) = STANDARD.decode(encoded) else {
                    job.output_error = Some("invalid_backend_output");
                    return None;
                };
                let stream = match params["stream"].as_str() {
                    Some("stdout") => &mut job.stdout,
                    Some("stderr") => &mut job.stderr,
                    _ => {
                        job.output_error = Some("invalid_backend_output");
                        return None;
                    }
                };
                stream.push(&bytes, self.session.settings.output_bytes_per_job / 2);
                stream.source_truncated |= params["capReached"].as_bool().unwrap_or(false);
                job.last_output = Instant::now();
                let input=job.diff_input.take().map(|patch|Action::Input(id.clone(),json!({"processId":job.process_id,"deltaBase64":STANDARD.encode(patch),"closeStdin":true})));
                self.trim_total(&mut store);
                if input.is_some() {
                    return input;
                }
            }
            Event::Finished(id, result) => {
                let closing = store.closing;
                let job = store.jobs.get_mut(&id)?;
                match result {
                    Ok(result) => {
                        let Some(code) = result["exitCode"].as_i64() else {
                            job.state = State::Unknown;
                            job.error = Some("missing_exit_result");
                            return None;
                        };
                        // Streaming results must not duplicate notification bytes.
                        if !result["stdout"].as_str().unwrap_or("").is_empty()
                            || !result["stderr"].as_str().unwrap_or("").is_empty()
                        {
                            job.output_error = Some("incompatible_streaming_result");
                        }
                        if code == 0
                            && let Some(params) = job.diff_apply.take()
                        {
                            if closing || job.state == State::StopRequested {
                                job.state = State::Finished;
                                job.exit_code = Some(code);
                                job.error = Some("diff_stopped_before_apply");
                                job.completed = Some(Instant::now());
                                return None;
                            }
                            job.process_id = params["processId"].as_str()?.to_owned();
                            job.state = State::Running;
                            job.diff_input = job.diff_patch.take();
                            let process = job.process_id.clone();
                            store.process_jobs.insert(process, id.clone());
                            return Some(Action::Exec(id, params));
                        }
                        job.diff_apply = None;
                        job.diff_input = None;
                        job.diff_patch = None;
                        job.state = State::Finished;
                        job.exit_code = Some(code);
                        job.completed = Some(Instant::now());
                    }
                    Err(error) => {
                        job.state = State::Unknown;
                        job.diff_apply = None;
                        job.diff_input = None;
                        job.diff_patch = None;
                        job.error = Some(error.code);
                    }
                }
            }
            Event::Lost => {
                for job in store.jobs.values_mut() {
                    if job.state != State::Finished {
                        job.state = State::Unknown;
                        job.diff_apply = None;
                        job.diff_input = None;
                        job.diff_patch = None;
                        job.error = Some("backend_unavailable");
                    }
                }
            }
        }
        None
    }
    fn expire(&self, store: &mut Store) {
        let ttl = Duration::from_secs(self.session.settings.output_ttl_secs);
        for job in store.jobs.values_mut() {
            if job.completed.is_some_and(|t| t.elapsed() >= ttl) {
                for stream in [&mut job.stdout, &mut job.stderr] {
                    stream.trim(stream.bytes.len());
                    stream.expired = true;
                }
            }
        }
    }
    fn trim_total(&self, store: &mut Store) {
        let total: usize = store
            .jobs
            .values()
            .map(|j| j.stdout.bytes.len() + j.stderr.bytes.len())
            .sum();
        let mut excess = total.saturating_sub(self.session.settings.output_bytes_total);
        while excess > 0 {
            let Some(job) = store
                .jobs
                .values_mut()
                .filter(|j| !j.stdout.bytes.is_empty() || !j.stderr.bytes.is_empty())
                .min_by_key(|j| j.last_output)
            else {
                break;
            };
            let n = excess.min(job.stdout.bytes.len());
            job.stdout.trim(n);
            excess -= n;
            let n = excess.min(job.stderr.bytes.len());
            job.stderr.trim(n);
            excess -= n;
        }
    }
    fn digest<T: Serialize>(name: &str, args: &T) -> Vec<u8> {
        let mut hash = Sha256::new();
        hash.update(name);
        hash.update(serde_json::to_vec(args).expect("serializable args"));
        hash.finalize().to_vec()
    }
    fn check_id(id: &str) -> Result<()> {
        ensure!(
            !id.is_empty() && id.len() <= 128,
            "request_id must be 1..128 bytes"
        );
        Ok(())
    }
    pub fn info(&self) -> rmcp::model::CallToolResult {
        let mut store = self.store.lock().unwrap();
        self.expire(&mut store);
        success(
            json!({"session_id":self.session.id,"runtime_id":self.session.runtime_id,
            "session":self.session,"backend_available":self.backend.alive(),"closing":store.closing,
            "active_jobs":store.jobs.values().filter(|j|j.state!=State::Finished).count(),
            "receipt_count":store.receipts.len(),"read_scope":"host_user","input_ack":"backend_acceptance_only"}),
        )
    }
    pub fn validate_scope(&self, session: &str, runtime: &str) -> Result<()> {
        ensure!(session == self.session.id, "session_mismatch");
        ensure!(runtime == self.session.runtime_id, "runtime_mismatch");
        Ok(())
    }
    pub fn execution_params(
        &self,
        args: &ExecArgs,
        access: Option<&Access>,
    ) -> Result<(Value, Value)> {
        ensure!(
            !args.argv.is_empty() && args.argv.len() <= 4096,
            "argv must be nonempty and bounded"
        );
        ensure!(
            args.argv.iter().all(|v| !v.contains('\0')),
            "argv contains NUL"
        );
        ensure!(
            serde_json::to_vec(&args.argv)?.len() <= self.session.settings.max_diff_bytes,
            "argv exceeds input budget"
        );
        let input = args.input_mode.unwrap_or_default();
        ensure!(
            args.size.is_none() || input == InputMode::Pty,
            "size requires PTY"
        );
        if let Some(size) = &args.size {
            ensure!(size.rows > 0 && size.cols > 0, "invalid terminal size");
        }
        ensure!(
            args.wait_ms.unwrap_or(30000) <= 30000,
            "wait_ms must be 0..30000"
        );
        for root in self.session.roots.iter() {
            ensure!(
                crate::config::canonical_directory(root)? == *root,
                "write root changed after startup"
            );
        }
        let mut roots = self.session.roots.clone();
        let mut network = false;
        let mut full = false;
        match access {
            Some(Access::Full {}) => {
                ensure!(
                    self.session.settings.allow_full_access,
                    "full access disabled"
                );
                full = true;
                network = true;
            }
            Some(Access::Limited {
                roots: additional,
                network: requested,
            }) => {
                ensure!(
                    !*requested || self.session.settings.allow_network,
                    "network expansion disabled"
                );
                network = *requested;
                for root in additional {
                    let root = crate::config::canonical_directory(std::path::Path::new(root))?;
                    ensure!(
                        self.session.extra_roots.iter().any(|r| root.starts_with(r)),
                        "extra root exceeds fixed ceiling"
                    );
                    roots.push(root);
                }
            }
            None => {}
        }
        roots.sort();
        roots.dedup();
        let cwd = crate::config::canonical_directory(
            &self.session.cwd.join(args.cwd.as_deref().unwrap_or(".")),
        )?;
        ensure!(
            full || roots.iter().any(|r| cwd.starts_with(r)),
            "cwd outside effective write roots"
        );
        let policy = if full {
            json!({"type":"dangerFullAccess"})
        } else {
            json!({"type":"workspaceWrite","writableRoots":roots,"networkAccess":network,"excludeSlashTmp":true,"excludeTmpdirEnvVar":true})
        };
        let effective = json!({"mode":if full{"full"}else{"limited"},"roots":if full{json!(null)}else{json!(roots)},"network":network,"cwd":cwd});
        let mut params = json!({"command":args.argv,"cwd":cwd,"sandboxPolicy":policy,
            "disableTimeout":true,"disableOutputCap":true,"streamStdoutStderr":true,
            "streamStdin":input!=InputMode::Closed,"tty":input==InputMode::Pty,"env":self.session.environment});
        if let Some(size) = &args.size {
            params["size"] = serde_json::to_value(size)?;
        }
        Ok((params, effective))
    }
    pub async fn execute(
        self: &Arc<Self>,
        mut args: ExecArgs,
        access: Option<Access>,
        reason: Option<String>,
    ) -> rmcp::model::CallToolResult {
        let result = (|| -> Result<_> {
            self.validate_scope(&args.session_id, &args.runtime_id)?;
            Self::check_id(&args.request_id)?;
            if access.is_some() {
                ensure!(
                    reason
                        .as_ref()
                        .is_some_and(|s| !s.trim().is_empty() && s.len() <= 4096),
                    "expanded execution requires reason"
                );
            }
            args.wait_ms = Some(args.wait_ms.unwrap_or(30000));
            args.input_mode = Some(args.input_mode.unwrap_or_default());
            let digest = Self::digest("exec", &(&args, &access, &reason));
            {
                let store = self.store.lock().unwrap();
                if let Some(receipt) = store.receipts.get(&args.request_id) {
                    ensure!(receipt.digest == digest, "request_id_mismatch");
                    return Ok((receipt.job_id.clone(), None));
                }
            }
            let (params, effective) = self.execution_params(&args, access.as_ref())?;
            self.reserve(
                &args.request_id,
                digest,
                params,
                effective,
                args.input_mode.unwrap_or_default(),
                None,
            )
        })();
        match result {
            Ok((id, dispatch)) => {
                if let Some(params) = dispatch {
                    self.dispatch(id.clone(), params).await;
                }
                self.wait_for_exit(&id, args.wait_ms.unwrap_or(30000)).await;
                self.status(StatusArgs {
                    session_id: args.session_id,
                    runtime_id: args.runtime_id,
                    job_id: Some(id),
                    request_id: None,
                    cursor: None,
                    wait_ms: Some(0),
                })
                .await
            }
            Err(e) => failure_detail("execution_rejected", e.to_string()),
        }
    }
    fn reserve(
        &self,
        request: &str,
        digest: Vec<u8>,
        mut params: Value,
        effective: Value,
        input: InputMode,
        diff_apply: Option<Value>,
    ) -> Result<(String, Option<Value>)> {
        let mut store = self.store.lock().unwrap();
        self.expire(&mut store);
        if let Some(receipt) = store.receipts.get(request) {
            ensure!(receipt.digest == digest, "request_id_mismatch");
            return Ok((receipt.job_id.clone(), None));
        }
        ensure!(
            !store.closing && self.backend.alive(),
            "backend_unavailable"
        );
        ensure!(
            store.receipts.len() < self.session.settings.max_receipts,
            "receipt_capacity"
        );
        ensure!(
            store
                .jobs
                .values()
                .filter(|j| j.state != State::Finished)
                .count()
                < self.session.settings.max_jobs,
            "job_capacity"
        );
        let id = uuid::Uuid::new_v4().to_string();
        let process = uuid::Uuid::new_v4().to_string();
        params["processId"] = json!(process);
        let diff_apply = diff_apply.map(|mut p| {
            p["processId"] = json!(uuid::Uuid::new_v4().to_string());
            p
        });
        store.process_jobs.insert(process.clone(), id.clone());
        store.jobs.insert(
            id.clone(),
            Job {
                state: State::Accepted,
                exit_code: None,
                error: None,
                output_error: None,
                input,
                input_closed: input == InputMode::Closed,
                input_unknown: false,
                input_busy: false,
                stop_attempted: false,
                stop_ack: None,
                process_id: process,
                stdout: Output::default(),
                stderr: Output::default(),
                last_output: Instant::now(),
                completed: None,
                diff_apply,
                diff_input: None,
                diff_patch: None,
                effective_access: effective,
            },
        );
        store.receipts.insert(
            request.into(),
            Receipt {
                digest,
                job_id: id.clone(),
                input_result: None,
                input: false,
            },
        );
        Ok((id, Some(params)))
    }
    async fn dispatch(&self, id: String, params: Value) {
        let _dispatch = self.dispatch_lock.lock().await;
        {
            let mut store = self.store.lock().unwrap();
            let closing = store.closing;
            if let Some(job) = store.jobs.get(&id)
                && job.state == State::Unknown
            {
                return;
            }
            if let Some(job) = store.jobs.get_mut(&id)
                && (closing || job.state == State::StopRequested)
            {
                job.state = State::Finished;
                job.error = Some("stopped_before_dispatch");
                job.completed = Some(Instant::now());
                self.changed.notify_waiters();
                return;
            }
        }
        match self.backend.exec(id.clone(), params).await {
            Ok(()) => {
                if let Some(job) = self.store.lock().unwrap().jobs.get_mut(&id)
                    && job.state == State::Accepted
                {
                    job.state = State::Running;
                }
            }
            Err(_) => self.unknown(&id, "backend_unavailable"),
        }
        self.changed.notify_waiters();
    }
    pub async fn status(&self, args: StatusArgs) -> rmcp::model::CallToolResult {
        if let Err(e) = self.validate_scope(&args.session_id, &args.runtime_id) {
            return failure("scope_mismatch", e.to_string());
        }
        if args.job_id.is_some() == args.request_id.is_some() || args.wait_ms.unwrap_or(0) > 30000 {
            return failure(
                "invalid_arguments",
                "provide exactly one job_id or request_id; wait_ms <= 30000",
            );
        }
        let id = {
            let store = self.store.lock().unwrap();
            match &args.job_id {
                Some(id) => Some(id.clone()),
                None => args
                    .request_id
                    .as_ref()
                    .and_then(|r| store.receipts.get(r))
                    .map(|r| r.job_id.clone()),
            }
        };
        let Some(id) = id else {
            return failure("not_found", "unknown request_id");
        };
        let cursor = match args.cursor {
            Some(s) => match URL_SAFE_NO_PAD
                .decode(s)
                .ok()
                .and_then(|b| serde_json::from_slice::<Cursor>(&b).ok())
            {
                Some(c) if c.runtime == self.session.runtime_id && c.job == id => c,
                _ => {
                    return failure(
                        "invalid_cursor",
                        "cursor does not belong to this runtime/job",
                    );
                }
            },
            None => Cursor {
                runtime: self.session.runtime_id.clone(),
                job: id.clone(),
                stdout: 0,
                stderr: 0,
            },
        };
        let deadline =
            tokio::time::Instant::now() + Duration::from_millis(args.wait_ms.unwrap_or(0));
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let result = {
                let mut store = self.store.lock().unwrap();
                self.expire(&mut store);
                let Some(job) = store.jobs.get(&id) else {
                    return failure("not_found", "unknown job_id");
                };
                if cursor.stdout > job.stdout.end || cursor.stderr > job.stderr.end {
                    return failure("invalid_cursor", "cursor is ahead of source output");
                }
                let terminal = job.state == State::Finished || job.state == State::Unknown;
                let changed = job.stdout.end > cursor.stdout || job.stderr.end > cursor.stderr;
                if terminal || changed || tokio::time::Instant::now() >= deadline {
                    let (stdout, so) = job
                        .stdout
                        .page(cursor.stdout, self.session.settings.output_page_bytes / 2);
                    let (stderr, se) = job
                        .stderr
                        .page(cursor.stderr, self.session.settings.output_page_bytes / 2);
                    let next = URL_SAFE_NO_PAD.encode(
                        serde_json::to_vec(&Cursor {
                            runtime: cursor.runtime.clone(),
                            job: id.clone(),
                            stdout: so,
                            stderr: se,
                        })
                        .unwrap(),
                    );
                    let receipt = args.request_id.as_ref().and_then(|r| store.receipts.get(r));
                    Some(success(
                        json!({"session_id":self.session.id,"runtime_id":self.session.runtime_id,"job_id":id,
                        "request_id":args.request_id,"state":job.state,"exit_code":job.exit_code,"error":job.error.or(job.output_error),
                        "stdout":stdout,"stderr":stderr,"next_cursor":next,"input_delivery_unknown":job.input_unknown,
                        "input_closed":job.input_closed,"pty_multiplexed":job.input==InputMode::Pty,
                        "output_unavailable":job.output_error.is_some(),"output_error":job.output_error,
                        "stop_ack":job.stop_ack,"effective_access":job.effective_access,
                        "input_receipt":receipt.filter(|r|r.input).and_then(|r|r.input_result.clone())}),
                    ))
                } else {
                    None
                }
            };
            if let Some(result) = result {
                return result;
            }
            let _ = tokio::time::timeout_at(deadline, notified).await;
        }
    }
    pub async fn input(self: &Arc<Self>, args: InputArgs) -> rmcp::model::CallToolResult {
        let prepared = (|| -> Result<_> {
            self.validate_scope(&args.session_id, &args.runtime_id)?;
            Self::check_id(&args.request_id)?;
            ensure!(
                usize::from(args.data.is_some())
                    + usize::from(args.eof == Some(true))
                    + usize::from(args.resize.is_some())
                    == 1,
                "exactly one data/EOF/resize required"
            );
            ensure!(args.eof != Some(false), "EOF must be true or omitted");
            ensure!(
                args.data
                    .as_ref()
                    .is_none_or(|d| d.len() <= self.session.settings.max_input_bytes),
                "input budget exceeded"
            );
            let digest = Self::digest("input", &args);
            let mut store = self.store.lock().unwrap();
            if let Some(receipt) = store.receipts.get(&args.request_id) {
                ensure!(receipt.digest == digest, "request_id_mismatch");
                return Ok((None, receipt.input_result.clone()));
            }
            ensure!(
                !store.closing && self.backend.alive(),
                "backend_unavailable"
            );
            ensure!(
                store.receipts.len() < self.session.settings.max_receipts,
                "receipt_capacity"
            );
            let job = store
                .jobs
                .get_mut(&args.job_id)
                .ok_or_else(|| anyhow::anyhow!("unknown job_id"))?;
            ensure!(
                job.state == State::Running || job.state == State::StopRequested,
                "job not accepting input"
            );
            ensure!(
                job.input != InputMode::Closed
                    && !job.input_closed
                    && !job.input_unknown
                    && !job.input_busy,
                "input closed, pending or delivery unknown"
            );
            // Serialize input operations per job without holding a lock across RPC.
            if let Some(size) = &args.resize {
                ensure!(
                    job.input == InputMode::Pty && size.rows > 0 && size.cols > 0,
                    "resize requires PTY and positive size"
                );
            }
            let process = job.process_id.clone();
            let (method, params) = if let Some(size) = &args.resize {
                ensure!(
                    job.input == InputMode::Pty && size.rows > 0 && size.cols > 0,
                    "resize requires PTY and positive size"
                );
                (
                    "command/exec/resize",
                    json!({"processId":process,"size":size}),
                )
            } else {
                (
                    "command/exec/write",
                    json!({"processId":process,"deltaBase64":args.data.as_ref().map(|s|STANDARD.encode(s)),"closeStdin":args.eof.unwrap_or(false)}),
                )
            };
            job.input_busy = true;
            store.receipts.insert(
                args.request_id.clone(),
                Receipt {
                    digest,
                    job_id: args.job_id.clone(),
                    input_result: None,
                    input: true,
                },
            );
            Ok((Some((method, params)), None))
        })();
        let (send, previous) = match prepared {
            Ok(v) => v,
            Err(e) => return failure_detail("input_rejected", e.to_string()),
        };
        let Some((method, params)) = send else {
            return success(previous.unwrap_or(
                json!({"request_id":args.request_id,"job_id":args.job_id,"delivery":"pending"}),
            ));
        };
        // The caller owns an Arc task; cancellation of the MCP/IPC request cannot cancel delivery bookkeeping.
        let result = self.backend.control(method, params).await;
        let mut store = self.store.lock().unwrap();
        let (delivery, uncertain) = match &result {
            Ok(_) => ("accepted", false),
            Err(e) if e.uncertain => ("unknown", true),
            Err(_) => ("rejected", false),
        };
        if let Some(job) = store.jobs.get_mut(&args.job_id) {
            job.input_busy = false;
            job.input_unknown = uncertain;
            if result.is_ok() && args.eof == Some(true) {
                job.input_closed = true;
            }
        }
        let value = json!({"session_id":self.session.id,"runtime_id":self.session.runtime_id,"job_id":args.job_id,"request_id":args.request_id,"delivery":delivery,"error":result.err().map(|e|e.code)});
        if let Some(receipt) = store.receipts.get_mut(&args.request_id) {
            receipt.input_result = Some(value.clone());
        }
        self.changed.notify_waiters();
        success(value)
    }
    pub async fn stop(&self, args: StopArgs) -> rmcp::model::CallToolResult {
        let _dispatch = self.dispatch_lock.lock().await;
        if let Err(e) = self.validate_scope(&args.session_id, &args.runtime_id) {
            return failure("scope_mismatch", e.to_string());
        }
        let process = {
            let mut store = self.store.lock().unwrap();
            let Some(job) = store.jobs.get_mut(&args.job_id) else {
                return failure("not_found", "unknown job_id");
            };
            if job.state == State::Finished {
                return success(
                    json!({"job_id":args.job_id,"state":"finished","exit_code":job.exit_code}),
                );
            }
            if job.stop_attempted {
                return success(
                    json!({"job_id":args.job_id,"state":job.state,"stop_ack":job.stop_ack}),
                );
            }
            if !self.backend.alive() {
                return failure(
                    "stop_unavailable",
                    "original backend connection is unavailable",
                );
            }
            job.stop_attempted = true;
            if job.state != State::Unknown {
                job.state = State::StopRequested;
            }
            job.process_id.clone()
        };
        let result = self
            .backend
            .control("command/exec/terminate", json!({"processId":process}))
            .await;
        let value = json!({"delivery":match &result{Ok(_)=>"accepted",Err(e) if e.uncertain=>"unknown",Err(_)=>"rejected"},"error":result.err().map(|e|e.code)});
        if let Some(job) = self.store.lock().unwrap().jobs.get_mut(&args.job_id) {
            job.stop_ack = Some(value.clone());
        }
        self.changed.notify_waiters();
        success(
            json!({"session_id":self.session.id,"runtime_id":self.session.runtime_id,"job_id":args.job_id,"stop_ack":value}),
        )
    }
    pub async fn apply_diff(self: &Arc<Self>, args: DiffArgs) -> rmcp::model::CallToolResult {
        let prepared=async {
            self.validate_scope(&args.session_id,&args.runtime_id)?;Self::check_id(&args.request_id)?;
            ensure!(args.diff.len()<=self.session.settings.max_diff_bytes,"diff budget exceeded");
            // Existing request IDs must not depend on files still matching the original patch.
            let digest=Self::digest("diff",&args);
            {
                let store=self.store.lock().unwrap();
                if let Some(receipt)=store.receipts.get(&args.request_id){ensure!(receipt.digest==digest,"request_id_mismatch");return Ok((receipt.job_id.clone(),None));}
                ensure!(!store.jobs.values().any(|j|j.diff_apply.is_some()||j.effective_access["operation"]=="apply_diff"&&j.state!=State::Finished),"diff_busy");
            }
            crate::files::validate_diff(&self.session,&args.diff).await?;
            let exec=ExecArgs{session_id:args.session_id.clone(),runtime_id:args.runtime_id.clone(),request_id:args.request_id.clone(),
                argv:vec!["/bin/sh".into(),"-c".into(),"printf 'local-mcp Git check ready\\n' >&2; exec git -c core.hooksPath=/dev/null apply --recount --whitespace=nowarn --check -".into()],cwd:None,wait_ms:Some(30000),input_mode:Some(InputMode::Pipe),size:None};
            // The fixed script emits readiness; patch bytes are delivered once through stdin, never shell interpolation or a temporary file.
            let (check,mut access)=self.execution_params(&exec,None)?;
            let mut apply=check.clone();apply["command"][2]=json!("printf 'local-mcp Git apply ready\\n' >&2; exec git -c core.hooksPath=/dev/null apply --recount --whitespace=nowarn -");
            access["operation"]=json!("apply_diff");
            // Recheck serialization atomically with receipt reservation after asynchronous validation.
            let _guard=self.diff_lock.lock().await;
            {
                let store=self.store.lock().unwrap();
                if !store.receipts.contains_key(&args.request_id){ensure!(!store.jobs.values().any(|j|j.effective_access["operation"]=="apply_diff"&&j.state!=State::Finished),"diff_busy");}
            }
            let result=self.reserve(&args.request_id,digest,check,access,InputMode::Closed,Some(apply))?;
            if result.1.is_some(){
                let mut store=self.store.lock().unwrap();
                let job=store.jobs.get_mut(&result.0).unwrap();
                job.diff_input=Some(args.diff.clone());job.diff_patch=Some(args.diff.clone());
            }
            Ok::<_,anyhow::Error>(result)
        }.await;
        match prepared {
            Ok((id, send)) => {
                if let Some(params) = send {
                    self.dispatch(id.clone(), params).await;
                }
                self.wait_for_exit(&id, 30000).await;
                self.status(StatusArgs {
                    session_id: args.session_id,
                    runtime_id: args.runtime_id,
                    job_id: Some(id),
                    request_id: None,
                    cursor: None,
                    wait_ms: Some(0),
                })
                .await
            }
            Err(e) => failure_detail("diff_rejected", e.to_string()),
        }
    }
    pub async fn shutdown(self: &Arc<Self>) -> bool {
        let active = {
            let mut store = self.store.lock().unwrap();
            store.closing = true;
            store
                .jobs
                .iter()
                .filter(|(_, j)| j.state != State::Finished)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };
        let mut stops = tokio::task::JoinSet::new();
        for id in active {
            let jobs = self.clone();
            stops.spawn(async move {
                jobs.stop(StopArgs {
                    session_id: jobs.session.id.clone(),
                    runtime_id: jobs.session.runtime_id.clone(),
                    job_id: id,
                })
                .await
            });
        }
        let duration = Duration::from_millis(self.session.settings.shutdown_ms);
        let _ = tokio::time::timeout(duration, async {
            while stops.join_next().await.is_some() {}
        })
        .await;
        stops.abort_all();
        self.backend.shutdown(duration).await
    }
}
