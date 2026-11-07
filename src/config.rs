use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Only the human-controlled startup file can set these ceilings.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub max_jobs: usize,
    pub max_receipts: usize,
    pub output_bytes_per_job: usize,
    pub output_bytes_total: usize,
    pub output_page_bytes: usize,
    pub output_ttl_secs: u64,
    pub max_message_bytes: usize,
    pub max_input_bytes: usize,
    pub max_diff_bytes: usize,
    pub max_image_pixels: usize,
    pub max_rpc_inflight: usize,
    pub rpc_timeout_ms: u64,
    pub shutdown_ms: u64,
    pub search_executable: Option<PathBuf>,
    pub edit_metadata_bytes_total: usize,
    pub write_roots: Vec<PathBuf>,
    pub extra_roots: Vec<PathBuf>,
    pub allow_network: bool,
    pub allow_full_access: bool,
    pub allow_git_metadata: bool,
    pub allow_ssh_agent: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            max_jobs: 4,
            max_receipts: 8192,
            output_bytes_per_job: 4 * 1024 * 1024,
            output_bytes_total: 16 * 1024 * 1024,
            output_page_bytes: 256 * 1024,
            output_ttl_secs: 3600,
            max_message_bytes: 16 * 1024 * 1024,
            max_input_bytes: 256 * 1024,
            max_diff_bytes: 1024 * 1024,
            max_image_pixels: 32 * 1024 * 1024,
            max_rpc_inflight: 64,
            rpc_timeout_ms: 10000,
            shutdown_ms: 5000,
            search_executable: None,
            edit_metadata_bytes_total: 1024 * 1024,
            write_roots: vec![],
            extra_roots: vec![],
            allow_network: false,
            allow_full_access: false,
            allow_git_metadata: false,
            allow_ssh_agent: false,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.max_jobs > 0 && self.max_receipts > 0 && self.edit_metadata_bytes_total > 0,
            "job/receipt limits must be positive"
        );
        ensure!(
            self.output_bytes_per_job > 0 && self.output_bytes_total >= self.output_bytes_per_job,
            "invalid output budget"
        );
        ensure!(
            self.output_page_bytes > 0 && self.output_page_bytes <= self.output_bytes_per_job,
            "invalid output page"
        );
        ensure!(
            self.max_message_bytes >= 65536 && self.max_message_bytes <= 64 * 1024 * 1024,
            "message budget must be 64 KiB..64 MiB"
        );
        ensure!(
            self.output_page_bytes <= self.max_message_bytes / 16
                && self.max_diff_bytes <= self.max_message_bytes / 8
                && self.max_input_bytes <= self.max_message_bytes / 8,
            "payload budgets exceed transport budget"
        );
        ensure!(
            self.max_rpc_inflight > 0
                && self.rpc_timeout_ms > 0
                && self.shutdown_ms > 0
                && self.output_ttl_secs > 0,
            "timeouts and RPC capacity must be positive"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Session {
    pub id: String,
    pub runtime_id: String,
    pub cwd: PathBuf,
    pub roots: Vec<PathBuf>,
    pub git_metadata_candidates: Vec<PathBuf>,
    pub extra_roots: Vec<PathBuf>,
    pub temporary_directory: PathBuf,
    pub diagnostics_directory: PathBuf,
    pub settings: Settings,
    #[serde(skip)]
    pub environment: BTreeMap<String, String>,
    #[serde(skip)]
    pub search_executable: Option<PathBuf>,
}
impl Session {
    pub fn new(id: String, cwd: PathBuf, directory: &Path, mut settings: Settings) -> Result<Self> {
        validate_session_id(&id)?;
        settings.validate()?;
        let cwd = canonical_directory(&cwd)?;
        let directory = canonical_directory(directory)?;
        let mut roots = vec![cwd.clone()];
        for p in &settings.write_roots {
            roots.push(canonical_directory(p)?);
        }
        let mut candidates = vec![];
        // Resolve linked worktree metadata using Git, never by trusting model env.
        for arg in ["--absolute-git-dir", "--git-common-dir"] {
            if let Ok(output) = std::process::Command::new("git")
                .args(["rev-parse", arg])
                .current_dir(&cwd)
                .env_clear()
                .envs(safe_environment())
                .output()
                && output.status.success()
            {
                let path = cwd.join(String::from_utf8(output.stdout)?.trim());
                let path = canonical_directory(&path)?;
                candidates.push(path.clone());
                if settings.allow_git_metadata {
                    roots.push(path);
                }
            }
        }
        candidates.sort();
        candidates.dedup();
        let temporary_directory = directory.join("tmp");
        let diagnostics_directory = directory.join("codex");
        std::fs::create_dir_all(&temporary_directory)?;
        std::fs::create_dir_all(&diagnostics_directory)?;
        roots.push(temporary_directory.clone());
        roots.sort();
        roots.dedup();
        let mut extra_roots = vec![];
        for p in &settings.extra_roots {
            extra_roots.push(canonical_directory(p)?);
        }
        extra_roots.sort();
        extra_roots.dedup();
        settings.write_roots = roots.clone();
        settings.extra_roots = extra_roots.clone();
        let mut environment = safe_environment();
        let search_executable = match &settings.search_executable {
            Some(path) => {
                ensure!(path.is_absolute(), "search_executable must be absolute");
                std::fs::canonicalize(path).ok()
            }
            None => environment.get("PATH").and_then(|path| {
                std::env::split_paths(path)
                    .filter(|p| p.is_absolute())
                    .map(|p| p.join(if cfg!(windows) { "rg.exe" } else { "rg" }))
                    .find(|p| p.is_file())
                    .and_then(|p| std::fs::canonicalize(p).ok())
            }),
        };
        // A permitted write must not replace a host-executed read-only backend.
        // Expansion roots also belong to the fixed authorization ceiling.
        let search_executable = search_executable.filter(|executable| {
            !roots
                .iter()
                .chain(extra_roots.iter())
                .any(|root| executable.starts_with(root))
        });
        environment.insert(
            "TMPDIR".into(),
            temporary_directory.to_string_lossy().into_owned(),
        );
        if settings.allow_ssh_agent
            && let Some(socket) = std::env::var_os("SSH_AUTH_SOCK")
        {
            #[cfg(unix)]
            {
                use std::os::unix::fs::{FileTypeExt, MetadataExt};
                let metadata = std::fs::metadata(&socket)?;
                ensure!(
                    metadata.file_type().is_socket()
                        && metadata.uid() == unsafe { libc::geteuid() },
                    "SSH agent must be a same-user socket"
                );
            }
            environment.insert(
                "SSH_AUTH_SOCK".into(),
                socket.to_string_lossy().into_owned(),
            );
        }
        Ok(Self {
            id,
            runtime_id: uuid::Uuid::new_v4().to_string(),
            cwd,
            roots,
            git_metadata_candidates: candidates,
            extra_roots,
            temporary_directory,
            diagnostics_directory,
            settings,
            environment,
            search_executable,
        })
    }
}
pub fn safe_environment() -> BTreeMap<String, String> {
    ["HOME", "PATH", "LANG", "LC_ALL", "LC_CTYPE", "TERM", "TZ"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|v| (key.into(), v)))
        .collect()
}
pub fn validate_session_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 64
            && id != "."
            && id != ".."
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)),
        "invalid session_id"
    );
    Ok(())
}
pub fn canonical_directory(path: &Path) -> Result<PathBuf> {
    let p = std::fs::canonicalize(path)
        .with_context(|| format!("cannot resolve {}", path.display()))?;
    ensure!(p.is_dir(), "not a directory");
    Ok(p)
}
pub fn socket_path(id: &str) -> Result<PathBuf> {
    validate_session_id(id)?;
    #[cfg(unix)]
    {
        Ok(socket_directory().join(format!("{id}.sock")))
    }
    #[cfg(not(unix))]
    {
        anyhow::bail!("this adapter currently requires macOS or Unix")
    }
}
#[cfg(unix)]
pub fn socket_directory() -> PathBuf {
    PathBuf::from("/tmp").join(format!("local-mcp-{}", unsafe { libc::geteuid() }))
}
