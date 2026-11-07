//! Public response contracts shared by the MCP catalogue and contract tests.
use serde_json::{Value, json};

fn string() -> Value {
    json!({"type":"string"})
}
fn count() -> Value {
    json!({"type":"integer","minimum":0})
}
fn boolean() -> Value {
    json!({"type":"boolean"})
}
fn nullable(value: Value) -> Value {
    json!({"anyOf":[value,{"type":"null"}]})
}
fn array(item: Value) -> Value {
    json!({"type":"array","items":item})
}
fn object(fields: Vec<(&str, Value)>, required: &[&str]) -> Value {
    let properties: serde_json::Map<String, Value> =
        fields.into_iter().map(|(k, v)| (k.into(), v)).collect();
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn state() -> Value {
    json!({"enum":["accepted","running","stop_requested","finished","unknown"]})
}
fn error() -> Value {
    object(
        vec![
            ("code", string()),
            ("message", string()),
            ("recovery", string()),
            ("field", string()),
            ("expected", string()),
            ("dispatch", json!({"enum":["not_attempted","unknown"]})),
        ],
        &["code", "message", "recovery"],
    )
}
fn ack() -> Value {
    object(
        vec![
            (
                "delivery",
                json!({"enum":["pending","accepted","rejected","unknown"]}),
            ),
            ("error", nullable(string())),
        ],
        &["delivery"],
    )
}
fn input_receipt() -> Value {
    object(
        vec![
            ("session_id", string()),
            ("runtime_id", string()),
            ("job_id", string()),
            ("request_id", string()),
            (
                "delivery",
                json!({"enum":["pending","accepted","rejected","unknown"]}),
            ),
            ("error", nullable(string())),
        ],
        &["job_id", "request_id", "delivery"],
    )
}
fn page() -> Value {
    object(
        vec![
            ("text", string()),
            ("lossy", boolean()),
            ("start_byte", count()),
            ("end_byte", count()),
            ("source_end_byte", count()),
            ("retained_start_byte", count()),
            ("gap", boolean()),
            ("has_more", boolean()),
            ("source_truncated", boolean()),
            ("expired", boolean()),
            ("pending_utf8", boolean()),
            ("budget_limited", boolean()),
        ],
        &[
            "text",
            "lossy",
            "start_byte",
            "end_byte",
            "source_end_byte",
            "retained_start_byte",
            "gap",
            "has_more",
            "source_truncated",
            "expired",
            "pending_utf8",
            "budget_limited",
        ],
    )
}
fn access() -> Value {
    object(
        vec![
            ("mode", json!({"enum":["limited","full"]})),
            ("roots", nullable(array(string()))),
            ("network", boolean()),
            ("cwd", string()),
            ("operation", json!({"const":"apply_diff"})),
        ],
        &["mode", "roots", "network", "cwd"],
    )
}
fn edits() -> Value {
    array(object(
        vec![
            ("path", string()),
            ("operation", json!({"enum":["create","update","delete"]})),
            ("planned_added_lines", count()),
            ("planned_removed_lines", count()),
            (
                "status",
                json!({"enum":["planned","applied","not_applied","unknown"]}),
            ),
        ],
        &[
            "path",
            "operation",
            "planned_added_lines",
            "planned_removed_lines",
            "status",
        ],
    ))
}
fn settings() -> Value {
    let mut fields = Vec::new();
    for key in [
        "max_jobs",
        "max_receipts",
        "output_bytes_per_job",
        "output_bytes_total",
        "output_page_bytes",
        "output_ttl_secs",
        "max_message_bytes",
        "max_input_bytes",
        "max_diff_bytes",
        "max_image_pixels",
        "max_rpc_inflight",
        "rpc_timeout_ms",
        "shutdown_ms",
        "edit_metadata_bytes_total",
    ] {
        fields.push((key, count()));
    }
    for key in [
        "allow_network",
        "allow_full_access",
        "allow_git_metadata",
        "allow_ssh_agent",
    ] {
        fields.push((key, boolean()));
    }
    for key in ["write_roots", "extra_roots"] {
        fields.push((key, array(string())));
    }
    fields.push(("search_executable", nullable(string())));
    let required: Vec<_> = fields.iter().map(|(key, _)| *key).collect();
    object(fields.clone(), &required)
}
fn session() -> Value {
    object(
        vec![
            ("id", string()),
            ("runtime_id", string()),
            ("cwd", string()),
            ("roots", array(string())),
            ("git_metadata_candidates", array(string())),
            ("extra_roots", array(string())),
            ("temporary_directory", string()),
            ("diagnostics_directory", string()),
            ("settings", settings()),
        ],
        &[
            "id",
            "runtime_id",
            "cwd",
            "roots",
            "git_metadata_candidates",
            "extra_roots",
            "temporary_directory",
            "diagnostics_directory",
            "settings",
        ],
    )
}

pub fn output(name: &str) -> Value {
    let mut fields = vec![
        ("schema_version", json!({"const":1})),
        ("ok", boolean()),
        ("session_id", string()),
        ("runtime_id", string()),
        ("job_id", string()),
        ("request_id", nullable(string())),
        ("error", json!({"anyOf":[error(),string(),{"type":"null"}]})),
    ];
    let required: &[&str] = match name {
        "session_info" => {
            fields.extend([
                ("session", session()),
                ("backend_available", boolean()),
                ("closing", boolean()),
                ("active_jobs", count()),
                ("receipt_count", count()),
                ("read_scope", json!({"const":"host_user"})),
                ("input_ack", json!({"const":"backend_acceptance_only"})),
            ]);
            &[
                "session_id",
                "runtime_id",
                "session",
                "backend_available",
                "closing",
                "active_jobs",
                "receipt_count",
                "read_scope",
                "input_ack",
            ]
        }
        "read_file" => {
            fields.extend([
                ("path", string()),
                ("text", string()),
                ("start_line", count()),
                ("lines_returned", count()),
                ("next_line", nullable(count())),
                ("has_more", boolean()),
            ]);
            &[
                "path",
                "text",
                "start_line",
                "lines_returned",
                "next_line",
                "has_more",
            ]
        }
        "exec_command" | "exec_command_expanded" | "command_status" | "apply_diff" => {
            fields.extend([
                ("state", state()),
                ("exit_code", nullable(json!({"type":"integer"}))),
                ("stdout", page()),
                ("stderr", page()),
                ("next_cursor", string()),
                ("output_budget_bytes", count()),
                ("input_delivery_unknown", boolean()),
                ("input_closed", boolean()),
                ("pty_multiplexed", boolean()),
                ("output_unavailable", boolean()),
                ("output_error", nullable(string())),
                ("stop_ack", nullable(ack())),
                ("effective_access", access()),
                ("input_receipt", nullable(input_receipt())),
                ("edits", nullable(edits())),
            ]);
            &[
                "job_id",
                "state",
                "exit_code",
                "stdout",
                "stderr",
                "next_cursor",
                "output_budget_bytes",
                "input_delivery_unknown",
                "input_closed",
                "pty_multiplexed",
                "output_unavailable",
                "output_error",
                "stop_ack",
                "effective_access",
                "input_receipt",
                "edits",
            ]
        }
        "write_stdin" => {
            fields.extend([(
                "delivery",
                json!({"enum":["pending","accepted","rejected","unknown"]}),
            )]);
            &["job_id", "request_id", "delivery"]
        }
        "stop_command" => {
            fields.extend([
                ("state", state()),
                ("exit_code", nullable(json!({"type":"integer"}))),
                ("stop_ack", nullable(ack())),
            ]);
            &["job_id"]
        }
        "list_jobs" => {
            fields.extend([
                (
                    "jobs",
                    array(object(
                        vec![
                            ("job_id", string()),
                            ("state", state()),
                            ("exit_code", nullable(json!({"type":"integer"}))),
                        ],
                        &["job_id", "state", "exit_code"],
                    )),
                ),
                ("has_more", boolean()),
                ("next_cursor", nullable(string())),
                ("snapshot", json!({"const":false})),
            ]);
            &["jobs", "has_more", "next_cursor", "snapshot"]
        }
        "list_files" | "search_files" => {
            fields.extend([("path",string()),("truncated",boolean()),("truncation_reason",nullable(json!({"enum":[
                "result_limit","output_limit","frame_limit","time_limit","session_closing","invalid_backend_output","read_errors"]}))),
                ("skipped_non_utf8",count()),("partial",boolean())]);
            if name == "list_files" {
                fields.push(("files", array(string())));
                &[
                    "path",
                    "files",
                    "truncated",
                    "truncation_reason",
                    "skipped_non_utf8",
                    "partial",
                ]
            } else {
                fields.push((
                    "matches",
                    array(object(
                        vec![
                            ("path", string()),
                            ("line", json!({"type":"integer","minimum":1})),
                            ("text", string()),
                        ],
                        &["path", "line", "text"],
                    )),
                ));
                &[
                    "path",
                    "matches",
                    "truncated",
                    "truncation_reason",
                    "skipped_non_utf8",
                    "partial",
                ]
            }
        }
        _ => &[],
    };
    let mut schema = object(fields, &["schema_version", "ok"]);
    schema["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
    schema["allOf"] = json!([{
        "if":{"properties":{"ok":{"const":true}}},
        "then":{"required":required},
        "else":{"required":["error"],"properties":{"error":error()}}
    }]);
    schema
}
