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
    check_can_elevate()?;
    let staged = crate::paths::data_dir(tool).join("staged.new");
    std::fs::create_dir_all(staged.parent().unwrap())?;
    std::fs::write(&staged, contents)?;
    let mut cmd = Command::new("pkexec");
    cmd.args(["install", "-m", "644", "-o", "root", "-g", "root", "--"]).arg(&staged).arg(target).stdin(Stdio::null());
    let r = pkexec(cmd);
    let _ = std::fs::remove_file(&staged);
    r.with_context(|| format!("Couldn't save {}", target.display()))?;
    Ok(backup)
}

const SANDBOXED: &str = "this terminal can't ask for administrator rights (it runs inside a sandbox). \
     Run the same command from a regular terminal, e.g. Ctrl+Alt+T";

/// Run a `pkexec` command, turning its failures into plain explanations.
/// The password prompt comes from the desktop's polkit agent, so stderr can be captured.
pub fn pkexec(mut cmd: Command) -> anyhow::Result<()> {
    let out = cmd.stdout(Stdio::inherit()).stderr(Stdio::piped()).output().context("running pkexec")?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    // pkexec's own exit codes: 126 = dismissed/not authorized, 127 = no agent or other error.
    if err.contains("must be setuid root") || err.contains("no new privileges") {
        bail!(SANDBOXED);
    }
    match out.status.code() {
        Some(126) => bail!("cancelled or not permitted"),
        Some(127) if err.contains("authentication agent") => {
            bail!("no password dialog is available here; run it from a terminal in your desktop session")
        }
        _ => bail!("{}", err.trim().lines().last().unwrap_or("failed")),
    }
}

/// pkexec and sudo can't work in a process started with the kernel's
/// "no new privileges" flag (sandboxes, some IDE and AI-tool terminals).
/// Say so plainly instead of reporting a vague failure.
pub fn check_can_elevate() -> anyhow::Result<()> {
    let flagged = std::fs::read_to_string("/proc/self/status")
        .is_ok_and(|s| s.lines().any(|l| l.starts_with("NoNewPrivs:") && l.trim_end().ends_with('1')));
    if flagged {
        bail!(SANDBOXED);
    }
    Ok(())
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
