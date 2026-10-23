//! Replacing files we may not own (`/etc/hosts`, `/etc/environment`):
//! keep a timestamped backup first, then write directly if we can or
//! through `pkexec` (one password prompt from the desktop's polkit agent).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, bail};

/// Back up `target` into `~/.local/share/forgekit/<tool>/backups`, then replace it with `contents`.
/// Returns the backup path (`None` when the file didn't exist yet).
pub fn replace_file(tool: &str, target: &Path, contents: &str) -> anyhow::Result<Option<PathBuf>> {
    let backup = if target.exists() { Some(backup(tool, target)?) } else { None };
    if let Some(dir) = target.parent()
        && !dir.exists()
    {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::write(target, contents).is_ok() {
        return Ok(backup);
    }
    let staged = crate::paths::data_dir(tool).join("staged.new");
    std::fs::create_dir_all(staged.parent().unwrap())?;
    std::fs::write(&staged, contents)?;
    let status = Command::new("pkexec")
        .args(["install", "-m", "644", "-o", "root", "-g", "root", "--"])
        .arg(&staged)
        .arg(target)
        .stdin(Stdio::null())
        .status()
        .context("running pkexec")?;
    let _ = std::fs::remove_file(&staged);
    if !status.success() {
        bail!("Couldn't save {} (cancelled or not permitted)", target.display());
    }
    Ok(backup)
}

fn backup(tool: &str, target: &Path) -> anyhow::Result<PathBuf> {
    let dir = backup_dir(tool);
    std::fs::create_dir_all(&dir)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    let name = target.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let dest = dir.join(format!("{name}.{stamp}"));
    std::fs::copy(target, &dest).with_context(|| format!("backing up {}", target.display()))?;
    Ok(dest)
}

pub fn backup_dir(tool: &str) -> PathBuf {
    crate::paths::data_dir(tool).join("backups")
}

/// Oldest first.
pub fn backups(tool: &str) -> Vec<PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(backup_dir(tool)).into_iter().flatten().flatten().map(|e| e.path()).collect();
    v.sort();
    v
}
