use crate::{protocol::ToolCall, rpc_capacity::RpcCapacity, session};
use anyhow::Result;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, model::*, service::RequestContext};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct Mcp {
    requests: RpcCapacity,
}
impl Default for Mcp {
    fn default() -> Self {
        Self {
            requests: RpcCapacity::new(64),
        }
    }
}
impl ServerHandler for Mcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(Implementation::new("local-mcp",env!("CARGO_PKG_VERSION")))
            .with_instructions("Use the user-supplied session_id. Call session_info first and include its runtime_id in subsequent calls. Reuse a request_id only for the original operation; inspect uncertain results before another change. Use list_jobs to recover job IDs, list_files for file names and search_files for text. Read/search limits affect delivery, not execution or retention. Nonzero exit is a command result. Stop acceptance is not exit confirmation.")
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: tools(),
            ..Default::default()
        })
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        tools().into_iter().find(|t| t.name == name)
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        let call = ToolCall {
            name: request.name.into_owned(),
            arguments: Value::Object(request.arguments.unwrap_or_default()),
        };
        let Some(_permit) = self.requests.acquire(&call) else {
            return Ok(
                crate::protocol::failure("rpc_capacity", "MCP request capacity reached").into(),
            );
        };
        // The owner detaches delivery bookkeeping from the disposable MCP facade.
        Ok(session::forward(call).await.into())
    }
}
pub async fn serve() -> Result<()> {
    let (read, write) = rmcp::transport::stdio();
    let transport = rmcp::transport::async_rw::AsyncRwTransport::new_server(read, write)
        .with_max_line_length(64 * 1024 * 1024);
    Mcp::default().serve(transport).await?.waiting().await?;
    Ok(())
}
fn string() -> Value {
    json!({"type":"string"})
}
fn size() -> Value {
    json!({"type":"object","properties":{"rows":{"type":"integer","minimum":1,"maximum":65535},"cols":{"type":"integer","minimum":1,"maximum":65535}},"required":["rows","cols"],"additionalProperties":false})
}
pub fn tools() -> Vec<Tool> {
    let mut result = vec![];
    let exec_fields = vec![
        ("request_id", string()),
        (
            "argv",
            json!({"type":"array","items":{"type":"string"},"minItems":1}),
        ),
        ("cwd", string()),
        (
            "wait_ms",
            json!({"type":"integer","minimum":0,"maximum":30000,"default":30000}),
        ),
        (
            "input_mode",
            json!({"enum":["closed","pipe","pty"],"default":"closed"}),
        ),
        ("size", size()),
    ];
    let access = json!({"oneOf":[{"type":"object","properties":{"mode":{"const":"full"}},"required":["mode"],"additionalProperties":false},{"type":"object","properties":{"mode":{"const":"limited"},"roots":{"type":"array","items":{"type":"string"}},"network":{"type":"boolean"}},"required":["mode","roots","network"],"additionalProperties":false}]});
    for (name, description, fields, required, read_only) in [
        (
            "session_info",
            "Inspect runtime, roots, fixed ceilings, effective resource limits and diagnostics location.",
            vec![],
            vec![],
            true,
        ),
        (
            "read_file",
            "Read host-user UTF-8 text, preserving CRLF. Optional one-based line range; explicit continuation when delivery budget is reached.",
            vec![
                ("path", string()),
                ("start_line", json!({"type":"integer","minimum":1})),
                ("max_lines", json!({"type":"integer","minimum":1})),
                ("max_bytes", json!({"type":"integer","minimum":1})),
            ],
            vec!["path"],
            true,
        ),
        (
            "view_image",
            "Return native MCP image content and a JSON text receipt for PNG/JPEG/GIF/WebP/BMP/TIFF/AVIF. When the host exposes content_items, emit its image item with the image output helper; text metadata alone is not visual recognition.",
            vec![("path", string())],
            vec!["path"],
            true,
        ),
        (
            "apply_diff",
            "Check and apply one standard unified text diff with Git, without index changes or automatic reapplication.",
            vec![("request_id", string()), ("diff", string())],
            vec!["request_id", "diff"],
            false,
        ),
        (
            "exec_command",
            "Execute argv with basic write roots and no network. Default 30s observation; 0 returns job immediately. No automatic execution deadline.",
            exec_fields.clone(),
            vec!["request_id", "argv"],
            false,
        ),
        (
            "exec_command_expanded",
            "Execute one argv job with explicit extra roots/network or full access within trusted Mac ceilings. Requires a reason.",
            exec_fields
                .into_iter()
                .chain([("reason", string()), ("access", access)])
                .collect(),
            vec!["request_id", "argv", "reason", "access"],
            false,
        ),
        (
            "command_status",
            "Observe exactly one job_id or request_id, exit and retained output using a non-consuming cursor. Does not run another command.",
            vec![
                ("job_id", string()),
                ("request_id", string()),
                ("cursor", string()),
                ("max_output_bytes", json!({"type":"integer","minimum":8})),
                (
                    "wait_ms",
                    json!({"type":"integer","minimum":0,"maximum":30000}),
                ),
            ],
            vec![],
            true,
        ),
        (
            "write_stdin",
            "Deliver exactly one UTF-8 data, EOF=true or PTY resize. ACK means backend acceptance. Unknown delivery blocks further input.",
            vec![
                ("job_id", string()),
                ("request_id", string()),
                ("data", string()),
                ("eof", json!({"const":true})),
                ("resize", size()),
            ],
            vec!["job_id", "request_id"],
            false,
        ),
        (
            "stop_command",
            "Request termination on the original connection. Repeated calls refer to one stop attempt. Acceptance does not prove exit.",
            vec![("job_id", string())],
            vec!["job_id"],
            false,
        ),
    ] {
        let mut properties = serde_json::Map::new();
        properties.insert("session_id".into(), string());
        let mut required: Vec<_> = required.into_iter().map(str::to_owned).collect();
        required.push("session_id".into());
        if name != "session_info" {
            properties.insert("runtime_id".into(), string());
            required.push("runtime_id".into());
        }
        for (key, value) in fields {
            properties.insert(key.into(), value);
        }
        let mut schema = json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
        if name == "command_status" {
            schema["oneOf"] = json!([{"required":["job_id"],"not":{"required":["request_id"]}},{"required":["request_id"],"not":{"required":["job_id"]}}]);
        }
        if name == "write_stdin" {
            schema["oneOf"] = json!([{"required":["data"],"not":{"anyOf":[{"required":["eof"]},{"required":["resize"]}]}},{"required":["eof"],"not":{"anyOf":[{"required":["data"]},{"required":["resize"]}]}},{"required":["resize"],"not":{"anyOf":[{"required":["eof"]},{"required":["data"]}]}}]);
        }
        describe_arguments(name, &mut schema);
        let mut value = json!({"name":name,"description":description,"inputSchema":schema,
            "outputSchema":crate::schemas::output(name),
            "annotations":{"readOnlyHint":read_only,"destructiveHint":!read_only,"idempotentHint":read_only||name=="stop_command","openWorldHint":name=="exec_command_expanded"}});
        if name == "view_image" {
            value.as_object_mut().unwrap().remove("outputSchema");
        }
        result.push(serde_json::from_value(value).expect("static tool schema"));
    }
    for (name, description, fields) in [
        (
            "list_jobs",
            "List bounded job IDs and states in this runtime, including after MCP reconnection. This is a live list, not a snapshot.",
            vec![
                (
                    "limit",
                    json!({"type":"integer","minimum":1,"maximum":200,"default":50}),
                ),
                ("cursor", string()),
            ],
        ),
        (
            "list_files",
            "Find file names by optional glob. Uses ripgrep; respects ignore files and excludes hidden files by default. Does not follow symlinks recursively. Results may be explicitly truncated.",
            search_fields(),
        ),
        (
            "search_files",
            "Search UTF-8 text with a literal pattern by default; regex requires regex=true. Returns file paths, one-based lines and original line text. Uses bounded ripgrep with ignore files and no recursive symlink following.",
            {
                let mut fields = search_fields();
                fields.extend([
                    (
                        "pattern",
                        json!({"type":"string","minLength":1,"maxLength":4096}),
                    ),
                    ("regex", json!({"type":"boolean","default":false})),
                ]);
                fields
            },
        ),
    ] {
        let mut properties = serde_json::Map::new();
        properties.insert("session_id".into(), string());
        properties.insert("runtime_id".into(), string());
        for (key, value) in fields {
            properties.insert(key.into(), value);
        }
        let mut required = vec!["session_id", "runtime_id"];
        if name == "search_files" {
            required.push("pattern");
        }
        let mut schema = json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
        describe_arguments(name, &mut schema);
        result.push(serde_json::from_value(json!({"name":name,"description":description,"inputSchema":schema,
            "outputSchema":crate::schemas::output(name),"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}})).unwrap());
    }
    result
}

fn search_fields() -> Vec<(&'static str, Value)> {
    vec![
        ("path", string()),
        ("glob", string()),
        ("hidden", json!({"type":"boolean","default":false})),
        ("respect_ignore", json!({"type":"boolean","default":true})),
        (
            "limit",
            json!({"type":"integer","minimum":1,"maximum":500,"default":50}),
        ),
    ]
}

fn describe_arguments(tool: &str, schema: &mut Value) {
    for (name, value) in schema["properties"].as_object_mut().unwrap() {
        value["description"] = json!(match name.as_str() {
            "session_id" =>
                "Session selected by the user or trusted launcher. Required on every call.",
            "runtime_id" =>
                "Exact runtime_id returned by session_info. Old runtime IDs are rejected.",
            "request_id" if tool == "command_status" =>
                "Observe an original request ID instead of a job ID; provide exactly one. No command is executed.",
            "request_id" =>
                "Client operation ID, 1..128 bytes. Reuse only for identical operation content after uncertain delivery. Example: edit-config-01.",
            "argv" =>
                "Nonempty argument array, without implicit shell expansion. Example: [\"git\",\"status\",\"--short\"]. For shell syntax use [\"/bin/sh\",\"-c\",script].",
            "cwd" =>
                "Working directory, relative to the selected workspace or absolute. Defaults to the workspace; must be inside effective write roots unless full access is explicitly authorized.",
            "path" =>
                "File/directory path, absolute or relative to the selected workspace. Search defaults to the workspace. Reads retain host-user access.",
            "wait_ms" =>
                "Observation wait in milliseconds, 0..30000. This is not an execution deadline. Zero returns immediately.",
            "input_mode" =>
                "closed (default), pipe, or pty. Select pipe/pty at command start to allow write_stdin.",
            "size" | "resize" =>
                "PTY dimensions in rows and columns, each 1..65535. Only valid for PTY input.",
            "start_line" => "First line to read, one-based; default 1.",
            "max_lines" =>
                "Maximum number of complete lines to return; default 200. Continue with next_line when has_more is true.",
            "max_bytes" =>
                "Maximum source text bytes to deliver; default 65536 clamped to the trusted delivery ceiling. A larger source file is allowed.",
            "max_output_bytes" =>
                "Maximum source bytes shared by stdout/stderr; default 16384 clamped to output_page_bytes. Explicit values require at least 8 bytes. Does not increase retained output.",
            "job_id" =>
                "Original job ID returned by execution or list_jobs. It belongs to this runtime.",
            "cursor" if tool == "list_jobs" =>
                "Opaque next_cursor from list_jobs. Reuse the same limit and runtime; omit to refresh the list.",
            "cursor" =>
                "Opaque next_cursor from command_status for this runtime/job. Reads are non-consuming; omit to read from retained output start.",
            "diff" =>
                "One standard Git unified text diff, not apply_patch format. Example headers: --- a/file.txt, +++ b/file.txt. Git checks before applying once.",
            "reason" =>
                "Why this one execution needs expanded access; nonempty, at most 4096 bytes.",
            "access" =>
                "One-job limited roots/network or full access, within trusted startup ceilings. Cannot change those ceilings.",
            "data" =>
                "UTF-8 input to the original job. Exactly one of data, eof, resize. ACK proves backend acceptance, not application consumption.",
            "eof" => "Use true to close stdin. Exactly one of data, eof, resize. False is invalid.",
            "pattern" =>
                "Nonempty single-line UTF-8 pattern, at most 4096 bytes. Literal unless regex=true. Example: TODO.",
            "regex" =>
                "Interpret pattern as a ripgrep regex only when true. Defaults to literal matching.",
            "glob" => "Optional ripgrep path glob, at most 4096 bytes. Example: **/*.rs.",
            "hidden" =>
                "Include hidden files when true; default false. Ignore rules still apply unless disabled separately.",
            "respect_ignore" =>
                "Respect .gitignore/.ignore rules when true (default). Set false explicitly to include ignored files.",
            "limit" =>
                "Maximum result count. Default 50; check has_more or truncated before treating results as complete.",
            _ => "See the tool description for this field.",
        });
        match name.as_str() {
            "start_line" => value["default"] = json!(1),
            "max_lines" => value["default"] = json!(200),
            "wait_ms" if tool == "command_status" => value["default"] = json!(0),
            _ => {}
        }
    }
}
