use rmcp::model::CallToolResult;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

pub fn success(value: Value) -> CallToolResult {
    let mut v = value;
    v["schema_version"] = json!(1);
    v["ok"] = json!(true);
    CallToolResult::structured(v)
}
pub fn failure(code: &str, message: impl Into<String>) -> CallToolResult {
    CallToolResult::structured_error(
        json!({"schema_version":1,"ok":false,"error":{"code":code,"message":message.into()}}),
    )
}
macro_rules! args {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            pub session_id: String,
            pub runtime_id: String,
            $(pub $field: $ty),*
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InfoArgs {
    pub session_id: String,
}
args!(ReadArgs { path: String, start_line: Option<usize>, max_lines: Option<usize> });
args!(ImageArgs { path: String });
args!(DiffArgs {
    request_id: String,
    diff: String
});
args!(StatusArgs { job_id: Option<String>, request_id: Option<String>, cursor: Option<String>, wait_ms: Option<u64> });
args!(StopArgs { job_id: String });
args!(InputArgs { job_id: String, request_id: String, data: Option<String>, eof: Option<bool>, resize: Option<TerminalSize> });
args!(ExecArgs { request_id: String, argv: Vec<String>, cwd: Option<String>, wait_ms: Option<u64>, input_mode: Option<InputMode>, size: Option<TerminalSize> });
args!(ExpandedArgs { request_id: String, argv: Vec<String>, cwd: Option<String>, wait_ms: Option<u64>, input_mode: Option<InputMode>, size: Option<TerminalSize>, reason: String, access: Access });
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputMode {
    #[default]
    Closed,
    Pipe,
    Pty,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalSize {
    pub rows: u16,
    pub cols: u16,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Access {
    Limited { roots: Vec<String>, network: bool },
    Full {},
}
impl ExpandedArgs {
    pub fn exec(&self) -> ExecArgs {
        ExecArgs {
            session_id: self.session_id.clone(),
            runtime_id: self.runtime_id.clone(),
            request_id: self.request_id.clone(),
            argv: self.argv.clone(),
            cwd: self.cwd.clone(),
            wait_ms: self.wait_ms,
            input_mode: self.input_mode,
            size: self.size.clone(),
        }
    }
}

/// Bounds JSONL allocation before deserialization, including a peer that never sends a newline.
pub async fn read_frame<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    cap: usize,
) -> std::io::Result<Option<Vec<u8>>> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};
    let mut bytes = Vec::new();
    let n = reader
        .take((cap + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .await?;
    if n == 0 {
        return Ok(None);
    }
    if n > cap {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame exceeds configured limit",
        ));
    }
    Ok(Some(bytes))
}

/// Stable categories are kept separate from explanatory text.
pub fn failure_detail(fallback: &str, message: String) -> CallToolResult {
    let code = match message.as_str() {
        "session_mismatch" => "session_mismatch",
        "runtime_mismatch" => "runtime_mismatch",
        "request_id_mismatch" => "request_id_mismatch",
        "job_capacity" => "job_capacity",
        "receipt_capacity" => "receipt_capacity",
        "backend_unavailable" => "backend_unavailable",
        "diff_busy" => "diff_busy",
        "full access disabled"
        | "network expansion disabled"
        | "extra root exceeds fixed ceiling"
        | "cwd outside effective write roots"
        | "write root changed after startup" => "authorization_denied",
        _ => fallback,
    };
    failure(code, message)
}
