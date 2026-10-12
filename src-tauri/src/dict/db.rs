//! One pack's sqlite, asked the ways a menu asks.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use dict_core::fold;
use rusqlite::{Connection, OpenFlags, params};

/// A row's read shape. The pack id answers what `lang_code`
/// and `source` would.
#[derive(Debug, Clone)]
pub struct RawRow {
    pub word: String,
    pub pos: Option<String>,
    pub definition: String,
    pub romanization: Option<String>,
    pub sense: Option<String>,
}

const COLUMNS: &str = "id, word, pos, definition, romanization, sense";

/// Which side of the table a near-miss pass inspects.
#[derive(Clone, Copy)]
enum FuzzySide {
    Word,
    Definition,
    Both,
}

/// A pack's open database.
pub struct DictDb {
    conn: Connection,
}

impl DictDb {
    /// Open the sqlite a finished build landed.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
        let _ = conn.execute_batch(
            "PRAGMA query_only = ON;
             PRAGMA temp_store = MEMORY;
             PRAGMA cache_size = -8000;",
        );
        Ok(Self { conn })
    }

    /// The headwords that fit `ask`: exact, prefix, substring,
    /// then near-miss spellings.
    pub fn lookup_word(&self, ask: &str, limit: usize) -> Result<Vec<RawRow>> {
        let ask_fold = fold(ask);
        let mut out: Vec<RawRow> = Vec::new();
        let mut seen: HashSet<i64> = HashSet::new();
        self.collect(
            "SELECT {COLUMNS} FROM entries WHERE word_fold = ?1",
            params![ask_fold],
            limit,
            &mut seen,
            &mut out,
        )?;
        self.collect(
            "SELECT {COLUMNS} FROM entries
             WHERE word_fold LIKE ?1 ESCAPE '\\' AND word_fold != ?2",
            params![like_prefix(&ask_fold), ask_fold],
            limit,
            &mut seen,
            &mut out,
        )?;
        self.collect(
            "SELECT {COLUMNS} FROM entries
             WHERE word_fold LIKE ?1 ESCAPE '\\' AND word_fold NOT LIKE ?2 ESCAPE '\\'",
            params![like_contains(&ask_fold), like_prefix(&ask_fold)],
            limit,
            &mut seen,
            &mut out,
        )?;
        self.fuzzy(&ask_fold, FuzzySide::Word, limit, &mut seen, &mut out)?;
        Ok(out)
    }

    /// The rows whose other-language side is `ask`: the way home.
    pub fn lookup_definition(&self, ask: &str, limit: usize) -> Result<Vec<RawRow>> {
        let ask_fold = fold(ask);
        let mut out: Vec<RawRow> = Vec::new();
        let mut seen: HashSet<i64> = HashSet::new();
        self.collect(
            "SELECT {COLUMNS} FROM entries WHERE def_fold = ?1",
            params![ask_fold],
            limit,
            &mut seen,
            &mut out,
        )?;
        self.collect(
            "SELECT {COLUMNS} FROM entries
             WHERE def_fold LIKE ?1 ESCAPE '\\' AND def_fold != ?2",
            params![like_prefix(&ask_fold), ask_fold],
            limit,
            &mut seen,
            &mut out,
        )?;
        Ok(out)
    }

    /// The rows a search page lists: the ask on either
    /// side of the row.
    pub fn search(&self, ask: &str, limit: usize) -> Result<Vec<RawRow>> {
        let ask_fold = fold(ask);
        let mut out: Vec<RawRow> = Vec::new();
        let mut seen: HashSet<i64> = HashSet::new();
        self.collect(
            "SELECT {COLUMNS} FROM entries
             WHERE word_fold = ?1 OR def_fold = ?1",
            params![ask_fold],
            limit,
            &mut seen,
            &mut out,
        )?;
        self.collect(
            "SELECT {COLUMNS} FROM entries
             WHERE word_fold LIKE ?1 ESCAPE '\\'
                OR def_fold LIKE ?1 ESCAPE '\\'",
            params![like_prefix(&ask_fold)],
            limit,
            &mut seen,
            &mut out,
        )?;
        self.collect(
            "SELECT {COLUMNS} FROM entries
             WHERE word_fold LIKE ?1 ESCAPE '\\'
                OR def_fold LIKE ?1 ESCAPE '\\'",
            params![like_contains(&ask_fold)],
            limit,
            &mut seen,
            &mut out,
        )?;
        self.fuzzy(&ask_fold, FuzzySide::Both, limit, &mut seen, &mut out)?;
        Ok(out)
    }

    /// The rows whose far side fits `ask`, loosely: prefixes too.
    pub fn search_definition(&self, ask: &str, limit: usize) -> Result<Vec<RawRow>> {
        let ask_fold = fold(ask);
        let mut out: Vec<RawRow> = Vec::new();
        let mut seen: HashSet<i64> = HashSet::new();
        self.collect(
            "SELECT {COLUMNS} FROM entries WHERE def_fold = ?1",
            params![ask_fold],
            limit,
            &mut seen,
            &mut out,
        )?;
        self.collect(
            "SELECT {COLUMNS} FROM entries
             WHERE def_fold LIKE ?1 ESCAPE '\\' AND def_fold != ?2",
            params![like_prefix(&ask_fold), ask_fold],
            limit,
            &mut seen,
            &mut out,
        )?;
        self.collect(
            "SELECT {COLUMNS} FROM entries
             WHERE def_fold LIKE ?1 ESCAPE '\\' AND def_fold NOT LIKE ?2 ESCAPE '\\'",
            params![like_contains(&ask_fold), like_prefix(&ask_fold)],
            limit,
            &mut seen,
            &mut out,
        )?;
        self.fuzzy(&ask_fold, FuzzySide::Definition, limit, &mut seen, &mut out)?;
        Ok(out)
    }

    /// A bridge's next hop asks the word side for several
    /// intermediates at once.
    pub fn lookup_words_any(&self, asks: &[String], limit: usize) -> Result<Vec<RawRow>> {
        self.lookup_column_any("word_fold", asks, limit)
    }

    /// A reversed bridge's next hop asks the definition side for several
    /// intermediates at once.
    pub fn lookup_defs_any(&self, asks: &[String], limit: usize) -> Result<Vec<RawRow>> {
        self.lookup_column_any("def_fold", asks, limit)
    }

    fn lookup_column_any(
        &self,
        column: &str,
        asks: &[String],
        limit: usize,
    ) -> Result<Vec<RawRow>> {
        let mut out: Vec<RawRow> = Vec::new();
        if asks.is_empty() || limit == 0 {
            return Ok(out);
        }
        let folds: Vec<String> = asks.iter().map(|ask| fold(ask)).collect();
        let mut marks = String::from("?1");
        for i in 2..=folds.len() {
            marks.push_str(&format!(", ?{i}"));
        }
        let sql = format!("SELECT {COLUMNS} FROM entries WHERE {column} IN ({marks})");
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(folds.iter()), row_shape)?;
        let mut seen = HashSet::new();
        for row in rows {
            if out.len() >= limit {
                break;
            }
            let (id, row) = row?;
            if seen.insert(id) {
                out.push(row);
            }
        }
        Ok(out)
    }

    /// A near-miss pass over the ask's length window.
    fn fuzzy(
        &self,
        ask_fold: &str,
        side: FuzzySide,
        limit: usize,
        seen: &mut HashSet<i64>,
        out: &mut Vec<RawRow>,
    ) -> Result<()> {
        if out.len() >= limit {
            return Ok(());
        }
        let chars = ask_fold.chars().count();
        if chars < 3 {
            return Ok(());
        }
        let clause = match side {
            FuzzySide::Word => "length(word_fold) BETWEEN ?1 AND ?2",
            FuzzySide::Definition => "length(def_fold) BETWEEN ?1 AND ?2",
            FuzzySide::Both => {
                "length(word_fold) BETWEEN ?1 AND ?2 OR length(def_fold) BETWEEN ?1 AND ?2"
            }
        };
        let sql =
            format!("SELECT {COLUMNS}, word_fold, def_fold FROM entries WHERE {clause} LIMIT 2000");
        let len = chars as i64;
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![len - 2, len + 2], |row| {
            let (id, raw) = row_shape(row)?;
            let word_fold: String = row.get(6)?;
            let def_fold: String = row.get(7)?;
            Ok((id, raw, word_fold, def_fold))
        })?;
        for row in rows {
            let (id, row, word_fold, def_fold) = row?;
            if out.len() >= limit {
                break;
            }
            if seen.contains(&id) {
                continue;
            }
            let near = match side {
                FuzzySide::Word => dict_core::edit_distance(ask_fold, &word_fold, 2),
                FuzzySide::Definition => dict_core::edit_distance(ask_fold, &def_fold, 2),
                FuzzySide::Both => dict_core::edit_distance(ask_fold, &word_fold, 2)
                    .min(dict_core::edit_distance(ask_fold, &def_fold, 2)),
            };
            if near <= 2 && !out.iter().any(|prev| prev.word == row.word) {
                seen.insert(id);
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
        seen: &mut HashSet<i64>,
        out: &mut Vec<RawRow>,
    ) -> Result<()> {
        if out.len() >= limit {
            return Ok(());
        }
        let sql = sql.replace("{COLUMNS}", COLUMNS);
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params, row_shape)?;
        for row in rows {
            if out.len() >= limit {
                break;
            }
            let (id, row) = row?;
            if seen.insert(id) {
                out.push(row);
            }
        }
        Ok(())
    }
}

/// Whether a file holds a nonempty database this reader can query.
pub fn is_usable(path: &Path) -> bool {
    let Ok(conn) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return false;
    };
    let version = conn
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .unwrap_or_default();
    if version != 1
        || conn
            .prepare("SELECT word, word_fold, pos, definition, def_fold, romanization, sense, lang_code, source FROM entries LIMIT 0")
            .is_err()
    {
        return false;
    }
    conn.query_row("SELECT 1 FROM entries LIMIT 1", [], |row| {
        row.get::<_, i64>(0)
    })
    .is_ok()
}

/// The row shape every select above carries.
fn row_shape(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, RawRow)> {
    Ok((
        row.get(0)?,
        RawRow {
            word: row.get(1)?,
            pos: row.get(2)?,
            definition: row.get(3)?,
            romanization: row.get(4)?,
            sense: row.get(5)?,
        },
    ))
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

    /// A temp pack table with a few rows to ask.
    fn temp_db(tag: &str, rows: &[(&str, &str)]) -> (std::path::PathBuf, DictDb) {
        let dir = std::env::temp_dir().join(format!("dict_db_{tag}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pack.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(crate::dict::build::SCHEMA).unwrap();
        {
            let mut stmt = conn
                .prepare(
                    "INSERT INTO entries (word, word_fold, definition, def_fold)                      VALUES (?1, ?2, ?3, ?4)",
                )
                .unwrap();
            for (word, definition) in rows {
                stmt.execute(params![word, fold(word), definition, fold(definition)])
                    .unwrap();
            }
        }
        conn.pragma_update(None, "user_version", 1).unwrap();
        drop(conn);
        let db = DictDb::open(&path).unwrap();
        (dir, db)
    }

    #[test]
    fn only_a_finished_nonempty_current_schema_is_ready() {
        let (dir, db) = temp_db("probe", &[("cat", "chat")]);
        assert!(is_usable(&dir.join("pack.db")));
        drop(db);
        let old = dir.join("old.db");
        let conn = Connection::open(&old).unwrap();
        conn.execute_batch("CREATE TABLE entry (word TEXT, gloss TEXT);")
            .unwrap();
        drop(conn);
        assert!(!is_usable(&old));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_misspelled_ask_still_finds_the_word_and_the_other_side() {
        let (dir, db) = temp_db(
            "fuzzy",
            &[("running", "palai"), ("color", "couleur"), ("chat", "cat")],
        );
        let got = db.search("runing", 10).unwrap();
        assert!(got.iter().any(|row| row.word == "running"), "{got:?}");
        // The far side answers too: a French ask one letter
        // off finds its English word.
        let got = db.search("couler", 10).unwrap();
        assert!(got.iter().any(|row| row.word == "color"), "{got:?}");
        // A short ask is a prefix question, not a spelling one.
        let got = db.search("ru", 10).unwrap();
        assert!(got.iter().any(|row| row.word == "running"), "{got:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_definition_side_is_askable_both_ways() {
        let (dir, db) = temp_db("def", &[("light", "lumiere")]);
        let got = db.lookup_definition("lumiere", 10).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].word, "light");
        let got = db.search("lumier", 10).unwrap();
        assert!(got.iter().any(|row| row.word == "light"), "{got:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_far_shore_is_searchable_the_loose_way() {
        let (dir, db) = temp_db("defsearch", &[("light", "lumiere"), ("book", "livre")]);
        // A prefix on the far shore finds its headword.
        let got = db.search_definition("lumier", 10).unwrap();
        assert!(got.iter().any(|row| row.word == "light"), "{got:?}");
        // So does the whole word, and nothing else comes with it.
        let got = db.search_definition("lumiere", 10).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].word, "light");
        // A near miss on the far shore still lands.
        let got = db.search_definition("lumierr", 10).unwrap();
        assert!(got.iter().any(|row| row.word == "light"), "{got:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_wildcard_in_the_ask_stays_data() {
        assert_eq!(like_escape("100%"), "100\\%");
        assert_eq!(like_escape("a_b"), "a\\_b");
        assert_eq!(like_escape("c\\d"), "c\\\\d");
        assert_eq!(like_prefix("ca"), "ca%");
        assert_eq!(like_contains("ca"), "%ca%");
    }

    #[test]
    fn search_deduplicates_rows_across_passes() {
        let (dir, db) = temp_db("dedup", &[("light", "lumiere"), ("lighter", "briquet")]);
        let got = db.search("light", 10).unwrap();
        assert_eq!(
            got.iter().filter(|row| row.word == "light").count(),
            1,
            "exact row must not repeat in prefix/contains passes"
        );
        let defs = db
            .lookup_defs_any(&["lumiere".into(), "briquet".into()], 10)
            .unwrap();
        assert_eq!(defs.len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
