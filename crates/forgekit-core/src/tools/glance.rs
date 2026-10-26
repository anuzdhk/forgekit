//! Glance: decide how to preview a file. The app does the rendering.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

/// The D-Bus name GNOME Files sends Space to (GNOME Sushi's). Only the
/// interface carries the "2".
pub const PREVIEWER_NAME: &str = "org.gnome.NautilusPreviewer";
pub const PREVIEWER_IFACE: &str = "org.gnome.NautilusPreviewer2";
pub const PREVIEWER_PATH: &str = "/org/gnome/NautilusPreviewer";

fn previewer_service() -> PathBuf {
    dirs::data_dir().unwrap_or_default().join("dbus-1/services").join(format!("{PREVIEWER_NAME}.service"))
}

/// GNOME Sushi already answers Space in Files; Forgekit then stays out of the way.
pub fn sushi_installed() -> bool {
    ["/usr/share/dbus-1/services", "/usr/local/share/dbus-1/services"]
        .iter()
        .any(|d| Path::new(d).join(format!("{PREVIEWER_NAME}.service")).exists())
}

/// Let D-Bus start `app --previewer` when Files asks for a preview, or stop
/// that when `app` is `None` (Glance switched off).
pub fn set_previewer(app: Option<&str>) -> anyhow::Result<()> {
    // v0.7 wrote its file under the interface name, which Files never asks for.
    let _ = std::fs::remove_file(previewer_service().with_file_name("org.gnome.NautilusPreviewer2.service"));
    let file = previewer_service();
    match app {
        Some(app) if !sushi_installed() => {
            let text =
                format!("[D-BUS Service]\nName={PREVIEWER_NAME}\nExec={} --previewer\n", crate::shortcuts::quote(app));
            if std::fs::read_to_string(&file).ok().as_deref() != Some(text.as_str()) {
                std::fs::create_dir_all(file.parent().unwrap())?;
                std::fs::write(&file, text)?;
            }
        }
        _ => {
            if file.exists() {
                std::fs::remove_file(&file)?;
            }
        }
    }
    Ok(())
}

pub fn previewer_installed() -> bool {
    previewer_service().exists()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Image,
    Video,
    Audio,
    Text,
    Pdf,
    Archive,
    Folder,
    /// Anything else: show details and "Open with…".
    Other,
}

const TEXT_EXT: &[&str] = &[
    "txt",
    "md",
    "markdown",
    "rst",
    "log",
    "csv",
    "tsv",
    "json",
    "jsonc",
    "toml",
    "yaml",
    "yml",
    "ini",
    "conf",
    "cfg",
    "xml",
    "html",
    "htm",
    "css",
    "scss",
    "js",
    "mjs",
    "ts",
    "tsx",
    "jsx",
    "rs",
    "py",
    "rb",
    "go",
    "c",
    "h",
    "cpp",
    "hpp",
    "cc",
    "java",
    "kt",
    "swift",
    "sh",
    "bash",
    "zsh",
    "fish",
    "lua",
    "php",
    "pl",
    "sql",
    "desktop",
    "service",
    "nix",
    "vim",
    "diff",
    "patch",
    "env",
    "gitignore",
    "dockerfile",
    "makefile",
    "cmake",
    "svg",
];

pub fn kind(path: &Path) -> Kind {
    if path.is_dir() {
        return Kind::Folder;
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico" | "avif" | "heic" => Kind::Image,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" | "ogv" => Kind::Video,
        "mp3" | "flac" | "ogg" | "oga" | "opus" | "wav" | "m4a" | "aac" => Kind::Audio,
        "pdf" => Kind::Pdf,
        "zip" | "tar" | "gz" | "tgz" | "xz" | "txz" | "bz2" | "zst" | "7z" | "rar" | "deb" | "rpm" | "jar" => {
            Kind::Archive
        }
        e if TEXT_EXT.contains(&e) => Kind::Text,
        _ if looks_like_text(path) => Kind::Text,
        _ => Kind::Other,
    }
}

/// Valid UTF-8 with no NUL bytes in the first 8 KiB.
fn looks_like_text(path: &Path) -> bool {
    let mut buf = vec![0; 8192];
    let Ok(n) = std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)) else { return false };
    buf.truncate(n);
    if buf.contains(&0) {
        return false;
    }
    match std::str::from_utf8(&buf) {
        Ok(_) => true,
        // A multi-byte character cut at the buffer edge is still text.
        Err(e) => e.error_len().is_none(),
    }
}

/// The first `max` bytes as text.
pub fn read_text(path: &Path, max: usize) -> std::io::Result<(String, bool)> {
    let f = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    f.take(max as u64 + 1).read_to_end(&mut buf)?;
    let truncated = buf.len() > max;
    buf.truncate(max);
    Ok((String::from_utf8_lossy(&buf).into_owned(), truncated))
}

/// Archive contents, via the usual command-line tools.
pub fn list_archive(path: &Path) -> anyhow::Result<Vec<String>> {
    let name = path.to_string_lossy().to_ascii_lowercase();
    let out = if name.ends_with(".zip") || name.ends_with(".jar") {
        Command::new("unzip").arg("-Z1").arg(path).output()
    } else if name.ends_with(".deb") {
        Command::new("dpkg-deb").arg("-c").arg(path).output()
    } else if name.ends_with(".rpm") {
        Command::new("rpm").args(["-qlp"]).arg(path).output()
    } else if name.ends_with(".7z") {
        Command::new("7z").arg("l").arg(path).output()
    } else {
        Command::new("tar").arg("-tf").arg(path).output()
    };
    let out = out.map_err(|e| anyhow::anyhow!("no tool to list this archive ({e})"))?;
    anyhow::ensure!(out.status.success(), "couldn't list archive: {}", String::from_utf8_lossy(&out.stderr).trim());
    Ok(String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect())
}

/// Folder entries, folders first.
pub fn list_folder(path: &Path, max: usize) -> std::io::Result<Vec<(String, bool)>> {
    let mut v: Vec<(String, bool)> = std::fs::read_dir(path)?
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path().is_dir()))
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    v.truncate(max);
    Ok(v)
}

/// "1.4 MB"
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 { format!("{bytes} B") } else { format!("{v:.1} {}", UNITS[i]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        let d = tempfile::tempdir().unwrap();
        let w = |name: &str, data: &[u8]| {
            let p = d.path().join(name);
            std::fs::write(&p, data).unwrap();
            p
        };
        assert_eq!(kind(d.path()), Kind::Folder);
        assert_eq!(kind(&w("a.PNG", b"x")), Kind::Image);
        assert_eq!(kind(&w("Makefile", b"all:\n\techo hi\n")), Kind::Text);
        assert_eq!(kind(&w("blob", &[0, 1, 2, 3])), Kind::Other);
        assert_eq!(kind(&w("x.tar.gz", b"")), Kind::Archive);
        let (t, cut) = read_text(&w("long.txt", b"abcdef"), 4).unwrap();
        assert_eq!((t.as_str(), cut), ("abcd", true));
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1_400_000), "1.4 MB");
    }
}
