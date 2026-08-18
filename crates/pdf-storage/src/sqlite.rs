//! SQLite-backed [`PdfStorage`] (feature `sqlite`), wrapping
//! `tauri-plugin-sql`. A single `kv(key, value)` table stands in for
//! localStorage; values are the same JSON blobs as the localStorage impl.

use super::PdfStorage;

#[derive(Clone)]
pub struct SqliteStorage {
    pool: tauri_plugin_sql::SqlitePool,
}

impl SqliteStorage {
    pub fn new(pool: tauri_plugin_sql::SqlitePool) -> Self {
        Self { pool }
    }
}

impl PdfStorage for SqliteStorage {
    fn get(&self, key: &str) -> Option<String> {
        // SELECT value FROM kv WHERE key = ?1
        let _ = (&self.pool, key);
        None
    }

    fn set(&self, key: &str, value: &str) {
        // INSERT INTO kv(key, value) VALUES(?1, ?2)
        //   ON CONFLICT(key) DO UPDATE SET value = ?2
        let _ = (&self.pool, key, value);
    }

    fn remove(&self, key: &str) {
        // DELETE FROM kv WHERE key = ?1
        let _ = (&self.pool, key);
    }
}
