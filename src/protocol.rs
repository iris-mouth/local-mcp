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
    let recovery = match code {
        "dispatch_unknown" | "result_unknown" => {
            "Inspect the original request_id with command_status; do not repeat the change with a new ID."
        }
        "scope_mismatch" | "runtime_mismatch" | "session_mismatch" => {
            "Call session_info for the selected session and use its runtime_id."
        }
        "invalid_arguments" => "Correct the named field using the tool input schema.",
        "invalid_cursor" => {
            "Use a cursor returned by this tool for the same runtime and target, or omit it to read retained data again."
        }
        "authorization_denied" => {
            "Use access within the trusted startup ceilings; tool arguments cannot raise those ceilings."
        }
        "request_id_mismatch" => {
            "Inspect the original request_id; reuse it only with the original operation content."
        }
        "feature_unavailable" => {
            "Select ripgrep outside all session write/expansion roots through the trusted startup PATH or search_executable, then start a new session."
        }
        _ => {
            "Inspect the reported state before retrying; an unavailable response does not prove that a change was not executed."
        }
    };
    CallToolResult::structured_error(
        json!({"schema_version":1,"ok":false,"error":{"code":code,"message":message.into(),"recovery":recovery}}),
    )
}

pub fn invalid_argument(field: &str, expected: &str) -> CallToolResult {
    CallToolResult::structured_error(json!({"schema_version":1,"ok":false,"error":{
        "code":"invalid_arguments","message":"This call was rejected before dispatch.",
        "field":field.chars().take(128).collect::<String>(),"expected":expected,
        "dispatch":"not_attempted","recovery":"Correct this field. If this is a retry of existing work, inspect its original request_id before another change."
    }}))
}

pub fn parse_arguments<T: serde::de::DeserializeOwned>(
    args: Value,
) -> Result<T, Box<CallToolResult>> {
    serde_path_to_error::deserialize(args).map_err(
        |error: serde_path_to_error::Error<serde_json::Error>| {
            // Serde errors can quote argument values. Return only the field path and
            // the static expected type, never the original invalid value.
            let detail = error.inner().to_string();
            let path = error.path().to_string();
            let named =
                detail.starts_with("missing field ") || detail.starts_with("unknown field ");
            let field = if named {
                detail.split('`').nth(1).unwrap_or("arguments")
            } else if path == "." {
                "arguments"
            } else {
                &path
            };
            let expected = if detail.starts_with("missing field ") {
                "required field"
            } else if detail.starts_with("unknown field ") {
                "a field declared in this tool's input schema"
            } else {
                detail
                    .rsplit_once("expected ")
                    .map(|(_, expected)| expected)
                    .unwrap_or("a value matching the tool's input schema")
            };
            Box::new(invalid_argument(
                field,
                &expected.chars().take(256).collect::<String>(),
            ))
        },
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
args!(ReadArgs { path: String, start_line: Option<usize>, max_lines: Option<usize>, max_bytes: Option<usize> });
args!(ImageArgs { path: String });
args!(DiffArgs {
    request_id: String,
    diff: String
});
args!(StatusArgs { job_id: Option<String>, request_id: Option<String>, cursor: Option<String>, wait_ms: Option<u64>, max_output_bytes: Option<usize> });
args!(ListJobsArgs { limit: Option<usize>, cursor: Option<String> });
args!(ListFilesArgs { path: Option<String>, glob: Option<String>, hidden: Option<bool>, respect_ignore: Option<bool>, limit: Option<usize> });
args!(SearchArgs { path: Option<String>, glob: Option<String>, hidden: Option<bool>, respect_ignore: Option<bool>, limit: Option<usize>, pattern: String, regex: Option<bool> });
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
    let field = match message.as_str() {
        "request_id must be 1..128 bytes" => Some(("request_id", "1..=128 bytes")),
        "argv must be nonempty and bounded" => Some(("argv", "1..=4096 string arguments")),
        "argv contains NUL" => Some(("argv", "arguments without NUL bytes")),
        "argv exceeds input budget" => Some((
            "argv",
            "arguments within the trusted max_diff_bytes input budget",
        )),
        "wait_ms must be 0..30000" => Some(("wait_ms", "0..=30000 milliseconds of observation")),
        "size requires PTY" | "invalid terminal size" => {
            Some(("size", "positive PTY dimensions with input_mode=pty"))
        }
        "exactly one data/EOF/resize required" => {
            Some(("data/eof/resize", "exactly one input action"))
        }
        "EOF must be true or omitted" => Some(("eof", "true or omitted")),
        "input budget exceeded" => Some((
            "data",
            "UTF-8 bytes within the trusted max_input_bytes budget",
        )),
        "resize requires PTY and positive size" => {
            Some(("resize", "positive dimensions for an existing PTY job"))
        }
        "expanded execution requires reason" => {
            Some(("reason", "1..=4096 bytes explaining this expansion"))
        }
        _ => None,
    };
    if let Some((field, expected)) = field {
        return invalid_argument(field, expected);
    }
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
