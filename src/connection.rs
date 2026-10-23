//! Trusted local deployment and session-scoped Tunnel lifecycle. No MCP APIs.
use crate::{
    config::{self, Settings},
    session,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Runtime {
    version: u32,
    tunnel_id: String,
    codex: PathBuf,
    keychain_service: String,
    keychain_account: String,
    local_mcp_sha256: String,
    tunnel_client_sha256: String,
}

fn digest(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn tunnel_id(id: &str) -> Result<()> {
    ensure!(
        id.starts_with("tunnel_")
            && id.len() > 7
            && id.len() <= 100
            && id[7..].bytes().all(|c| c.is_ascii_alphanumeric()),
        "expected a Tunnel ID, not an API key"
    );
    Ok(())
}

fn executable(path: &Path) -> Result<PathBuf> {
    let path = path
        .canonicalize()
        .with_context(|| format!("cannot resolve {}", path.display()))?;
    let meta = path.metadata()?;
    ensure!(
        meta.is_file() && meta.mode() & 0o111 != 0,
        "not an executable file: {}",
        path.display()
    );
    Ok(path)
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn write_private(path: &Path, content: &[u8], mode: u32) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)?;
    file.write_all(content)?;
    Ok(())
}

fn profile(root: &Path, id: &str) -> Result<String> {
    // The official client accepts an argv command string. POSIX quoting preserves
    // paths; env removes connection keys before the MCP binary is executed.
    let command = format!(
        "/usr/bin/env -u CONTROL_PLANE_API_KEY -u OPENAI_API_KEY {} mcp",
        quote(
            root.join("bin/local-mcp")
                .to_str()
                .context("UTF-8 runtime path required")?
        )
    );
    Ok(format!(
        "config_version: 1\ncontrol_plane:\n  base_url: \"https://api.openai.com\"\n  tunnel_id: {}\n  api_key: \"env:CONTROL_PLANE_API_KEY\"\nhealth:\n  listen_addr: \"127.0.0.1:0\"\nadmin_ui:\n  open_browser: false\nlog:\n  level: info\n  format: json\nmcp:\n  commands:\n    - channel: main\n      command: {}\n",
        serde_json::to_string(id)?,
        serde_json::to_string(&command)?
    ))
}

pub fn prepare(
    destination: PathBuf,
    id: String,
    client: PathBuf,
    codex: PathBuf,
    service: String,
    account: Option<String>,
) -> Result<()> {
    tunnel_id(&id)?;
    ensure!(
        !service.is_empty() && !service.contains(['\n', '\0']),
        "invalid Keychain service"
    );
    let binary = executable(&std::env::current_exe()?)?;
    let client = executable(&client)?;
    let codex = executable(&codex)?;
    let account = match account {
        Some(account) => account,
        None => {
            let output = std::process::Command::new("/usr/bin/id")
                .arg("-un")
                .output()?;
            ensure!(output.status.success(), "cannot resolve local account");
            String::from_utf8(output.stdout)?.trim().to_owned()
        }
    };
    ensure!(
        !account.is_empty() && !account.contains(['\n', '\0']),
        "invalid Keychain account"
    );
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let destination = parent.join(
        destination
            .file_name()
            .context("destination requires a directory name")?,
    );
    // Do not merge with an existing runtime, including an empty directory or symlink.
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&destination)
        .context("destination must not exist; prepare a new directory for review")?;
    for name in ["bin", "client", "profiles", "state", "logs"] {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(destination.join(name))?;
    }
    fs::copy(&binary, destination.join("bin/local-mcp"))?;
    fs::copy(&client, destination.join("client/tunnel-client"))?;
    for name in ["bin/local-mcp", "client/tunnel-client"] {
        fs::set_permissions(destination.join(name), fs::Permissions::from_mode(0o700))?;
    }
    let runtime = Runtime {
        version: 1,
        tunnel_id: id.clone(),
        codex,
        keychain_service: service,
        keychain_account: account,
        local_mcp_sha256: digest(&destination.join("bin/local-mcp"))?,
        tunnel_client_sha256: digest(&destination.join("client/tunnel-client"))?,
    };
    write_private(
        &destination.join("runtime.json"),
        &serde_json::to_vec_pretty(&runtime)?,
        0o600,
    )?;
    write_private(
        &destination.join("profiles/local-mcp.yaml"),
        profile(&destination, &id)?.as_bytes(),
        0o600,
    )?;
    let root = destination
        .to_str()
        .context("UTF-8 runtime path required")?;
    let launcher = format!(
        "#!/bin/sh\nset -eu\numask 077\nexport PATH=\"/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:${{PATH:-}}\"\nexec {} connect --runtime {} \"$@\"\n",
        quote(&format!("{root}/bin/local-mcp")),
        quote(root)
    );
    write_private(
        &destination.join("start-local.command"),
        launcher.as_bytes(),
        0o700,
    )?;
    println!(
        "Prepared {}. No keys read and no services started. Review runtime.json and start-local.command before connecting.",
        destination.display()
    );
    Ok(())
}

fn load(root: &Path) -> Result<Runtime> {
    let runtime: Runtime = serde_json::from_slice(&fs::read(root.join("runtime.json"))?)?;
    ensure!(runtime.version == 1, "unsupported runtime version");
    tunnel_id(&runtime.tunnel_id)?;
    ensure!(
        digest(&root.join("bin/local-mcp"))? == runtime.local_mcp_sha256
            && digest(&root.join("client/tunnel-client"))? == runtime.tunnel_client_sha256,
        "runtime binary hash mismatch; prepare a new runtime instead of replacing files in place"
    );
    Ok(runtime)
}

/// A stable flock inode avoids removing a lock that another process just acquired.
fn lease(id: &str) -> Result<File> {
    tunnel_id(id)?;
    let directory = config::socket_directory();
    match fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error.into()),
    }
    let meta = fs::symlink_metadata(&directory)?;
    ensure!(
        meta.is_dir()
            && !meta.file_type().is_symlink()
            && meta.uid() == unsafe { libc::geteuid() }
            && meta.mode() & 0o077 == 0,
        "insecure session socket directory"
    );
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(directory.join(format!("{id}.connection.lock")))?;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file() && meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0,
        "insecure Tunnel lock"
    );
    use std::os::fd::AsRawFd;
    ensure!(
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "this Tunnel is already owned by another local-mcp connection"
    );
    Ok(file)
}

pub async fn connect(
    project: Option<PathBuf>,
    root: Option<PathBuf>,
    settings: Settings,
) -> Result<()> {
    let root = match root {
        Some(root) => root,
        None => std::env::current_exe()?
            .parent()
            .and_then(Path::parent)
            .context("specify --runtime")?
            .to_owned(),
    }
    .canonicalize()?;
    let runtime = load(&root)?;
    let project = match project {
        Some(project) => project,
        None => {
            eprint!("作業するプロジェクトの絶対パス: ");
            std::io::stderr().flush()?;
            let mut text = String::new();
            std::io::stdin().read_line(&mut text)?;
            ensure!(!text.trim().is_empty(), "project required");
            PathBuf::from(text.trim())
        }
    };
    let project = config::canonical_directory(&project)?;
    let guard = lease(&runtime.tunnel_id)?;
    let connection = Connection {
        root,
        child: None,
        retry_at: None,
        restarts: 0,
        state: "not started",
        _guard: guard,
    };
    session::start_connected(None, settings, runtime.codex, project, connection).await
}

/// Runs in a separate process: the session owner never receives credential bytes.
pub fn tunnel_run(root: &Path, id: &str) -> Result<()> {
    config::validate_session_id(id)?;
    let runtime = load(root)?;
    let output = std::process::Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-s",
            &runtime.keychain_service,
            "-a",
            &runtime.keychain_account,
            "-w",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .context("cannot access macOS Keychain")?;
    ensure!(
        output.status.success(),
        "Keychain item unavailable; unlock or authorize it in macOS, then use /tunnel-restart"
    );
    let key = String::from_utf8(output.stdout).context("invalid Keychain value")?;
    let key = key.trim();
    ensure!(
        !key.is_empty() && key.len() <= 16384 && !key.contains(['\n', '\0']),
        "invalid Keychain value"
    );
    let mut command = tunnel_command(root, id, key);
    Err(command.exec()).context("cannot execute official Tunnel client")
}

fn tunnel_command(root: &Path, id: &str, key: &str) -> std::process::Command {
    let mut command = std::process::Command::new(root.join("client/tunnel-client"));
    command
        .args(["run", "--profile", "local-mcp", "--profile-dir"])
        .arg(root.join("profiles"))
        .arg("--health.url-file")
        .arg(root.join("state/health.url"))
        .arg("--pid.file")
        .arg(root.join("state/tunnel.pid"))
        .env_clear()
        .envs(config::safe_environment())
        .env("LOCAL_MCP_SESSION", id)
        .env("CONTROL_PLANE_API_KEY", key);
    command
}

pub struct Connection {
    root: PathBuf,
    child: Option<OwnedTunnel>,
    retry_at: Option<Instant>,
    restarts: u32,
    state: &'static str,
    _guard: File,
}

/// The unreaped leader reserves the process-group ID until all group signals
/// have been sent. No Tokio waiter or other task may reap this std Child.
struct OwnedTunnel {
    child: Option<Child>,
}

impl OwnedTunnel {
    fn leader_exited(&self) -> Result<bool> {
        let child = self
            .child
            .as_ref()
            .context("Tunnel leader already reaped")?;
        // WNOWAIT observes exit without freeing the PID. Success also verifies
        // that this process still owns that child; ECHILD must never signal it.
        loop {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    child.id() as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result == 0 {
                let pid = unsafe { info.si_pid() };
                ensure!(
                    pid == 0 || pid == child.id() as i32,
                    "unexpected Tunnel wait result"
                );
                return Ok(pid != 0);
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error).context("cannot verify owned Tunnel leader");
            }
        }
    }

    fn signal_group(&self, signal: i32) -> Result<()> {
        self.leader_exited()?;
        let pid = self.child.as_ref().unwrap().id();
        if unsafe { libc::kill(-(pid as i32), signal) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            // Darwin returns EPERM even for a group containing only zombies.
            // Verify that no live group member remains; other permission errors
            // must still fail rather than claiming successful cleanup.
            #[cfg(target_os = "macos")]
            if error.raw_os_error() == Some(libc::EPERM)
                && self.leader_exited()?
                && group_has_no_live_members(pid as i32)?
            {
                return Ok(());
            }
            return Err(error).context("cannot signal owned Tunnel group");
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<ExitStatus> {
        // Even after a leader-only crash, its zombie retains this group's ID.
        // Clear surviving connection children before consuming the leader exit.
        self.signal_group(libc::SIGKILL)?;
        self.child
            .take()
            .unwrap()
            .wait()
            .context("Tunnel exit not confirmed")
    }

    fn poll_exit(&mut self) -> Result<Option<ExitStatus>> {
        if self.leader_exited()? {
            self.finish().map(Some)
        } else {
            Ok(None)
        }
    }

    async fn stop(&mut self) -> Result<()> {
        self.signal_group(libc::SIGTERM)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.leader_exited()? && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        self.finish()?;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn group_has_no_live_members(group: i32) -> Result<bool> {
    // PROC_PGRP_ONLY from the macOS sys/proc_info.h ABI. Inspect only the group
    // whose ID is still reserved by our unreaped child; never signal listed PIDs.
    const PROC_PGRP_ONLY: u32 = 2;
    let mut capacity = 64;
    loop {
        let mut pids = vec![0_i32; capacity];
        let bytes = std::mem::size_of_val(pids.as_slice());
        unsafe {
            *libc::__error() = 0;
        }
        let count = unsafe {
            libc::proc_listpids(
                PROC_PGRP_ONLY,
                group as u32,
                pids.as_mut_ptr().cast(),
                bytes as i32,
            )
        };
        if count < 0 || (count == 0 && std::io::Error::last_os_error().raw_os_error() != Some(0)) {
            return Err(std::io::Error::last_os_error())
                .context("cannot inspect owned Tunnel group");
        }
        if count as usize == bytes {
            ensure!(
                capacity < 4096,
                "owned Tunnel group exceeds inspection budget"
            );
            capacity *= 2;
            continue;
        }
        for pid in pids
            .into_iter()
            .take(count as usize / std::mem::size_of::<i32>())
            .filter(|pid| *pid > 0)
        {
            let mut info: libc::proc_bsdshortinfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of_val(&info) as i32;
            let read = unsafe {
                libc::proc_pidinfo(
                    pid,
                    libc::PROC_PIDT_SHORTBSDINFO,
                    0,
                    (&mut info as *mut libc::proc_bsdshortinfo).cast(),
                    size,
                )
            };
            if read != size {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ESRCH) {
                    continue;
                }
                return Err(error).context("cannot inspect owned Tunnel group member");
            }
            if info.pbsi_pgid == group as u32 && info.pbsi_status != libc::SZOMB {
                return Ok(false);
            }
        }
        return Ok(true);
    }
}

impl Drop for OwnedTunnel {
    fn drop(&mut self) {
        if self.child.is_some()
            && let Err(error) = self.finish()
        {
            eprintln!("Owned Tunnel cleanup was not confirmed: {error}");
        }
    }
}

impl Connection {
    pub fn start(&mut self, id: &str) -> Result<()> {
        for file in ["state/health.url", "state/tunnel.pid"] {
            match fs::remove_file(self.root.join(file)) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
            }
        }
        let log = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.root.join("logs/local-mcp.log"))?;
        let mut command = Command::new(self.root.join("bin/local-mcp"));
        command
            .arg("tunnel-run")
            .arg("--runtime")
            .arg(&self.root)
            .arg("--session")
            .arg(id)
            .env_clear()
            .envs(config::safe_environment())
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .process_group(0);
        self.child = Some(OwnedTunnel {
            child: Some(
                command
                    .spawn()
                    .context("cannot start Tunnel credential helper")?,
            ),
        });
        self.state = "client started (remote readiness not confirmed)";
        self.retry_at = None;
        eprintln!(
            "Tunnel client started. Health URL file: {}. Log: {}",
            self.root.join("state/health.url").display(),
            self.root.join("logs/local-mcp.log").display()
        );
        Ok(())
    }

    pub fn status(&self) {
        eprintln!(
            "Tunnel: {}; automatic restarts: {}/3",
            self.state, self.restarts
        );
    }

    pub fn tick(&mut self, id: &str) -> Result<()> {
        if let Some(child) = &mut self.child
            && let Some(status) = child.poll_exit()?
        {
            self.child = None;
            self.state = "stopped";
            if status.code() == Some(78) {
                eprintln!(
                    "Tunnel credential/startup failure. Inspect the log; use /tunnel-restart after resolving it. The local session remains alive."
                );
            } else if self.restarts < 3 {
                self.retry_at = Some(Instant::now() + Duration::from_secs(1 << self.restarts));
                self.state = "waiting to restart";
                eprintln!(
                    "Tunnel client exited ({status}); scheduling restart. The original session and jobs remain alive."
                );
            } else {
                eprintln!(
                    "Tunnel restart limit reached. Inspect the log, then use /tunnel-restart."
                );
            }
        }
        if self.retry_at.is_some_and(|when| Instant::now() >= when) {
            self.retry_at = None;
            self.restarts += 1;
            if let Err(error) = self.start(id) {
                self.state = "stopped (restart failed)";
                return Err(error);
            }
        }
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        self.retry_at = None;
        if let Some(mut child) = self.child.take() {
            child.stop().await?;
        }
        self.state = "stopped";
        for file in ["state/health.url", "state/tunnel.pid"] {
            let _ = fs::remove_file(self.root.join(file));
        }
        Ok(())
    }

    pub async fn restart(&mut self, id: &str) -> Result<()> {
        self.stop().await?;
        self.restarts = 0;
        self.start(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_command_removes_keys_and_preserves_session_with_quoted_path() {
        let directory = tempfile::Builder::new()
            .prefix("local-mcp quote ' ")
            .tempdir()
            .unwrap();
        let root = directory.path();
        fs::create_dir(root.join("bin")).unwrap();
        write_private(&root.join("bin/local-mcp"), b"#!/bin/sh\nprintf '%s|%s|%s|%s' \"$1\" \"$LOCAL_MCP_SESSION\" \"${CONTROL_PLANE_API_KEY-unset}\" \"${OPENAI_API_KEY-unset}\"\n", 0o700).unwrap();
        let text = profile(root, "tunnel_fixture").unwrap();
        let command: String = serde_json::from_str(
            text.lines()
                .find_map(|line| line.strip_prefix("      command: "))
                .unwrap(),
        )
        .unwrap();
        let output = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(command)
            .env_clear()
            .env("LOCAL_MCP_SESSION", "chosen-session")
            .env("CONTROL_PLANE_API_KEY", "dummy-only")
            .env("OPENAI_API_KEY", "dummy-only")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"mcp|chosen-session|unset|unset");
    }

    #[test]
    fn tunnel_environment_contains_only_allowlisted_values_and_its_own_key() {
        let command = tunnel_command(Path::new("/tmp/fixture"), "chosen-session", "dummy-only");
        let env: std::collections::BTreeMap<_, _> = command
            .get_envs()
            .map(|(k, v)| (k.to_str().unwrap(), v.unwrap().to_str().unwrap()))
            .collect();
        assert_eq!(env.get("CONTROL_PLANE_API_KEY"), Some(&"dummy-only"));
        assert_eq!(env.get("LOCAL_MCP_SESSION"), Some(&"chosen-session"));
        assert!(!env.contains_key("OPENAI_API_KEY"));
        assert!(env.keys().all(|key| {
            [
                "HOME",
                "PATH",
                "LANG",
                "LC_ALL",
                "LC_CTYPE",
                "TERM",
                "TZ",
                "LOCAL_MCP_SESSION",
                "CONTROL_PLANE_API_KEY",
            ]
            .contains(key)
        }));
        assert!(!format!("{:?}", command.get_args().collect::<Vec<_>>()).contains("dummy-only"));
    }

    fn fixture(exit: i32) -> (tempfile::TempDir, Connection, String) {
        let directory = tempfile::tempdir().unwrap();
        for sub in ["bin", "logs", "state"] {
            fs::create_dir(directory.path().join(sub)).unwrap();
        }
        write_private(
            &directory.path().join("bin/local-mcp"),
            format!("#!/bin/sh\nexit {exit}\n").as_bytes(),
            0o700,
        )
        .unwrap();
        let id = format!("tunnel_{}", uuid::Uuid::new_v4().simple());
        let connection = Connection {
            root: directory.path().to_owned(),
            child: None,
            retry_at: None,
            restarts: 0,
            state: "not started",
            _guard: lease(&id).unwrap(),
        };
        (directory, connection, id)
    }

    async fn wait_for_exit(connection: &mut Connection) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if connection.child.as_mut().unwrap().leader_exited().unwrap() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn tunnel_restarts_are_finite_and_spawn_failure_does_not_loop() {
        let (_directory, mut connection, _id) = fixture(1);
        connection.start("fixture").unwrap();
        for expected in 0..=3 {
            wait_for_exit(&mut connection).await;
            connection.tick("fixture").unwrap();
            assert_eq!(connection.restarts, expected);
            if expected < 3 {
                assert!(connection.retry_at.is_some());
                connection.retry_at = Some(Instant::now());
                connection.tick("fixture").unwrap();
            }
        }
        assert!(connection.child.is_none() && connection.retry_at.is_none());
        connection.retry_at = Some(Instant::now());
        fs::remove_file(connection.root.join("bin/local-mcp")).unwrap();
        assert!(connection.tick("fixture").is_err());
        assert!(connection.retry_at.is_none());
        connection.tick("fixture").unwrap();
    }

    #[tokio::test]
    async fn credential_failure_waits_for_manual_restart() {
        let (_directory, mut connection, id) = fixture(78);
        assert!(lease(&id).is_err());
        connection.start("same-session").unwrap();
        wait_for_exit(&mut connection).await;
        connection.tick("same-session").unwrap();
        assert!(connection.child.is_none() && connection.retry_at.is_none());
        connection.restart("same-session").await.unwrap();
        connection.stop().await.unwrap();
        drop(connection);
        assert!(lease(&id).is_ok());
    }
}
