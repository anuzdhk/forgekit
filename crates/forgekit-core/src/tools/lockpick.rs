//! Lockpick: find processes holding a file or folder open, via `/proc`.

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Hold {
    /// An open file descriptor.
    Open,
    /// The process's working directory.
    Cwd,
    /// A memory-mapped file (libraries, mmapped data).
    Mapped,
    /// The process's executable.
    Exe,
}

#[derive(Debug, Clone, Serialize)]
pub struct Holder {
    pub pid: u32,
    pub name: String,
    pub hold: Hold,
    pub path: PathBuf,
}

/// Every process (that we're allowed to inspect) holding `target` or anything under it.
pub fn holders(target: &Path) -> std::io::Result<Vec<Holder>> {
    let target = target.canonicalize()?;
    let me = std::process::id();
    let mut out = Vec::new();

    for entry in std::fs::read_dir("/proc")?.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if pid == me {
            continue;
        }
        let proc = entry.path();
        let name = std::fs::read_to_string(proc.join("comm")).map(|s| s.trim().to_string()).unwrap_or_default();
        let mut push = |hold, path: PathBuf| {
            if path.starts_with(&target) && !out.iter().any(|h: &Holder| h.pid == pid && h.path == path) {
                out.push(Holder { pid, name: name.clone(), hold, path });
            }
        };

        if let Ok(p) = std::fs::read_link(proc.join("cwd")) {
            push(Hold::Cwd, p);
        }
        if let Ok(p) = std::fs::read_link(proc.join("exe")) {
            push(Hold::Exe, p);
        }
        if let Ok(fds) = std::fs::read_dir(proc.join("fd")) {
            for fd in fds.flatten() {
                if let Ok(p) = std::fs::read_link(fd.path()) {
                    push(Hold::Open, p);
                }
            }
        }
        if let Ok(maps) = std::fs::read_to_string(proc.join("maps")) {
            for line in maps.lines() {
                // address perms offset dev inode path
                if let Some(p) = line.splitn(6, ' ').nth(5).map(str::trim_start).filter(|p| p.starts_with('/')) {
                    push(Hold::Mapped, PathBuf::from(p.trim_end_matches(" (deleted)")));
                }
            }
        }
    }
    out.sort_by(|a, b| (a.pid, &a.path).cmp(&(b.pid, &b.path)));
    Ok(out)
}

/// Ask a process to quit (SIGTERM), or force it (SIGKILL).
pub fn close(pid: u32, force: bool) -> std::io::Result<()> {
    let sig = if force { libc::SIGKILL } else { libc::SIGTERM };
    // SAFETY: kill(2) has no memory-safety preconditions.
    if unsafe { libc::kill(pid as libc::pid_t, sig) } == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_child_holding_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("held.txt");
        std::fs::write(&file, "x").unwrap();
        let mut child =
            std::process::Command::new("sleep").arg("5").stdin(std::fs::File::open(&file).unwrap()).spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let found = holders(dir.path()).unwrap();
        close(child.id(), false).unwrap();
        child.wait().unwrap();
        assert!(found.iter().any(|h| h.pid == child.id() && h.hold == Hold::Open), "{found:?}");
    }
}
