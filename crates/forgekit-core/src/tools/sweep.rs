//! Sweep: find space that's safe to reclaim, show it, clean what the user
//! picks. Root-only cleanups run together under one `pkexec` prompt.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    PackageCache,
    UnusedFlatpaks,
    OldSnaps,
    SystemLogs,
    Thumbnails,
    AppCaches,
    Trash,
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub kind: Kind,
    pub title: String,
    pub detail: String,
    /// Bytes that cleaning frees, when known.
    pub size: Option<u64>,
    /// Pre-ticked in the app: always safe and regenerated as needed.
    pub recommended: bool,
    /// Needs administrator rights.
    pub root: bool,
}

pub fn dir_size(p: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(p) else { return 0 };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| dir_size(&e.path())).sum()
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

fn cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(|| home().join(".cache"))
}

fn trash_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_default().join("Trash")
}

fn package_manager() -> Option<(&'static str, &'static str, PathBuf)> {
    let which = crate::paths::which;
    if which("apt-get").is_some() {
        Some(("apt", "apt-get clean", "/var/cache/apt/archives".into()))
    } else if which("dnf").is_some() {
        Some(("dnf", "dnf clean packages", "/var/cache/dnf".into()))
    } else if which("pacman").is_some() {
        Some(("pacman", "pacman -Sc --noconfirm", "/var/cache/pacman/pkg".into()))
    } else if which("zypper").is_some() {
        Some(("zypper", "zypper clean --all", "/var/cache/zypp/packages".into()))
    } else {
        None
    }
}

/// Disabled (older) snap revisions: (name, revision).
fn old_snaps() -> Vec<(String, String)> {
    let Ok(out) = Command::new("snap").args(["list", "--all"]).output() else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .skip(1)
        .filter(|l| l.contains("disabled"))
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let (name, rev) = (f.first()?, f.get(2)?);
            let safe = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
            (safe(name) && safe(rev)).then(|| (name.to_string(), rev.to_string()))
        })
        .collect()
}

fn journal_size() -> Option<u64> {
    let out = Command::new("journalctl").arg("--disk-usage").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    // "Archived and active journals take up 382.1M in the file system."
    let word = text.split_whitespace().find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    let (num, unit) = word.split_at(word.find(|c: char| c.is_ascii_alphabetic())?);
    let n: f64 = num.parse().ok()?;
    let mul = match unit.chars().next()? {
        'K' => 1e3,
        'M' => 1e6,
        'G' => 1e9,
        'T' => 1e12,
        _ => 1.0,
    };
    Some((n * mul) as u64)
}

/// What can be cleaned on this system, with sizes.
pub fn scan() -> Vec<Item> {
    let mut items = Vec::new();
    if let Some((name, _, dir)) = package_manager() {
        items.push(Item {
            kind: Kind::PackageCache,
            title: "Downloaded packages".into(),
            detail: format!("Installer files {name} keeps after updating"),
            size: Some(dir_size(&dir)),
            recommended: true,
            root: true,
        });
    }
    if crate::paths::which("flatpak").is_some() {
        items.push(Item {
            kind: Kind::UnusedFlatpaks,
            title: "Unused Flatpak runtimes".into(),
            detail: "Runtimes no installed app needs anymore".into(),
            size: None,
            recommended: true,
            root: false,
        });
    }
    let snaps = old_snaps();
    if !snaps.is_empty() {
        items.push(Item {
            kind: Kind::OldSnaps,
            title: "Old Snap versions".into(),
            detail: format!("{} disabled revision(s) kept after updates", snaps.len()),
            size: Some(
                snaps
                    .iter()
                    .map(|(n, r)| dir_size(&Path::new("/var/lib/snapd/snaps").join(format!("{n}_{r}.snap"))))
                    .sum(),
            ),
            recommended: true,
            root: true,
        });
    }
    if let Some(size) = journal_size() {
        items.push(Item {
            kind: Kind::SystemLogs,
            title: "Old system logs".into(),
            detail: "Keeps the last two weeks".into(),
            size: Some(size),
            recommended: false,
            root: true,
        });
    }
    let thumbs = cache_dir().join("thumbnails");
    items.push(Item {
        kind: Kind::Thumbnails,
        title: "Thumbnails".into(),
        detail: "Previews of pictures and videos; made again when needed".into(),
        size: Some(dir_size(&thumbs)),
        recommended: true,
        root: false,
    });
    let caches = dir_size(&cache_dir()).saturating_sub(dir_size(&thumbs));
    items.push(Item {
        kind: Kind::AppCaches,
        title: "App caches".into(),
        detail: "~/.cache. Apps rebuild these, but may be slower to start the first time".into(),
        size: Some(caches),
        recommended: false,
        root: false,
    });
    items.push(Item {
        kind: Kind::Trash,
        title: "Trash".into(),
        detail: "Deleted files. Emptying it can't be undone".into(),
        size: Some(dir_size(&trash_dir())),
        recommended: false,
        root: false,
    });
    items
}

/// Delete the contents of a directory, keeping the directory itself.
fn empty_dir(dir: &Path, keep: &[&Path]) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if keep.iter().any(|k| p == *k) {
            continue;
        }
        let _ = if p.is_dir() && !p.is_symlink() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
    }
}

/// Clean the chosen kinds. Root work happens in a single `pkexec` call.
/// Returns a line per finished step.
pub fn clean(kinds: &[Kind]) -> anyhow::Result<Vec<String>> {
    let mut done = Vec::new();
    let mut root_cmds: Vec<String> = Vec::new();
    for k in kinds {
        match k {
            Kind::PackageCache => {
                if let Some((_, cmd, _)) = package_manager() {
                    root_cmds.push(cmd.to_string());
                }
            }
            Kind::OldSnaps => {
                for (n, r) in old_snaps() {
                    root_cmds.push(format!("snap remove {n} --revision={r}"));
                }
            }
            Kind::SystemLogs => root_cmds.push("journalctl --vacuum-time=2weeks".into()),
            Kind::UnusedFlatpaks => {
                // Flatpak asks for permission itself (polkit) for system-wide runtimes.
                let ok = Command::new("flatpak")
                    .args(["uninstall", "--unused", "--noninteractive", "-y"])
                    .status()?
                    .success();
                done.push(if ok {
                    "Removed unused Flatpak runtimes".into()
                } else {
                    "Flatpak cleanup didn't finish".into()
                });
            }
            Kind::Thumbnails => {
                empty_dir(&cache_dir().join("thumbnails"), &[]);
                done.push("Cleared thumbnails".into());
            }
            Kind::AppCaches => {
                let thumbs = cache_dir().join("thumbnails");
                // Keep Forgekit's own cache folder out of it as well.
                let ours = cache_dir().join("forgekit");
                empty_dir(&cache_dir(), &[&thumbs, &ours]);
                done.push("Cleared app caches".into());
            }
            Kind::Trash => {
                empty_dir(&trash_dir().join("files"), &[]);
                empty_dir(&trash_dir().join("info"), &[]);
                done.push("Emptied the trash".into());
            }
        }
    }
    if !root_cmds.is_empty() {
        crate::privileged::check_can_elevate()?;
        let mut cmd = Command::new("pkexec");
        cmd.args(["sh", "-c", &root_cmds.join(" ; ")]);
        crate::privileged::pkexec(cmd).context("system cleanup")?;
        done.push("Cleaned system caches and logs".into());
    }
    if done.is_empty() {
        bail!("nothing selected");
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_emptying() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("a/b")).unwrap();
        std::fs::write(d.path().join("a/b/f"), [0u8; 100]).unwrap();
        std::fs::write(d.path().join("keep"), [0u8; 10]).unwrap();
        assert_eq!(dir_size(d.path()), 110);
        empty_dir(d.path(), &[&d.path().join("keep")]);
        assert_eq!(dir_size(d.path()), 10);
        assert!(d.path().exists());
    }
}
