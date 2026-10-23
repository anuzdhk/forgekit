//! DropShelf's items, saved so the shelf survives restarts and so
//! `forgekit shelf add FILE` can add to a shelf that's already open.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Item {
    /// A file or folder. `copied` means it lives in the shelf's own folder.
    File {
        path: PathBuf,
        copied: bool,
    },
    Text {
        text: String,
    },
    Link {
        url: String,
    },
}

impl Item {
    pub fn label(&self) -> String {
        match self {
            Item::File { path, .. } => {
                path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
            }
            Item::Text { text } => {
                let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
                let mut s: String = one_line.chars().take(60).collect();
                if one_line.chars().count() > 60 {
                    s.push('…');
                }
                s
            }
            Item::Link { url } => url.clone(),
        }
    }

    /// Files that were moved or deleted after being shelved.
    pub fn is_missing(&self) -> bool {
        matches!(self, Item::File { path, .. } if !path.exists())
    }

    /// Text that looks like a URL becomes a link.
    pub fn from_text(text: &str) -> Item {
        let t = text.trim();
        let is_url = !t.contains(char::is_whitespace)
            && ["http://", "https://", "ftp://", "mailto:"].iter().any(|p| t.starts_with(p));
        if is_url { Item::Link { url: t.to_string() } } else { Item::Text { text: text.to_string() } }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shelf {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Copy dropped files into the shelf (for temporary downloads) instead of keeping references.
    pub copy_files: bool,
}

impl Settings {
    pub fn load() -> anyhow::Result<Self> {
        crate::settings::load("shelf")
    }

    pub fn save(&self) -> anyhow::Result<()> {
        crate::settings::save("shelf", self)
    }
}

pub fn store_dir() -> PathBuf {
    crate::paths::data_dir("shelf")
}

fn state_file() -> PathBuf {
    store_dir().join("shelf.json")
}

impl Shelf {
    pub fn load() -> Self {
        std::fs::read_to_string(state_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(store_dir())?;
        let tmp = state_file().with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(tmp, state_file())?;
        Ok(())
    }

    /// Add files, skipping ones already on the shelf. With `copy`, files are
    /// copied into the shelf folder first (folders are kept as references).
    pub fn add_files(&mut self, paths: &[PathBuf], copy: bool) -> anyhow::Result<usize> {
        let mut added = 0;
        for p in paths {
            let p = p.canonicalize().unwrap_or_else(|_| p.clone());
            let item = if copy && p.is_file() {
                Item::File { path: copy_in(&p)?, copied: true }
            } else {
                Item::File { path: p, copied: false }
            };
            if !self.items.contains(&item) {
                self.items.push(item);
                added += 1;
            }
        }
        Ok(added)
    }

    pub fn add(&mut self, item: Item) {
        if !self.items.contains(&item) {
            self.items.push(item);
        }
    }

    /// Remove an item; copied files are deleted from the shelf folder.
    pub fn remove(&mut self, index: usize) -> Option<Item> {
        if index >= self.items.len() {
            return None;
        }
        let item = self.items.remove(index);
        if let Item::File { path, copied: true } = &item
            && path.starts_with(store_dir())
        {
            let _ = std::fs::remove_file(path);
        }
        Some(item)
    }

    pub fn clear(&mut self) {
        while !self.items.is_empty() {
            self.remove(0);
        }
    }
}

fn copy_in(src: &Path) -> anyhow::Result<PathBuf> {
    let dir = store_dir().join("files");
    std::fs::create_dir_all(&dir)?;
    let name = src.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let mut dest = dir.join(&name);
    let mut n = 2;
    while dest.exists() {
        dest = dir.join(format!("{n} {name}"));
        n += 1;
    }
    std::fs::copy(src, &dest)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items() {
        assert_eq!(Item::from_text(" https://example.com/a "), Item::Link { url: "https://example.com/a".into() });
        assert!(matches!(Item::from_text("hello there"), Item::Text { .. }));
        let long = Item::Text { text: "word ".repeat(30) };
        assert!(long.label().ends_with('…'));
        assert!(Item::File { path: "/nope/gone".into(), copied: false }.is_missing());

        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.txt");
        std::fs::write(&f, "x").unwrap();
        let mut s = Shelf::default();
        assert_eq!(s.add_files(&[f.clone(), f.clone()], false).unwrap(), 1);
        assert_eq!(s.items[0].label(), "a.txt");
        s.remove(0);
        assert!(s.items.is_empty() && f.exists());
    }
}
