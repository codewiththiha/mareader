//! The AI dictionary on disk: `ai.sqlite` beside the packs.
//!
//! Made on first use and opened by the lookup worker only.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use crate::ai::{Saved, class};
use crate::lang::code_of;
use crate::search::fold;
use crate::store::escape;

const FILE: &str = "ai.sqlite";
const LAYOUT: &str = "1";
/// Joins a list in one column; no synonym or example holds it.
const SEP: char = '\u{1f}';

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS saved (
    word TEXT NOT NULL,
    word_fold TEXT NOT NULL,
    pos TEXT NOT NULL,
    lang TEXT NOT NULL,
    meaning TEXT NOT NULL,
    synonyms TEXT NOT NULL,
    usages TEXT NOT NULL,
    model TEXT NOT NULL,
    added INTEGER NOT NULL,
    PRIMARY KEY (word_fold, pos, lang)
);";

/// The file beside the packs in `dir`.
pub fn file(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// The languages kept in `dir`'s AI dictionary; none before it is made.
pub fn langs_in(dir: &Path) -> Vec<String> {
    let path = file(dir);
    let open = path.is_file().then(|| AiStore::open(&path).ok()).flatten();
    open.and_then(|ai| ai.langs().ok()).unwrap_or_default()
}

pub struct AiStore {
    conn: Connection,
}

impl AiStore {
    /// Opens the file, making it and its table when missing.
    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        let sql = "INSERT OR IGNORE INTO meta (key, value) VALUES ('layout', ?1)";
        conn.execute(sql, [LAYOUT]).map_err(|e| e.to_string())?;
        let sql = "SELECT value FROM meta WHERE key = 'layout'";
        let layout: String = conn
            .query_row(sql, [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if layout != LAYOUT {
            return Err("an AI dictionary in another layout".into());
        }
        let store = Self { conn };
        store.refile()?;
        Ok(store)
    }

    /// Moves meanings kept under a language's name, as `russian` or
    /// `chinese (simplified)` were, to its code; one whose word and class
    /// the code already keeps goes.
    fn refile(&self) -> Result<(), String> {
        let up = "UPDATE OR IGNORE saved SET lang = ?1 WHERE lang = ?2";
        let gone = "DELETE FROM saved WHERE lang = ?1";
        for old in self.langs()? {
            let code = code_of(&old);
            if code != old {
                let moved = self.conn.execute(up, [&code, &old]);
                moved.map_err(|e| e.to_string())?;
                let left = self.conn.execute(gone, [&old]);
                left.map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    /// Keeps `saved`; one already under its key stays unless `replace`.
    /// True when this one was written.
    pub fn put(&self, saved: &Saved, replace: bool) -> Result<bool, String> {
        let verb = if replace { "REPLACE" } else { "IGNORE" };
        let sql = format!(
            "INSERT OR {verb} INTO saved (word, word_fold, pos, lang, meaning, synonyms, \
             usages, model, added) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
        );
        let (word_fold, pos, lang) = saved.key();
        let added = i64::try_from(saved.added).unwrap_or(i64::MAX);
        let (synonyms, usages) = (join(&saved.synonyms), join(&saved.usages));
        let word = saved.word.trim();
        let row = params![
            word,
            word_fold,
            pos,
            lang,
            saved.meaning,
            synonyms,
            usages,
            saved.model,
            added
        ];
        let n = self.conn.execute(&sql, row).map_err(|e| e.to_string())?;
        Ok(n > 0)
    }

    /// The meaning kept under `word`, `pos` and `lang`, if any.
    pub fn get(&self, word: &str, pos: &str, lang: &str) -> Result<Option<Saved>, String> {
        let sql = format!("{SELECT} WHERE word_fold = ?1 AND pos = ?2 AND lang = ?3");
        let key = params![fold(word), class(pos), lang];
        let found = self.conn.query_row(&sql, key, read).optional();
        found.map_err(|e| e.to_string())
    }

    /// Drops the meaning under `saved`'s key; true when one was there.
    pub fn forget(&self, saved: &Saved) -> Result<bool, String> {
        let sql = "DELETE FROM saved WHERE word_fold = ?1 AND pos = ?2 AND lang = ?3";
        let (word_fold, pos, lang) = saved.key();
        let gone = self.conn.execute(sql, params![word_fold, pos, lang]);
        gone.map(|n| n > 0).map_err(|e| e.to_string())
    }

    /// Removes selected words and classes in all languages, atomically.
    pub fn forget_words(&self, words: &[(String, Option<String>)]) -> Result<usize, String> {
        let tx = self.conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let mut count = 0;
        for (word, pos) in words {
            let n = match pos {
                Some(pos) => tx.execute(
                    "DELETE FROM saved WHERE word_fold = ?1 AND pos = ?2",
                    params![fold(word), class(pos)],
                ),
                None => tx.execute("DELETE FROM saved WHERE word_fold = ?1", [fold(word)]),
            };
            count += n.map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(count)
    }

    /// Drops every meaning kept in `lang`; how many went.
    pub fn clear(&self, lang: &str) -> Result<usize, String> {
        let sql = "DELETE FROM saved WHERE lang = ?1";
        let gone = self.conn.execute(sql, [lang]);
        gone.map_err(|e| e.to_string())
    }

    /// Puts `new` in `old`'s place. Another meaning under the new key stays
    /// unless `replace`, and comes back instead.
    pub fn edit(&self, old: &Saved, new: &Saved, replace: bool) -> Result<Option<Saved>, String> {
        if new.key() != old.key() && !replace {
            let (_, pos, lang) = new.key();
            if let Some(kept) = self.get(&new.word, &pos, &lang)? {
                return Ok(Some(kept));
            }
        }
        let tx = self.conn.unchecked_transaction();
        let tx = tx.map_err(|e| e.to_string())?;
        self.forget(old)?;
        self.put(new, true)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(None)
    }

    /// Each language a meaning is kept in, by code.
    pub fn langs(&self) -> Result<Vec<String>, String> {
        let sql = "SELECT DISTINCT lang FROM saved ORDER BY lang";
        let mut stmt = self.conn.prepare(sql).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0));
        let rows = rows.map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    /// Every class kept for `word` in `lang`, or in any, the newest first.
    pub fn word(&self, word: &str, lang: Option<&str>) -> Result<Vec<Saved>, String> {
        let only = "(?2 IS NULL OR lang = ?2)";
        let sql = format!("{SELECT} WHERE word_fold = ?1 AND {only} ORDER BY added DESC");
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![fold(word), lang], read)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    /// Up to `limit` meanings whose word, or else meaning, holds `text`.
    pub fn search(
        &self,
        text: &str,
        lang: Option<&str>,
        meaning: bool,
        limit: usize,
    ) -> Result<Vec<Saved>, String> {
        let col = if meaning { "meaning" } else { "word_fold" };
        let only = "(?2 IS NULL OR lang = ?2)";
        let sql = format!("{SELECT} WHERE {col} LIKE ?1 ESCAPE '\\' AND {only} LIMIT ?3");
        let like = format!("%{}%", escape(&fold(text)));
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![like, lang, limit], read)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }
}

const SELECT: &str = "SELECT word, pos, lang, meaning, synonyms, usages, model, added FROM saved";

fn read(r: &rusqlite::Row<'_>) -> rusqlite::Result<Saved> {
    let added: i64 = r.get(7)?;
    Ok(Saved {
        word: r.get(0)?,
        pos: r.get(1)?,
        lang: r.get(2)?,
        meaning: r.get(3)?,
        synonyms: split(&r.get::<_, String>(4)?),
        usages: split(&r.get::<_, String>(5)?),
        model: r.get(6)?,
        added: u64::try_from(added).unwrap_or(0),
    })
}

fn join(items: &[String]) -> String {
    let flat = items.iter().map(|s| s.replace(SEP, " "));
    flat.collect::<Vec<_>>().join(&SEP.to_string())
}

fn split(cell: &str) -> Vec<String> {
    let parts = cell.split(SEP).filter(|s| !s.is_empty());
    parts.map(str::to_owned).collect()
}

#[cfg(test)]
#[path = "ai_store_tests.rs"]
mod tests;
