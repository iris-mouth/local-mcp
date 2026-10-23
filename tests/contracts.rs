use local_mcp::{
    app_server::AppServer,
    config::{Session, Settings},
    files,
    jobs::Jobs,
    mcp,
    protocol::*,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

async fn setup(mut settings: Settings) -> (tempfile::TempDir, Arc<Jobs>) {
    settings.rpc_timeout_ms = 80;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let session = Session::new("test".into(), root, dir.path(), settings).unwrap();
    let exe = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/app_server.py");
    let (backend, events) = AppServer::spawn(&exe, &session).await.unwrap();
    (dir, Jobs::new(session, backend, events))
}
fn exec(j: &Jobs, request: &str, argv: &[&str]) -> ExecArgs {
    ExecArgs {
        session_id: j.session.id.clone(),
        runtime_id: j.session.runtime_id.clone(),
        request_id: request.into(),
        argv: argv.iter().map(|s| s.to_string()).collect(),
        cwd: None,
        wait_ms: Some(0),
        input_mode: None,
        size: None,
    }
}
fn status(j: &Jobs, id: &str, cursor: Option<String>) -> StatusArgs {
    StatusArgs {
        session_id: j.session.id.clone(),
        runtime_id: j.session.runtime_id.clone(),
        job_id: Some(id.into()),
        request_id: None,
        cursor,
        wait_ms: Some(0),
    }
}
async fn terminal(j: &Jobs, id: &str) -> Value {
    for _ in 0..100 {
        let v = j
            .status(status(j, id, None))
            .await
            .structured_content
            .unwrap();
        if v["state"] == "finished" || v["state"] == "unknown" {
            return v;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("job did not finish")
}
#[tokio::test]
async fn dedup_retains_nonzero_exit_and_output_is_not_consumed() {
    let (_dir, j) = setup(Settings::default()).await;
    let args = exec(&j, "once", &["exit", "7"]);
    let first = j
        .execute(args.clone(), None, None)
        .await
        .structured_content
        .unwrap();
    let id = first["job_id"].as_str().unwrap();
    let done = terminal(&j, id).await;
    assert_eq!(done["exit_code"], 7);
    assert_eq!(done["ok"], true);
    assert_eq!(done["stdout"]["lossy"], true);
    let again = j
        .execute(args.clone(), None, None)
        .await
        .structured_content
        .unwrap();
    assert_eq!(again["job_id"], id);
    assert_eq!(again["stdout"], done["stdout"]);
    let mut changed = args;
    changed.argv = vec!["exit".into(), "0".into()];
    assert!(j.execute(changed, None, None).await.is_error.unwrap());
    assert!(j.shutdown().await);
}
#[tokio::test]
async fn full_job_capacity_keeps_status_input_and_stop_working() {
    let settings = Settings {
        max_jobs: 1,
        ..Default::default()
    };
    let (_dir, j) = setup(settings).await;
    let mut args = exec(&j, "active", &["hang"]);
    args.input_mode = Some(InputMode::Pipe);
    let first = j
        .execute(args, None, None)
        .await
        .structured_content
        .unwrap();
    let id = first["job_id"].as_str().unwrap().to_owned();
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert!(
        j.execute(exec(&j, "other", &["exit", "0"]), None, None)
            .await
            .is_error
            .unwrap()
    );
    let input = InputArgs {
        session_id: j.session.id.clone(),
        runtime_id: j.session.runtime_id.clone(),
        job_id: id.clone(),
        request_id: "input".into(),
        data: Some("hello".into()),
        eof: None,
        resize: None,
    };
    assert_eq!(
        j.input(input.clone()).await.structured_content.unwrap()["delivery"],
        "accepted"
    );
    assert_eq!(
        j.input(input).await.structured_content.unwrap()["delivery"],
        "accepted"
    );
    let stop = j
        .stop(StopArgs {
            session_id: j.session.id.clone(),
            runtime_id: j.session.runtime_id.clone(),
            job_id: id.clone(),
        })
        .await
        .structured_content
        .unwrap();
    assert_eq!(stop["stop_ack"]["delivery"], "accepted");
    let pending = j
        .status(status(&j, &id, None))
        .await
        .structured_content
        .unwrap();
    assert_eq!(pending["state"], "stop_requested");
    assert!(pending["exit_code"].is_null());
    assert_eq!(terminal(&j, &id).await["exit_code"], 143);
    assert!(j.shutdown().await);
}
#[tokio::test]
async fn lost_input_ack_blocks_further_delivery_but_keeps_observation() {
    let (_dir, j) = setup(Settings::default()).await;
    let mut args = exec(&j, "active", &["noack"]);
    args.input_mode = Some(InputMode::Pipe);
    let first = j
        .execute(args, None, None)
        .await
        .structured_content
        .unwrap();
    let id = first["job_id"].as_str().unwrap().to_owned();
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    let mut input = InputArgs {
        session_id: j.session.id.clone(),
        runtime_id: j.session.runtime_id.clone(),
        job_id: id.clone(),
        request_id: "unknown".into(),
        data: Some("once".into()),
        eof: None,
        resize: None,
    };
    assert_eq!(
        j.input(input.clone()).await.structured_content.unwrap()["delivery"],
        "unknown"
    );
    assert_eq!(
        j.input(input.clone()).await.structured_content.unwrap()["delivery"],
        "unknown"
    );
    input.request_id = "next".into();
    assert!(j.input(input).await.is_error.unwrap());
    let v = j
        .status(status(&j, &id, None))
        .await
        .structured_content
        .unwrap();
    assert_eq!(v["state"], "running");
    assert_eq!(v["input_delivery_unknown"], true);
    assert!(j.shutdown().await);
}
#[tokio::test]
async fn output_gaps_expiry_and_receipt_capacity_preserve_known_exit() {
    let s = Settings {
        max_receipts: 1,
        output_bytes_per_job: 1024,
        output_bytes_total: 2048,
        output_page_bytes: 512,
        output_ttl_secs: 1,
        ..Default::default()
    };
    let (_dir, j) = setup(s).await;
    let args = exec(&j, "flood", &["flood"]);
    let v = j
        .execute(args.clone(), None, None)
        .await
        .structured_content
        .unwrap();
    let id = v["job_id"].as_str().unwrap();
    let v = terminal(&j, id).await;
    assert_eq!(v["stdout"]["gap"], true);
    assert_eq!(v["stdout"]["source_truncated"], false);
    let mut bad = status(&j, id, v["next_cursor"].as_str().map(String::from));
    bad.runtime_id = "other".into();
    assert!(j.status(bad).await.is_error.unwrap());
    tokio::time::sleep(std::time::Duration::from_millis(1050)).await;
    let v = j
        .status(status(&j, id, None))
        .await
        .structured_content
        .unwrap();
    assert_eq!(v["state"], "finished");
    assert_eq!(v["stdout"]["expired"], true);
    assert_eq!(v["exit_code"], 0);
    assert!(
        j.execute(exec(&j, "new", &["exit", "0"]), None, None)
            .await
            .is_error
            .unwrap()
    );
    assert_eq!(
        j.execute(args, None, None)
            .await
            .structured_content
            .unwrap()["job_id"],
        id
    );
    assert!(j.shutdown().await);
}
#[tokio::test]
async fn backend_loss_marks_unfinished_unknown_and_refuses_new_execution() {
    let (_dir, j) = setup(Settings::default()).await;
    let done = j
        .execute(exec(&j, "done", &["exit", "0"]), None, None)
        .await
        .structured_content
        .unwrap();
    let done = done["job_id"].as_str().unwrap();
    terminal(&j, done).await;
    let v = j
        .execute(exec(&j, "lost", &["lose"]), None, None)
        .await
        .structured_content
        .unwrap();
    let id = v["job_id"].as_str().unwrap();
    assert_eq!(terminal(&j, id).await["state"], "unknown");
    assert_eq!(
        j.status(status(&j, done, None))
            .await
            .structured_content
            .unwrap()["exit_code"],
        0
    );
    assert!(
        j.execute(exec(&j, "new", &["exit", "0"]), None, None)
            .await
            .is_error
            .unwrap()
    );
    assert!(
        j.stop(StopArgs {
            session_id: j.session.id.clone(),
            runtime_id: j.session.runtime_id.clone(),
            job_id: id.into()
        })
        .await
        .is_error
        .unwrap()
    );
    j.shutdown().await;
}
#[tokio::test]
async fn roots_cwd_and_runtime_are_checked_before_dispatch() {
    let (dir, j) = setup(Settings::default()).await;
    let mut args = exec(&j, "outside", &["exit", "0"]);
    args.cwd = Some(dir.path().to_string_lossy().into_owned());
    assert!(j.execute(args, None, None).await.is_error.unwrap());
    let mut args = exec(&j, "old", &["exit", "0"]);
    args.runtime_id = "old".into();
    assert!(j.execute(args, None, None).await.is_error.unwrap());
    assert!(
        j.execute(
            exec(&j, "full", &["exit", "0"]),
            Some(Access::Full {}),
            Some("need".into())
        )
        .await
        .is_error
        .unwrap()
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.path(), j.session.cwd.join("escape")).unwrap();
        let mut args = exec(&j, "symlink", &["exit", "0"]);
        args.cwd = Some("escape".into());
        assert!(j.execute(args, None, None).await.is_error.unwrap());
    }
    assert!(j.shutdown().await);
}
#[tokio::test]
async fn read_preserves_crlf_outside_roots_and_schema_rejects_unknown_fields() {
    let (dir, j) = setup(Settings::default()).await;
    let path = dir.path().join("outside.txt");
    std::fs::write(&path, b"first\r\nsecond\n").unwrap();
    let args = ReadArgs {
        session_id: j.session.id.clone(),
        runtime_id: j.session.runtime_id.clone(),
        path: path.to_string_lossy().into_owned(),
        start_line: None,
        max_lines: None,
    };
    assert_eq!(
        files::read(&j.session, args)
            .await
            .unwrap()
            .structured_content
            .unwrap()["text"],
        "first\r\nsecond\n"
    );
    assert!(serde_json::from_value::<ExecArgs>(json!({"session_id":"test","runtime_id":"r","request_id":"x","argv":["true"],"approved":true})).is_err());
    assert_eq!(mcp::tools().len(), 9);
    assert!(
        mcp::tools()
            .iter()
            .all(|t| t.input_schema["additionalProperties"] == false)
    );
    assert!(j.shutdown().await);
}

#[tokio::test]
async fn root_replacement_does_not_expand_authority_or_break_old_receipts() {
    let (dir, j) = setup(Settings::default()).await;
    let args = exec(&j, "before-replacement", &["exit", "0"]);
    let first = j
        .execute(args.clone(), None, None)
        .await
        .structured_content
        .unwrap();
    let id = first["job_id"].as_str().unwrap();
    terminal(&j, id).await;
    std::fs::rename(&j.session.cwd, dir.path().join("old-project")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path(), &j.session.cwd).unwrap();
    assert_eq!(
        j.execute(args, None, None)
            .await
            .structured_content
            .unwrap()["job_id"],
        id
    );
    let rejected = j
        .execute(exec(&j, "after-replacement", &["exit", "0"]), None, None)
        .await
        .structured_content
        .unwrap();
    assert_eq!(rejected["error"]["code"], "authorization_denied");
    assert!(j.shutdown().await);
}
#[tokio::test]
async fn known_exit_survives_incompatible_output_and_cursor_is_job_bound() {
    let (_dir, j) = setup(Settings::default()).await;
    let result = j
        .execute(exec(&j, "buffered", &["buffered"]), None, None)
        .await
        .structured_content
        .unwrap();
    let done = terminal(&j, result["job_id"].as_str().unwrap()).await;
    assert_eq!(done["state"], "finished");
    assert_eq!(done["exit_code"], 0);
    assert_eq!(done["output_unavailable"], true);
    let other = j
        .execute(exec(&j, "other", &["exit", "0"]), None, None)
        .await
        .structured_content
        .unwrap();
    let cursor = done["next_cursor"].as_str().unwrap().to_string();
    assert!(
        j.status(status(&j, other["job_id"].as_str().unwrap(), Some(cursor)))
            .await
            .is_error
            .unwrap()
    );
    assert!(j.shutdown().await);
}
#[test]
fn session_ids_and_socket_paths_preserve_existing_constraints() {
    use local_mcp::config::{socket_path, validate_session_id};
    for id in ["my-project_1.dev", "7418eda5-fd07-4e00-ace5-c1ece2f68a02"] {
        assert!(validate_session_id(id).is_ok());
    }
    for id in ["", "..", "../escape", "contains spaces"] {
        assert!(validate_session_id(id).is_err());
    }
    assert!(validate_session_id(&"x".repeat(65)).is_err());
    #[cfg(unix)]
    assert!(
        socket_path("7418eda5-fd07-4e00-ace5-c1ece2f68a02")
            .unwrap()
            .as_os_str()
            .len()
            < 104
    );
}

#[test]
fn full_access_rejects_fields_that_pretend_to_retain_restrictions() {
    assert!(serde_json::from_value::<Access>(json!({"mode":"full"})).is_ok());
    assert!(serde_json::from_value::<Access>(json!({"mode":"full","roots":[]})).is_err());
    assert!(serde_json::from_value::<Access>(json!({"mode":"full","network":false})).is_err());
    assert!(
        serde_json::from_value::<Access>(
            json!({"mode":"limited","roots":[],"network":false,"approved":true})
        )
        .is_err()
    );
}
