use crate::{config::Session, protocol::*};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::json;
use std::{
    io::{BufRead, BufReader, Read},
    path::Path,
    process::Stdio,
};
use tokio::io::AsyncWriteExt;

pub async fn read(session: &Session, args: ReadArgs) -> Result<CallToolResult> {
    ensure!(
        args.start_line.unwrap_or(1) > 0 && args.max_lines != Some(0),
        "line range must be positive"
    );
    ensure!(args.path.len() <= 4096, "path too long");
    let path = session.cwd.join(&args.path);
    let cap = session.settings.max_message_bytes / 16;
    let start = args.start_line.unwrap_or(1);
    let max_lines = args.max_lines.unwrap_or(usize::MAX);
    let value = tokio::task::spawn_blocking(move || -> Result<_> {
        let file = open_regular(&path)?;
        let mut reader = BufReader::new(file);
        let mut number = 1usize;
        let mut text = String::new();
        let mut count = 0usize;
        let mut more = false;
        loop {
            if count >= max_lines || text.len() == cap {
                more = !reader.fill_buf()?.is_empty();
                break;
            }
            let mut line = Vec::new();
            let mut found = false;
            let mut eof = false;
            loop {
                let buffer = reader.fill_buf()?;
                if buffer.is_empty() {
                    eof = true;
                    break;
                }
                let n = buffer
                    .iter()
                    .position(|b| *b == b'\n')
                    .map(|n| n + 1)
                    .unwrap_or(buffer.len());
                found = true;
                if number >= start {
                    ensure!(
                        line.len() + n <= cap,
                        "single line exceeds delivery budget; use CLI byte ranges"
                    );
                    line.extend_from_slice(&buffer[..n]);
                }
                let newline = buffer[n - 1] == b'\n';
                reader.consume(n);
                if newline {
                    break;
                }
            }
            if !found {
                break;
            }
            if number >= start {
                if text.len() + line.len() > cap {
                    more = true;
                    break;
                }
                text.push_str(std::str::from_utf8(&line).context("file is not UTF-8")?);
                count += 1;
            }
            number = number.checked_add(1).context("line range overflow")?;
            if eof {
                break;
            }
        }
        Ok(
            json!({"path":path,"text":text,"start_line":start,"lines_returned":count,
            "next_line":if more{Some(start+count)}else{None},"has_more":more}),
        )
    })
    .await??;
    Ok(success(value))
}
fn open_regular(path: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).context("cannot open file")?;
    ensure!(file.metadata()?.is_file(), "path is not a regular file");
    Ok(file)
}
pub async fn image(session: &Session, args: ImageArgs) -> Result<CallToolResult> {
    ensure!(args.path.len() <= 4096, "path too long");
    let path = session.cwd.join(&args.path);
    let cap = (session.settings.max_message_bytes - 65536) / 4 * 3;
    let read_path = path.clone();
    let bytes = tokio::task::spawn_blocking(move || -> Result<_> {
        let file = open_regular(&read_path)?;
        let mut bytes = vec![];
        file.take((cap + 1) as u64).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= cap, "image exceeds MCP delivery budget");
        Ok(bytes)
    })
    .await??;
    let mime = image_mime(&bytes).context("unsupported or invalid image")?;
    let max_pixels = session.settings.max_image_pixels;
    let bytes = tokio::task::spawn_blocking(move || -> Result<_> {
        crate::image_validation::validate(&bytes, max_pixels)?;
        Ok(bytes)
    })
    .await??;
    let mut result = success(json!({"path":path,"mime_type":mime,"bytes":bytes.len()}));
    result
        .content
        .push(ContentBlock::image(STANDARD.encode(bytes), mime));
    Ok(result)
}
pub fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"BM") {
        Some("image/bmp")
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        Some("image/tiff")
    } else if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && matches!(&bytes[8..12], b"avif" | b"avis")
    {
        Some("image/avif")
    } else {
        None
    }
}

/// Read metadata interpreted by Git rather than parsing model-supplied patch headers.
async fn diff_metadata(session: &Session, patch: &str, format: &str) -> Result<Vec<u8>> {
    let mut child = tokio::process::Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "apply",
            format,
            "-z",
            "--recount",
            "--whitespace=nowarn",
            "-",
        ])
        .current_dir(&session.cwd)
        .env_clear()
        .envs(&session.environment)
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = child.stdin.take().context("missing Git stdin")?;
    let bytes = patch.as_bytes().to_vec();
    let writer = tokio::spawn(async move {
        let result = stdin.write_all(&bytes).await;
        drop(stdin);
        result
    });
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), child.wait_with_output())
        .await??;
    writer.await??;
    ensure!(output.status.success(), "Git rejected diff syntax");
    Ok(output.stdout)
}

pub async fn validate_diff(session: &Session, patch: &str) -> Result<()> {
    let summary = diff_metadata(session, patch, "--summary").await?;
    for line in std::str::from_utf8(&summary)?.lines() {
        let fields: Vec<_> = line.split_ascii_whitespace().collect();
        let regular = |mode: &str| matches!(mode, "100644" | "100755");
        let modes_valid = match fields.as_slice() {
            ["create" | "delete", "mode", mode, ..] => regular(mode),
            ["mode", "change", old, "=>", new, ..] => regular(old) && regular(new),
            _ => true,
        };
        ensure!(modes_valid, "patch target must be a regular text file");
    }
    let targets = diff_metadata(session, patch, "--numstat").await?;
    let mut count = 0;
    for entry in targets.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let parts: Vec<_> = entry.splitn(3, |b| *b == b'\t').collect();
        ensure!(
            parts.len() == 3 && parts[0] != b"-" && parts[1] != b"-",
            "only text patches are supported"
        );
        let name = std::str::from_utf8(parts[2])?;
        ensure!(!name.is_empty(), "renamed binary/unsupported patch path");
        let relative = Path::new(name);
        ensure!(
            !relative.is_absolute()
                && relative
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_)))
                && !relative
                    .components()
                    .any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git")),
            "invalid patch target"
        );
        let path = session.cwd.join(relative);
        let mut parent = path.as_path();
        while !parent.exists() {
            parent = parent.parent().context("invalid target parent")?;
        }
        let resolved = std::fs::canonicalize(parent)?;
        ensure!(
            resolved.starts_with(&session.cwd),
            "patch target escapes project"
        );
        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
            ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "patch target must be a regular text file"
            );
            // Validate UTF-8 without retaining a whole source file in memory.
            let file = open_regular(&path)?;
            let mut reader = BufReader::new(file);
            let mut chunk = [0u8; 8192];
            let mut tail = Vec::new();
            loop {
                let n = reader.read(&mut chunk)?;
                if n == 0 {
                    ensure!(tail.is_empty(), "target is not UTF-8");
                    break;
                }
                tail.extend_from_slice(&chunk[..n]);
                match std::str::from_utf8(&tail) {
                    Ok(_) => tail.clear(),
                    Err(e) => {
                        ensure!(e.error_len().is_none(), "target is not UTF-8");
                        tail.drain(..e.valid_up_to());
                    }
                }
            }
        }
        count += 1;
    }
    ensure!(count > 0, "diff has no text targets");
    Ok(())
}
