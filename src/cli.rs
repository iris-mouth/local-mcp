//! Line-oriented local controls; no terminal framework or shell evaluation.
use anyhow::{Context, Result, ensure};
use std::{
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
};

pub fn interactive() -> bool {
    std::io::stdin().is_terminal()
}

pub fn prompt(label: &str, default: Option<&str>) -> Result<String> {
    ensure!(interactive(), "interactive setup requires a terminal");
    eprint!(
        "{label}{}: ",
        default.map(|d| format!(" [{d}]")).unwrap_or_default()
    );
    std::io::stderr().flush()?;
    let mut text = String::new();
    ensure!(
        std::io::stdin().read_line(&mut text)? != 0,
        "input closed; cancelled"
    );
    let value = text.trim();
    let value = if value.is_empty() {
        default.unwrap_or("")
    } else {
        value
    };
    ensure!(!value.is_empty(), "value required; cancelled");
    Ok(value.to_owned())
}

pub fn path(value: &str) -> PathBuf {
    if let Some(suffix) = value.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(suffix);
    }
    PathBuf::from(value)
}

pub fn choose_workspace(recent: &[PathBuf]) -> Result<PathBuf> {
    ensure!(
        interactive(),
        "PROJECT is required when stdin is not a terminal"
    );
    for (i, p) in recent.iter().enumerate() {
        eprintln!("  {}: {}", i + 1, p.display());
    }
    let value = prompt("作業先のパス、または最近使ったworkspaceの番号", None)?;
    let selected = if let Ok(index) = value.parse::<usize>() {
        recent
            .get(index.wrapping_sub(1))
            .context("invalid workspace number; use ./ for a numeric directory name")?
            .clone()
    } else {
        path(&value)
    };
    crate::config::canonical_directory(&selected)
}

pub fn executable(label: &str, name: &str, previous: Option<&Path>) -> Result<PathBuf> {
    let found = previous.map(Path::to_owned).or_else(|| {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|dir| dir.join(name))
            .find(|p| crate::connection::executable(p).is_ok())
    });
    let default = found.as_deref().and_then(Path::to_str);
    crate::connection::executable(&path(&prompt(label, default)?))
}

pub const HELP: &str = "/help  /status  /jobs  /stop JOB_ID  /tunnel-restart  /quit\nworkspaceは起動中固定。Terminalを閉じると停止処理を行います。";

pub(crate) fn status(jobs: &crate::jobs::Jobs, connection: Option<&crate::connection::Connection>) {
    use crate::jobs::State;
    let list = jobs.summaries();
    let active = list
        .iter()
        .filter(|j| !matches!(j.state, State::Finished | State::Unknown))
        .count();
    let unknown = list.iter().filter(|j| j.state == State::Unknown).count();
    eprintln!(
        "workspace: {} | 未終了: {active} | 不明: {unknown} | backend: {}",
        jobs.session.cwd.display(),
        if jobs.backend.alive() {
            "利用可能"
        } else {
            "利用不可"
        }
    );
    if let Some(c) = connection {
        c.status();
    }
    eprintln!("/jobs でジョブ確認、/help で操作一覧");
}

pub(crate) fn show_jobs(jobs: &crate::jobs::Jobs) {
    eprintln!("JOB_ID\t状態\t終了コード");
    for j in jobs.summaries() {
        eprintln!(
            "{}\t{}\t{}",
            j.id,
            j.state.label(),
            j.exit_code
                .map(|n| n.to_string())
                .unwrap_or_else(|| "-".into())
        );
    }
}

pub(crate) fn stop_result(id: &str, result: rmcp::model::CallToolResult) {
    let Some(value) = result.structured_content else {
        eprintln!("{id}: 停止要求の結果不明。/jobs で確認してください。");
        return;
    };
    if value["ok"] != true {
        eprintln!(
            "{id}: 停止要求を送れませんでした: {}",
            value["error"]["message"]
                .as_str()
                .unwrap_or("unknown error")
        );
    } else if value["state"] == "finished" {
        eprintln!("{id}: 終了済み。終了コード: {}", value["exit_code"]);
    } else {
        let message = match value["stop_ack"]["delivery"].as_str() {
            Some("accepted") => "停止要求（実行終了は /jobs で確認）: 受付済み",
            Some("rejected") => "停止要求は拒否されました。/jobs で確認してください。",
            _ => "停止要求の配送結果は不明です。/jobs で確認してください。",
        };
        eprintln!("{id}: {message}");
    }
}

pub(crate) fn request_quit(
    jobs: &crate::jobs::Jobs,
    pending: &mut bool,
    interactive: bool,
) -> bool {
    if *pending || !interactive || jobs.close_if_idle() {
        return true;
    }
    let count = jobs
        .summaries()
        .iter()
        .filter(|j| j.state != crate::jobs::State::Finished)
        .count();
    *pending = true;
    eprintln!(
        "未終了・不明のジョブが{count}件あります。停止して終了しますか？ [y/N]（再度Ctrl-Cで終了処理）"
    );
    false
}

#[derive(Debug, PartialEq)]
pub enum Control<'a> {
    Help,
    Status,
    Jobs,
    Stop(&'a str),
    Restart,
    Quit,
    Invalid,
}

pub fn control(line: &str) -> Control<'_> {
    let mut words = line.split_whitespace();
    let action = match words.next() {
        Some("/help") => Control::Help,
        Some("/status") => Control::Status,
        Some("/jobs") => Control::Jobs,
        Some("/stop") => match words.next() {
            Some(id) => Control::Stop(id),
            None => Control::Invalid,
        },
        Some("/tunnel-restart") => Control::Restart,
        Some("/quit") => Control::Quit,
        _ => Control::Invalid,
    };
    if words.next().is_some() {
        Control::Invalid
    } else {
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_never_evaluate_shell_input() {
        assert_eq!(control("/stop job"), Control::Stop("job"));
        for text in [
            "/stop",
            "/stop job extra",
            "/quit yes",
            "echo hi",
            "/status; rm",
        ] {
            assert_eq!(control(text), Control::Invalid);
        }
    }
}
