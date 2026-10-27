//! Mac-side preferences only. Never loaded from MCP arguments or the workspace.
use crate::config::Settings;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub version: u32,
    pub runtime: Option<PathBuf>,
    pub settings: Settings,
    pub recent_workspaces: Vec<PathBuf>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: 1,
            runtime: None,
            settings: Settings::default(),
            recent_workspaces: vec![],
        }
    }
}

pub fn directory() -> Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("HOME").context("HOME is required for local preferences")?)
            .join("Library/Application Support/local-mcp"),
    )
}

fn private(path: &Path, directory: bool) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        !meta.file_type().is_symlink()
            && meta.uid() == unsafe { libc::geteuid() }
            && meta.mode() & 0o077 == 0
            && if directory {
                meta.is_dir()
            } else {
                meta.is_file()
            },
        "insecure preferences path: {}",
        path.display()
    );
    Ok(())
}

fn read_at(root: &Path) -> Result<Preferences> {
    match fs::symlink_metadata(root) {
        Ok(_) => private(root, true)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Preferences::default()),
        Err(e) => return Err(e.into()),
    }
    let path = root.join("preferences.json");
    match fs::symlink_metadata(&path) {
        Ok(_) => private(&path, false)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Preferences::default()),
        Err(e) => return Err(e.into()),
    }
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Preferences::default()),
        Err(e) => return Err(e.into()),
    };
    ensure!(
        file.metadata()?.is_file(),
        "preferences must be a regular file"
    );
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 1024 * 1024, "preferences exceed size limit");
    let prefs: Preferences = serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "invalid preferences: {}; repair this file before retrying",
            path.display()
        )
    })?;
    ensure!(
        prefs.version == 1 && prefs.recent_workspaces.len() <= 10,
        "unsupported preferences"
    );
    prefs.settings.validate()?;
    Ok(prefs)
}

pub fn load() -> Result<Preferences> {
    read_at(&directory()?)
}

fn update_at(root: &Path, change: impl FnOnce(&mut Preferences) -> Result<()>) -> Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root)?;
    private(root, true)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root.join("preferences.lock"))?;
    private(&root.join("preferences.lock"), false)?;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } == 0,
        "cannot lock preferences"
    );
    let mut prefs = read_at(root)?;
    change(&mut prefs)?;
    prefs.settings.validate()?;
    let mut temp = tempfile::NamedTempFile::new_in(root)?;
    serde_json::to_writer_pretty(&mut temp, &prefs)?;
    temp.write_all(b"\n")?;
    temp.as_file().sync_all()?;
    temp.persist(root.join("preferences.json"))?;
    File::open(root)?.sync_all()?;
    Ok(())
}

pub fn activate(runtime: PathBuf, settings: Settings) -> Result<()> {
    update_at(&directory()?, |p| {
        p.runtime = Some(runtime);
        p.settings = settings;
        Ok(())
    })
}

pub fn remember(workspace: &Path) -> Result<()> {
    remember_at(&directory()?, workspace)
}

fn remember_at(root: &Path, workspace: &Path) -> Result<()> {
    update_at(root, |p| {
        p.recent_workspaces.retain(|path| path != workspace);
        p.recent_workspaces.insert(0, workspace.to_owned());
        p.recent_workspaces.truncate(10);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn recents_are_bounded_unique_and_do_not_change_authority() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("prefs");
        update_at(&root, |p| {
            p.settings.allow_network = true;
            Ok(())
        })
        .unwrap();
        for n in 0..12 {
            remember_at(&root, Path::new(&format!("/project/{n}"))).unwrap();
        }
        remember_at(&root, Path::new("/project/8")).unwrap();
        let p = read_at(&root).unwrap();
        assert_eq!(p.recent_workspaces.len(), 10);
        assert_eq!(p.recent_workspaces[0], PathBuf::from("/project/8"));
        assert_eq!(
            p.recent_workspaces
                .iter()
                .filter(|p| p.as_path() == Path::new("/project/8"))
                .count(),
            1
        );
        assert!(p.settings.allow_network);
    }
    #[test]
    fn atomic_preferences_keep_settings_and_reject_corruption() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("prefs");
        update_at(&root, |p| {
            p.settings.max_jobs = 2;
            p.runtime = Some("/runtime".into());
            Ok(())
        })
        .unwrap();
        update_at(&root, |p| {
            p.recent_workspaces.push("/workspace".into());
            Ok(())
        })
        .unwrap();
        let p = read_at(&root).unwrap();
        assert_eq!(p.settings.max_jobs, 2);
        assert_eq!(p.runtime.unwrap(), PathBuf::from("/runtime"));
        assert_eq!(
            fs::metadata(root.join("preferences.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::write(root.join("preferences.json"), b"broken").unwrap();
        assert!(read_at(&root).is_err());
        let alias = tmp.path().join("dangling-root");
        std::os::unix::fs::symlink(tmp.path().join("missing-root"), &alias).unwrap();
        assert!(read_at(&alias).is_err());
        assert!(update_at(&root, |_| Ok(())).is_err());
        assert_eq!(fs::read(root.join("preferences.json")).unwrap(), b"broken");
    }
    #[test]
    fn symlinked_preferences_are_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("prefs");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("missing"), root.join("preferences.json"))
            .unwrap();
        assert!(read_at(&root).is_err());
    }
}
