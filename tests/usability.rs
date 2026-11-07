#![cfg(unix)]
use local_mcp::{
    app_server::AppServer,
    config::{Session, Settings},
    jobs::Jobs,
    protocol::*,
    session,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

fn rg() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|p| p.join("rg"))
        .find(|p| p.is_file())
        .expect("ripgrep is required for search integration tests")
}
async fn setup(mut settings: Settings) -> (tempfile::TempDir, Arc<Jobs>) {
    settings.rpc_timeout_ms = 3000;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let session = Session::new("usability".into(), root, dir.path(), settings).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/app_server.py");
    let (backend, events) = AppServer::spawn(&fixture, &session).await.unwrap();
    (dir, Jobs::new(session, backend, events))
}
async fn call(jobs: &Arc<Jobs>, name: &str, mut args: Value) -> Value {
    args["session_id"] = json!(jobs.session.id);
    if name != "session_info" {
        args["runtime_id"] = json!(jobs.session.runtime_id);
    }
    session::dispatch(
        jobs.clone(),
        ToolCall {
            name: name.into(),
            arguments: args,
        },
    )
    .await
    .structured_content
    .unwrap()
}
async fn done(jobs: &Arc<Jobs>, id: &str) -> Value {
    for _ in 0..200 {
        let value = call(jobs, "command_status", json!({"job_id":id,"wait_ms":20})).await;
        if value["state"] == "finished" || value["state"] == "unknown" {
            return value;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("job did not finish")
}
fn write(jobs: &Jobs, name: &str, data: impl AsRef<[u8]>) {
    std::fs::write(jobs.session.cwd.join(name), data).unwrap();
}

#[tokio::test]
async fn diagnostics_locate_fields_without_echoing_values() {
    let (_dir, j) = setup(Settings::default()).await;
    for (args, field) in [
        (
            json!({"request_id":"bad","argv":"private-sentinel"}),
            "argv",
        ),
        (json!({"request_id":"bad"}), "argv"),
        (
            json!({"request_id":"bad","argv":"expected private-sentinel"}),
            "argv",
        ),
        (
            json!({"request_id":"bad","argv":["true"],"unexpected":"private-sentinel"}),
            "unexpected",
        ),
        (
            json!({"request_id":"bad","argv":["true"],"wait_ms":-1}),
            "wait_ms",
        ),
    ] {
        let value = call(&j, "exec_command", args).await;
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["field"], field);
        assert_eq!(value["error"]["dispatch"], "not_attempted");
        assert!(!value.to_string().contains("private-sentinel"));
    }
    assert_eq!(
        call(&j, "session_info", json!({})).await["receipt_count"],
        0
    );
    assert!(j.shutdown().await);
}

#[tokio::test]
async fn reads_are_bounded_and_continue_without_losing_crlf() {
    let (_dir, j) = setup(Settings::default()).await;
    let source = "あ\r\n".repeat(250);
    write(&j, "text.txt", &source);
    let first = call(&j, "read_file", json!({"path":"text.txt"})).await;
    assert_eq!(first["lines_returned"], 200);
    assert_eq!(first["next_line"], 201);
    let next = call(&j, "read_file", json!({"path":"text.txt","start_line":201})).await;
    assert_eq!(
        first["text"].as_str().unwrap().to_owned() + next["text"].as_str().unwrap(),
        source
    );
    let short = call(&j, "read_file", json!({"path":"text.txt","max_bytes":10})).await;
    assert_eq!(short["lines_returned"], 2);
    assert_eq!(short["next_line"], 3);
    assert_eq!(
        call(
            &j,
            "read_file",
            json!({"path":"text.txt","max_bytes":usize::MAX})
        )
        .await["error"]["field"],
        "max_bytes"
    );
    assert_eq!(
        call(&j, "read_file", json!({"path":"text.txt","start_line":0})).await["error"]["field"],
        "start_line"
    );
    for argument in ["max_lines", "max_bytes"] {
        let mut args = json!({"path":"text.txt"});
        args[argument] = json!(0);
        assert_eq!(
            call(&j, "read_file", args).await["error"]["field"],
            argument
        );
    }
    write(&j, "empty.txt", "");
    let empty = call(&j, "read_file", json!({"path":"empty.txt"})).await;
    assert_eq!(empty["text"], "");
    assert_eq!(empty["has_more"], false);
    assert_eq!(empty["next_line"], Value::Null);
    let past_end = call(&j, "read_file", json!({"path":"text.txt","start_line":300})).await;
    assert_eq!(past_end["lines_returned"], 0);
    assert_eq!(past_end["has_more"], false);
    let larger = call(
        &j,
        "read_file",
        json!({"path":"text.txt","max_lines":1000,"max_bytes":200000}),
    )
    .await;
    assert_eq!(larger["text"], source);
    assert_eq!(larger["has_more"], false);
    write(&j, "long.txt", "x".repeat(65537));
    let long = call(&j, "read_file", json!({"path":"long.txt"})).await;
    assert_eq!(long["ok"], false);
    assert!(
        long["error"]["message"]
            .as_str()
            .unwrap()
            .contains("single line")
    );
    assert!(j.shutdown().await);
}

#[tokio::test]
async fn unicode_output_obeys_each_calls_budget_and_is_non_consuming() {
    let (_dir, j) = setup(Settings::default()).await;
    let start = call(
        &j,
        "exec_command",
        json!({"request_id":"unicode","argv":["unicode"],"wait_ms":0}),
    )
    .await;
    let id = start["job_id"].as_str().unwrap();
    done(&j, id).await;
    let mut cursor = Value::Null;
    let mut stdout = String::new();
    let mut stderr = String::new();
    loop {
        let mut args = json!({"job_id":id,"max_output_bytes":8});
        if !cursor.is_null() {
            args["cursor"] = cursor.clone();
        }
        let page = call(&j, "command_status", args.clone()).await;
        assert_eq!(page, call(&j, "command_status", args).await);
        assert_eq!(page["output_budget_bytes"], 8);
        for (key, text) in [("stdout", &mut stdout), ("stderr", &mut stderr)] {
            assert_eq!(page[key]["lossy"], false);
            text.push_str(page[key]["text"].as_str().unwrap());
        }
        if page["stdout"]["has_more"] == false && page["stderr"]["has_more"] == false {
            break;
        }
        assert_ne!(cursor, page["next_cursor"]);
        cursor = page["next_cursor"].clone();
    }
    assert_eq!(stdout, "あ😀\r\n".repeat(40));
    assert_eq!(stderr, "終わり\n");
    assert_eq!(
        call(
            &j,
            "command_status",
            json!({"job_id":id,"max_output_bytes":1})
        )
        .await["error"]["field"],
        "max_output_bytes"
    );
    assert_eq!(
        call(
            &j,
            "exec_command",
            json!({"request_id":"unicode","argv":["unicode"],"wait_ms":0})
        )
        .await["job_id"],
        id
    );
    assert!(j.shutdown().await);
}

#[tokio::test]
async fn job_list_is_bounded_live_and_scoped_to_runtime_and_limit() {
    let (_dir, j) = setup(Settings::default()).await;
    assert_eq!(call(&j, "list_jobs", json!({})).await["jobs"], json!([]));
    for n in 0..3 {
        let started = call(
            &j,
            "exec_command",
            json!({"request_id":format!("job-{n}"),"argv":["exit","0"],"wait_ms":0}),
        )
        .await;
        done(&j, started["job_id"].as_str().unwrap()).await;
    }
    let first = call(&j, "list_jobs", json!({"limit":2})).await;
    assert_eq!(first["jobs"].as_array().unwrap().len(), 2);
    assert_eq!(first["has_more"], true);
    let second = call(
        &j,
        "list_jobs",
        json!({"limit":2,"cursor":first["next_cursor"]}),
    )
    .await;
    assert_eq!(second["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(second["has_more"], false);
    assert_eq!(
        call(
            &j,
            "list_jobs",
            json!({"limit":1,"cursor":first["next_cursor"]})
        )
        .await["error"]["code"],
        "invalid_cursor"
    );
    let bad = session::dispatch(
        j.clone(),
        ToolCall {
            name: "list_jobs".into(),
            arguments: json!({
        "session_id":j.session.id,"runtime_id":"old"}),
        },
    )
    .await;
    assert_eq!(bad.structured_content.unwrap()["ok"], false);
    assert!(j.shutdown().await);
}

#[tokio::test]
async fn searches_use_literal_by_default_and_report_limits_and_failures() {
    let (_dir, j) = setup(Settings {
        search_executable: Some(rg()),
        ..Settings::default()
    })
    .await;
    std::fs::create_dir(j.session.cwd.join(".git")).unwrap();
    write(&j, ".ignore", "ignored.txt\n");
    write(&j, "ignored.txt", "a.b\n");
    write(&j, ".hidden.txt", "a.b\n");
    write(&j, "日本 語.txt", "a.b\naxb\n");
    write(&j, "second.txt", "a.b\n");
    let literal = call(&j, "search_files", json!({"pattern":"a.b"})).await;
    assert_eq!(literal["matches"].as_array().unwrap().len(), 2);
    let regex = call(&j, "search_files", json!({"pattern":"a.b","regex":true})).await;
    assert_eq!(regex["matches"].as_array().unwrap().len(), 3);
    let all = call(
        &j,
        "search_files",
        json!({"pattern":"a.b","hidden":true,"respect_ignore":false}),
    )
    .await;
    assert_eq!(all["matches"].as_array().unwrap().len(), 4);
    let limited = call(&j, "list_files", json!({"glob":"*.txt","limit":1})).await;
    assert_eq!(limited["files"].as_array().unwrap().len(), 1);
    assert_eq!(limited["truncated"], true);
    assert_eq!(
        call(&j, "search_files", json!({"pattern":"[","regex":true})).await["error"]["field"],
        "pattern"
    );
    assert_eq!(
        call(&j, "search_files", json!({"pattern":"no matches"})).await["matches"],
        json!([])
    );
    let outside = _dir.path().join("outside.txt");
    std::fs::write(&outside, "outside needle").unwrap();
    let read = call(
        &j,
        "search_files",
        json!({"path":outside,"pattern":"needle"}),
    )
    .await;
    assert_eq!(read["matches"].as_array().unwrap().len(), 1);
    let outside_dir = _dir.path().join("outside");
    std::fs::create_dir(&outside_dir).unwrap();
    std::fs::write(outside_dir.join("secret.txt"), "needle").unwrap();
    std::os::unix::fs::symlink(&outside_dir, j.session.cwd.join("link")).unwrap();
    assert_eq!(
        call(&j, "search_files", json!({"pattern":"needle"})).await["matches"],
        json!([])
    );
    use std::os::unix::ffi::OsStringExt;
    let raw_path = _dir
        .path()
        .join(std::ffi::OsString::from_vec(b"non-utf8-\xff".to_vec()));
    match std::fs::create_dir(&raw_path) {
        Ok(()) => {
            std::os::unix::fs::symlink(&raw_path, j.session.cwd.join("raw-alias")).unwrap();
            assert_eq!(
                call(&j, "list_files", json!({"path":"raw-alias"})).await["error"]["code"],
                "search_failed"
            );
        }
        // APFS rejects such names before they can become a search target.
        Err(error) => assert_eq!(error.raw_os_error(), Some(libc::EILSEQ)),
    }
    write(&j, "binary.txt", b"needle\xff\n");
    let non_text = call(
        &j,
        "search_files",
        json!({"pattern":"needle","glob":"binary.txt"}),
    )
    .await;
    assert_eq!(non_text["matches"], json!([]));
    assert_eq!(non_text["skipped_non_utf8"], 1);
    assert_eq!(non_text["partial"], true);
    for (size, reason) in [(70000, "output_limit"), (2 * 1024 * 1024, "frame_limit")] {
        write(&j, "long-match.txt", "x".repeat(size));
        let limited = call(
            &j,
            "search_files",
            json!({"pattern":"^x+$","regex":true,"glob":"long-match.txt"}),
        )
        .await;
        assert_eq!(limited["truncated"], true);
        assert_eq!(limited["truncation_reason"], reason);
        assert_eq!(limited["partial"], true);
    }
    assert!(j.shutdown().await);
}

#[tokio::test]
async fn search_missing_dependency_and_timeout_are_explicit() {
    let (_dir, j) = setup(Settings {
        search_executable: Some("/usr/bin/false".into()),
        ..Settings::default()
    })
    .await;
    // A trusted executable that does not implement ripgrep cannot yield a
    // fabricated complete result merely because its exit status is 1.
    let result = call(&j, "list_files", json!({})).await;
    assert_eq!(result["error"]["code"], "feature_unavailable");
    assert!(j.shutdown().await);
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("slow-rg");
    std::fs::write(&script, "#!/bin/sh\nif [ \"$2\" = --version ]; then printf 'ripgrep fixture\\n'; else exec /bin/sleep 10; fi\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (_dir, j) = setup(Settings {
        search_executable: Some(script),
        ..Settings::default()
    })
    .await;
    let result = call(&j, "list_files", json!({})).await;
    assert_eq!(result["truncated"], true);
    assert_eq!(result["truncation_reason"], "time_limit");
    assert!(j.shutdown().await);
}

#[tokio::test]
async fn tiny_trusted_page_preserves_the_state_of_an_executed_command() {
    let (_dir, j) = setup(Settings {
        output_page_bytes: 1,
        ..Settings::default()
    })
    .await;
    let started = call(
        &j,
        "exec_command",
        json!({"request_id":"tiny-page","argv":["exit","0"],"wait_ms":0}),
    )
    .await;
    assert_eq!(started["ok"], true);
    let finished = done(&j, started["job_id"].as_str().unwrap()).await;
    assert_eq!(finished["state"], "finished");
    assert_eq!(finished["exit_code"], 0);
    assert_eq!(finished["stdout"]["budget_limited"], true);
    assert_eq!(
        call(&j, "list_jobs", json!({})).await["jobs"][0]["exit_code"],
        0
    );
    assert_eq!(
        call(
            &j,
            "command_status",
            json!({"job_id":started["job_id"],"max_output_bytes":8})
        )
        .await["error"]["code"],
        "delivery_limit"
    );
    assert!(j.shutdown().await);
}

#[tokio::test]
async fn a_writable_search_backend_cannot_bypass_command_authority() {
    for expanded in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let tools = if expanded {
            directory.path().join("tools")
        } else {
            root.join("tools")
        };
        std::fs::create_dir(&tools).unwrap();
        let executable = tools.join("rg");
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        let settings = Settings {
            search_executable: Some(executable),
            extra_roots: if expanded { vec![tools] } else { vec![] },
            ..Settings::default()
        };
        let session =
            Session::new("guarded-backend".into(), root, directory.path(), settings).unwrap();
        assert!(session.search_executable.is_none());
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/app_server.py");
        let (backend, events) = AppServer::spawn(&fixture, &session).await.unwrap();
        let j = Jobs::new(session, backend, events);
        let result = call(&j, "list_files", json!({})).await;
        assert_eq!(result["error"]["code"], "feature_unavailable");
        assert_eq!(
            call(&j, "session_info", json!({})).await["backend_available"],
            true
        );
        assert!(j.shutdown().await);
    }
}

fn patch(path: &str, old: &str, new: &str) -> String {
    format!("--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-{old}\n+{new}\n")
}

#[tokio::test]
async fn session_shutdown_reaps_an_owned_search_process() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let script = directory.path().join("rg-fixture");
    std::fs::write(&script,"#!/bin/sh\nif [ \"$2\" = --version ]; then printf 'ripgrep fixture\\n'; else printf '%s' \"$$\" > \"$TMPDIR/search-pid\"; exec /bin/sleep 10; fi\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (_dir, j) = setup(Settings {
        search_executable: Some(script),
        ..Settings::default()
    })
    .await;
    let pending = tokio::spawn({
        let j = j.clone();
        async move { call(&j, "list_files", json!({})).await }
    });
    let pid_file = j.session.temporary_directory.join("search-pid");
    tokio::time::timeout(Duration::from_secs(2), async {
        while !pid_file.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let pid: libc::pid_t = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();
    assert!(j.shutdown().await);
    let result = pending.await.unwrap();
    assert_eq!(result["truncation_reason"], "session_closing");
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        -1,
        "owned search process survived shutdown"
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}

#[tokio::test]
async fn edit_results_follow_real_git_and_receipts_survive_output_expiry() {
    let (_dir, j) = setup(Settings {
        output_ttl_secs: 1,
        ..Settings::default()
    })
    .await;
    write(&j, "text.txt", "before\n");
    let diff = patch("text.txt", "before", "after");
    let first = call(&j, "apply_diff", json!({"request_id":"edit","diff":diff})).await;
    assert_eq!(first["edits"][0]["status"], "applied");
    assert_eq!(first["edits"][0]["planned_added_lines"], 1);
    assert_eq!(
        std::fs::read_to_string(j.session.cwd.join("text.txt")).unwrap(),
        "after\n"
    );
    tokio::time::sleep(Duration::from_millis(1050)).await;
    let again = call(&j, "apply_diff", json!({"request_id":"edit","diff":diff})).await;
    assert_eq!(again["job_id"], first["job_id"]);
    assert_eq!(again["edits"], first["edits"]);
    let rejected = call(
        &j,
        "apply_diff",
        json!({"request_id":"bad-edit","diff":patch("text.txt","wrong","value")}),
    )
    .await;
    assert_eq!(rejected["edits"][0]["status"], "not_applied");
    let multi = "diff --git a/new.txt b/new.txt\nnew file mode 100644\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+created\ndiff --git a/text.txt b/text.txt\ndeleted file mode 100644\n--- a/text.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-after\n";
    let changed = call(&j, "apply_diff", json!({"request_id":"multi","diff":multi})).await;
    assert_eq!(changed["edits"][0]["operation"], "create");
    assert_eq!(changed["edits"][1]["operation"], "delete");
    assert!(!j.session.cwd.join("text.txt").exists());
    assert_eq!(
        std::fs::read_to_string(j.session.cwd.join("new.txt")).unwrap(),
        "created\n"
    );
    let quoted = "diff --git \"a/日本\\t語.txt\" \"b/日本\\t語.txt\"\nnew file mode 100644\n--- /dev/null\n+++ \"b/日本\\t語.txt\"\n@@ -0,0 +1 @@\n+unicode\n";
    let changed = call(
        &j,
        "apply_diff",
        json!({"request_id":"quoted","diff":quoted}),
    )
    .await;
    assert_eq!(changed["edits"][0]["path"], "日本\t語.txt");
    assert_eq!(changed["edits"][0]["operation"], "create");
    assert_eq!(changed["edits"][0]["status"], "applied");
    assert_eq!(
        call(&j, "apply_diff", json!({"request_id":"edit","diff":diff})).await["edits"],
        first["edits"]
    );
    assert!(j.shutdown().await);
}
#[tokio::test]
async fn post_dispatch_edit_failure_is_unknown_and_capacity_rejects_before_changes() {
    let (_dir, j) = setup(Settings::default()).await;
    write(&j, "text.txt", "before\n");
    write(&j, "pause-check", "");
    let editor = j.clone();
    let pending = tokio::spawn(async move {
        call(
            &editor,
            "apply_diff",
            json!({"request_id":"stopped-check","diff":patch("text.txt","before","after")}),
        )
        .await
    });
    let id = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let jobs = call(&j, "list_jobs", json!({})).await;
            if let Some(id) = jobs["jobs"][0]["job_id"].as_str() {
                break id.to_owned();
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let stopped = call(&j, "stop_command", json!({"job_id":id})).await;
    assert_eq!(stopped["stop_ack"]["delivery"], "accepted");
    let result = tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result["edits"][0]["status"], "not_applied");
    assert_eq!(
        std::fs::read_to_string(j.session.cwd.join("text.txt")).unwrap(),
        "before\n"
    );
    assert!(j.shutdown().await);
    let (_dir, j) = setup(Settings::default()).await;
    write(&j, "text.txt", "before\n");
    write(&j, "fail-apply", "");
    let result = call(
        &j,
        "apply_diff",
        json!({"request_id":"failed-apply","diff":patch("text.txt","before","after")}),
    )
    .await;
    assert_eq!(result["edits"][0]["status"], "unknown");
    assert_eq!(result["exit_code"], 1);
    assert!(j.shutdown().await);
    let (_dir, j) = setup(Settings::default()).await;
    write(&j, "text.txt", "before\n");
    write(&j, "lose-apply", "");
    let lost = call(
        &j,
        "apply_diff",
        json!({"request_id":"lost-apply","diff":patch("text.txt","before","after")}),
    )
    .await;
    assert_eq!(lost["state"], "unknown");
    assert_eq!(lost["edits"][0]["status"], "unknown");
    // A backend already lost before shutdown is not a confirmed normal exit.
    assert!(!j.shutdown().await);
    let (_dir, j) = setup(Settings {
        edit_metadata_bytes_total: 1,
        ..Settings::default()
    })
    .await;
    write(&j, "text.txt", "before\n");
    let result = call(
        &j,
        "apply_diff",
        json!({"request_id":"capacity","diff":patch("text.txt","before","after")}),
    )
    .await;
    assert_eq!(result["ok"], false);
    assert_eq!(
        std::fs::read_to_string(j.session.cwd.join("text.txt")).unwrap(),
        "before\n"
    );
    assert_eq!(
        call(&j, "session_info", json!({})).await["receipt_count"],
        0
    );
    assert!(j.shutdown().await);
}
