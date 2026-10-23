//! Clipstack history in `~/.local/share/forgekit/clipstack/history.db`.
//! Several processes (daemon, app, CLI) open it at once; SQLite's WAL mode
//! and a busy timeout keep that safe.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use super::{Clip, Settings};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub id: i64,
    pub mime: String,
    /// The text, or a short description for images.
    pub preview: String,
    pub size: usize,
    pub pinned: bool,
    /// Unix milliseconds.
    pub created: i64,
    pub last_used: i64,
}

impl Item {
    pub fn is_image(&self) -> bool {
        self.mime.starts_with("image/")
    }
}

pub struct Store {
    conn: Connection,
}

pub fn default_path() -> PathBuf {
    crate::paths::data_dir("clipstack").join("history.db")
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// FNV-1a; only used to spot duplicates.
fn hash(mime: &str, data: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in mime.as_bytes().iter().chain([0u8].iter()).chain(data) {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}{:x}", data.len())
}

impl Store {
    /// A strictly increasing "used at" stamp, so recency order is exact even
    /// for two copies in the same millisecond.
    fn stamp(&self) -> anyhow::Result<i64> {
        let max: i64 = self.conn.query_row("SELECT coalesce(max(last_used), 0) FROM items", [], |r| r.get(0))?;
        Ok(now_ms().max(max + 1))
    }

    pub fn open_default() -> anyhow::Result<Self> {
        Self::open(&default_path())
    }

    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
            // History can hold anything people copy; keep it private.
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700));
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(3))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS items (
                id INTEGER PRIMARY KEY,
                mime TEXT NOT NULL,
                data BLOB NOT NULL,
                preview TEXT NOT NULL,
                hash TEXT NOT NULL UNIQUE,
                pinned INTEGER NOT NULL DEFAULT 0,
                created INTEGER NOT NULL,
                last_used INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS items_last_used ON items(last_used);",
        )?;
        Ok(Store { conn })
    }

    /// Record a clip. Copying something already in history moves it to the top.
    /// Returns the item id, or `None` if settings say to skip it.
    pub fn add(&self, clip: &Clip, settings: &Settings) -> anyhow::Result<Option<i64>> {
        if !settings.accepts(clip) {
            return Ok(None);
        }
        let h = hash(&clip.mime, &clip.data);
        let t = self.stamp()?;
        if let Some(id) =
            self.conn.query_row("SELECT id FROM items WHERE hash = ?1", [&h], |r| r.get::<_, i64>(0)).optional()?
        {
            self.conn.execute("UPDATE items SET last_used = ?1 WHERE id = ?2", params![t, id])?;
            return Ok(Some(id));
        }
        let preview = if clip.is_image() {
            match image::load_from_memory(&clip.data) {
                Ok(img) => format!("Image · {}×{}", img.width(), img.height()),
                Err(_) => "Image".to_string(),
            }
        } else {
            let s = String::from_utf8_lossy(&clip.data);
            s.chars().take(500).collect()
        };
        self.conn.execute(
            "INSERT INTO items (mime, data, preview, hash, created, last_used) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![clip.mime, clip.data, preview, h, t],
        )?;
        let id = self.conn.last_insert_rowid();
        self.prune(settings)?;
        Ok(Some(id))
    }

    /// Newest first. `query` filters text items (case-insensitive); pinned items come first.
    pub fn list(&self, query: &str, limit: usize) -> anyhow::Result<Vec<Item>> {
        let mut st = self.conn.prepare(
            "SELECT id, mime, preview, length(data), pinned, created, last_used FROM items
             WHERE (?1 = '' OR (mime NOT LIKE 'image/%' AND preview LIKE '%' || ?1 || '%' ESCAPE '\\'))
             ORDER BY pinned DESC, last_used DESC, id DESC LIMIT ?2",
        )?;
        let q = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let rows = st.query_map(params![q, limit as i64], |r| {
            Ok(Item {
                id: r.get(0)?,
                mime: r.get(1)?,
                preview: r.get(2)?,
                size: r.get::<_, i64>(3)? as usize,
                pinned: r.get::<_, i64>(4)? != 0,
                created: r.get(5)?,
                last_used: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn get(&self, id: i64) -> anyhow::Result<Option<Clip>> {
        Ok(self
            .conn
            .query_row("SELECT mime, data FROM items WHERE id = ?1", [id], |r| {
                Ok(Clip { mime: r.get(0)?, data: r.get(1)? })
            })
            .optional()?)
    }

    /// Mark an item as just used (it moves to the top).
    pub fn touch(&self, id: i64) -> anyhow::Result<()> {
        self.conn.execute("UPDATE items SET last_used = ?1 WHERE id = ?2", params![self.stamp()?, id])?;
        Ok(())
    }

    pub fn set_pinned(&self, id: i64, pinned: bool) -> anyhow::Result<bool> {
        Ok(self.conn.execute("UPDATE items SET pinned = ?1 WHERE id = ?2", params![pinned as i64, id])? > 0)
    }

    pub fn delete(&self, id: i64) -> anyhow::Result<bool> {
        Ok(self.conn.execute("DELETE FROM items WHERE id = ?1", [id])? > 0)
    }

    /// Remove everything except pinned items. Returns how many were removed.
    pub fn clear(&self) -> anyhow::Result<usize> {
        Ok(self.conn.execute("DELETE FROM items WHERE pinned = 0", [])?)
    }

    pub fn count(&self) -> anyhow::Result<usize> {
        Ok(self.conn.query_row("SELECT count(*) FROM items", [], |r| r.get::<_, i64>(0))? as usize)
    }

    /// Apply the item limit and expiry. Pinned items are never removed.
    pub fn prune(&self, s: &Settings) -> anyhow::Result<()> {
        if s.expire_days > 0 {
            let cutoff = now_ms() - s.expire_days as i64 * 86_400_000;
            self.conn.execute("DELETE FROM items WHERE pinned = 0 AND last_used < ?1", [cutoff])?;
        }
        self.conn.execute(
            "DELETE FROM items WHERE pinned = 0 AND id NOT IN
               (SELECT id FROM items WHERE pinned = 0 ORDER BY last_used DESC, id DESC LIMIT ?1)",
            [s.max_items as i64],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("h.db")).unwrap();
        (d, s)
    }

    #[test]
    fn add_dedupe_search_pin_prune() {
        let (_d, st) = store();
        let s = Settings { max_items: 2, ..Default::default() };
        let a = st.add(&Clip::text("alpha"), &s).unwrap().unwrap();
        st.add(&Clip::text("beta 100%"), &s).unwrap();
        assert_eq!(st.add(&Clip::text("alpha"), &s).unwrap(), Some(a));
        assert_eq!(st.count().unwrap(), 2);
        assert_eq!(st.list("", 10).unwrap()[0].preview, "alpha");
        assert_eq!(st.list("100%", 10).unwrap().len(), 1);
        assert_eq!(st.list("_", 10).unwrap().len(), 0);

        st.set_pinned(a, true).unwrap();
        st.add(&Clip::text("gamma"), &s).unwrap();
        st.add(&Clip::text("delta"), &s).unwrap();
        let all: Vec<_> = st.list("", 10).unwrap().into_iter().map(|i| i.preview).collect();
        assert_eq!(all, ["alpha", "delta", "gamma"]);
        assert_eq!(st.clear().unwrap(), 2);
        assert_eq!(st.get(a).unwrap().unwrap().data, b"alpha");
        assert_eq!(st.add(&Clip::text("  "), &s).unwrap(), None);
    }

    #[test]
    fn images_get_a_description() {
        let (_d, st) = store();
        let mut png = Vec::new();
        image::RgbaImage::new(3, 2).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let id = st.add(&Clip { mime: "image/png".into(), data: png }, &Settings::default()).unwrap().unwrap();
        let item = &st.list("", 1).unwrap()[0];
        assert_eq!((item.id, item.preview.as_str(), item.is_image()), (id, "Image · 3×2", true));
    }
}
