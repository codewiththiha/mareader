//! One pack's sqlite, asked the ways a menu asks.

use std::path::Path;

use anyhow::{Context, Result};
use dict_core::fold;
use rusqlite::{Connection, params};

/// A row as the builder carried it.
#[derive(Debug, Clone)]
pub struct RawRow {
    pub word: String,
    pub pos: Option<String>,
    pub definition: String,
    pub romanization: Option<String>,
    pub sense: Option<String>,
    pub lang_code: Option<String>,
    pub source: Option<String>,
}

const COLUMNS: &str = "word, pos, definition, romanization, sense, lang_code, source";

/// A pack's open database.
pub struct DictDb {
    conn: Connection,
}

impl DictDb {
    /// Open the sqlite a finished build landed.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
        Ok(Self { conn })
    }

    /// The headwords that fit `ask`: exact, prefix, substring,
    /// then near-miss spellings.
    pub fn lookup_word(&self, ask: &str, limit: usize) -> Result<Vec<RawRow>> {
        let ask_fold = fold(ask);
        let mut out: Vec<RawRow> = Vec::new();
        self.collect(
            "SELECT {COLUMNS} FROM entries WHERE word_fold = ?1",
            params![ask_fold],
            limit,
            &mut out,
        )?;
        self.collect(
            "SELECT {COLUMNS} FROM entries
             WHERE word_fold LIKE ?1 ESCAPE '\\' AND word_fold != ?2",
            params![like_prefix(&ask_fold), ask_fold],
            limit,
            &mut out,
        )?;
        self.collect(
            "SELECT {COLUMNS} FROM entries
             WHERE word_fold LIKE ?1 ESCAPE '\\' AND word_fold NOT LIKE ?2 ESCAPE '\\'",
            params![like_contains(&ask_fold), like_prefix(&ask_fold)],
            limit,
            &mut out,
        )?;
        self.fuzzy(&ask_fold, limit, &mut out)?;
        Ok(out)
    }

    /// The rows whose other-language side is `ask`: the way home.
    pub fn lookup_definition(&self, ask: &str, limit: usize) -> Result<Vec<RawRow>> {
        let ask_fold = fold(ask);
        let mut out: Vec<RawRow> = Vec::new();
        self.collect(
            "SELECT {COLUMNS} FROM entries WHERE def_fold = ?1",
            params![ask_fold],
            limit,
            &mut out,
        )?;
        self.collect(
            "SELECT {COLUMNS} FROM entries
             WHERE def_fold LIKE ?1 ESCAPE '\\' AND def_fold != ?2",
            params![like_prefix(&ask_fold), ask_fold],
            limit,
            &mut out,
        )?;
        Ok(out)
    }

    /// The rows a search page lists: the ask on either
    /// side of the row.
    pub fn search(&self, ask: &str, limit: usize) -> Result<Vec<RawRow>> {
        let ask_fold = fold(ask);
        let mut out: Vec<RawRow> = Vec::new();
        self.collect(
            "SELECT {COLUMNS} FROM entries
             WHERE word_fold = ?1 OR def_fold = ?1",
            params![ask_fold],
            limit,
            &mut out,
        )?;
        self.collect(
            "SELECT {COLUMNS} FROM entries
             WHERE word_fold LIKE ?1 ESCAPE '\\'
                OR def_fold LIKE ?1 ESCAPE '\\'",
            params![like_prefix(&ask_fold)],
            limit,
            &mut out,
        )?;
        self.collect(
            "SELECT {COLUMNS} FROM entries
             WHERE word_fold LIKE ?1 ESCAPE '\\'
                OR def_fold LIKE ?1 ESCAPE '\\'",
            params![like_contains(&ask_fold)],
            limit,
            &mut out,
        )?;
        self.fuzzy(&ask_fold, limit, &mut out)?;
        Ok(out)
    }

    /// A bridge's next hop asks the word side for several
    /// intermediates at once.
    pub fn lookup_words_any(&self, asks: &[String], limit: usize) -> Result<Vec<RawRow>> {
        let mut out: Vec<RawRow> = Vec::new();
        if asks.is_empty() {
            return Ok(out);
        }
        let folds: Vec<String> = asks.iter().map(|ask| fold(ask)).collect();
        let mut marks = String::from("?1");
        for i in 2..=folds.len() {
            marks.push_str(&format!(", ?{i}"));
        }
        let sql = format!("SELECT {COLUMNS} FROM entries WHERE word_fold IN ({marks})");
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(folds.iter()), row_shape)?;
        for row in rows {
            if out.len() >= limit {
                break;
            }
            out.push(row?);
        }
        Ok(out)
    }

    /// A pass over the ask's first letter for near-miss spellings.
    fn fuzzy(&self, ask_fold: &str, limit: usize, out: &mut Vec<RawRow>) -> Result<()> {
        let Some(first) = ask_fold.chars().next() else {
            return Ok(());
        };
        let sql = "SELECT {COLUMNS} FROM entries
             WHERE word_fold LIKE ?1 ESCAPE '\\'
             LIMIT 500"
            .replace("{COLUMNS}", COLUMNS);
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![like_prefix(&first.to_string())], row_shape)?;
        for row in rows {
            let row = row?;
            if out.len() >= limit {
                break;
            }
            let distance = dict_core::edit_distance(ask_fold, &fold(&row.word), 2);
            if distance <= 2 && !out.iter().any(|seen| seen.word == row.word) {
                out.push(row);
            }
        }
        Ok(())
    }

    /// Run one select, appending rows until the limit.
    fn collect(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
        limit: usize,
        out: &mut Vec<RawRow>,
    ) -> Result<()> {
        let sql = sql.replace("{COLUMNS}", COLUMNS);
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params, row_shape)?;
        for row in rows {
            if out.len() >= limit {
                break;
            }
            out.push(row?);
        }
        Ok(())
    }
}

/// The row shape every select above carries.
fn row_shape(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
    Ok(RawRow {
        word: row.get(0)?,
        pos: row.get(1)?,
        definition: row.get(2)?,
        romanization: row.get(3)?,
        sense: row.get(4)?,
        lang_code: row.get(5)?,
        source: row.get(6)?,
    })
}

/// A LIKE pattern for `ask` starting a word, wildcards escaped.
fn like_prefix(ask: &str) -> String {
    format!("{}%", like_escape(ask))
}

/// A LIKE pattern for `ask` inside a word.
fn like_contains(ask: &str) -> String {
    format!("%{}%", like_escape(ask))
}

/// The three LIKE metacharacters, backslashed.
fn like_escape(ask: &str) -> String {
    ask.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wildcard_in_the_ask_stays_data() {
        assert_eq!(like_escape("100%"), "100\\%");
        assert_eq!(like_escape("a_b"), "a\\_b");
        assert_eq!(like_escape("c\\d"), "c\\\\d");
        assert_eq!(like_prefix("ca"), "ca%");
        assert_eq!(like_contains("ca"), "%ca%");
    }
}
