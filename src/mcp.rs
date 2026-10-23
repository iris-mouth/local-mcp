use crate::{protocol::ToolCall, session};
use anyhow::Result;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, model::*, service::RequestContext};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct Mcp {
    requests: std::sync::Arc<tokio::sync::Semaphore>,
}
impl Default for Mcp {
    fn default() -> Self {
        Self {
            requests: std::sync::Arc::new(tokio::sync::Semaphore::new(64)),
        }
    }
}
impl ServerHandler for Mcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(Implementation::new("local-mcp",env!("CARGO_PKG_VERSION")))
            .with_instructions("Use the user-supplied session_id. Call session_info first and include its runtime_id in subsequent calls. Reuse the same request_id only for the same operation; inspect uncertain results before any new change. Command stderr/nonzero exit are command results. Stop acceptance is not exit confirmation. Search/list with argv tools.")
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
        let Ok(_permit) = self.requests.clone().try_acquire_owned() else {
            return Ok(
                crate::protocol::failure("rpc_capacity", "MCP request capacity reached").into(),
            );
        };
        let call = ToolCall {
            name: request.name.into_owned(),
            arguments: Value::Object(request.arguments.unwrap_or_default()),
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
fn integer() -> Value {
    json!({"type":"integer","minimum":0})
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
            ],
            vec!["path"],
            true,
        ),
        (
            "view_image",
            "Return native MCP image content for PNG/JPEG/GIF/WebP/BMP/TIFF/AVIF. Actual visual use depends on host support.",
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
        let value = json!({"name":name,"description":description,"inputSchema":schema,
            "outputSchema":{"type":"object","properties":{"schema_version":integer(),"ok":{"type":"boolean"}},"required":["schema_version","ok"]},
            "annotations":{"readOnlyHint":read_only,"destructiveHint":!read_only,"idempotentHint":read_only||name=="stop_command","openWorldHint":name=="exec_command_expanded"}});
        result.push(serde_json::from_value(value).expect("static tool schema"));
    }
    result
}
