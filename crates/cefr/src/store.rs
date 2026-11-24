//! The dataset as a local SQLite table: built once, then asked.

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::Field;
use parquet::schema::types::Type;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params, params_from_iter};

/// Marks a finished build; a table without it is rebuilt.
const LAYOUT: i64 = 1;
/// More rows than this is a mistake.
const MAX_ROWS: i64 = 5_000_000;
/// Words asked per query, far below SQLite's bind limit.
const CHUNK: usize = 500;

const SCHEMA: &str = "CREATE TABLE cefr (word TEXT NOT NULL, pos_tag TEXT, level REAL NOT NULL);";

/// Builds `out` from the downloaded `parquet`; the senses kept.
pub fn build(parquet: &Path, out: &Path) -> Result<u64, String> {
    let file = File::open(parquet).map_err(|e| format!("open: {e}"))?;
    let reader = SerializedFileReader::new(file).map_err(|e| format!("not the dataset: {e}"))?;
    let meta = reader.metadata().file_metadata();
    if meta.num_rows() > MAX_ROWS {
        return Err(format!("{} rows is more than the dataset", meta.num_rows()));
    }
    // Only the three columns read, in the file's own types.
    let wanted = ["word", "pos_tag", "level"];
    let fields: Vec<_> = meta
        .schema()
        .get_fields()
        .iter()
        .filter(|f| wanted.contains(&f.name()))
        .cloned()
        .collect();
    for needed in ["word", "level"] {
        if !fields.iter().any(|f| f.name() == needed) {
            return Err(format!("the dataset has no {needed} column"));
        }
    }
    let projection = Type::group_type_builder("schema")
        .with_fields(fields)
        .build()
        .map_err(|e| e.to_string())?;
    if out.exists() {
        std::fs::remove_file(out).map_err(|e| format!("replace: {e}"))?;
    }
    let mut db = Connection::open(out).map_err(|e| e.to_string())?;
    // A file renamed into place when whole needs no journal.
    db.execute_batch("PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;")
        .map_err(|e| e.to_string())?;
    db.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let mut kept = 0u64;
    {
        let sql = "INSERT INTO cefr (word, pos_tag, level) VALUES (?1, ?2, ?3)";
        let mut insert = tx.prepare(sql).map_err(|e| e.to_string())?;
        let rows = reader
            .get_row_iter(Some(projection))
            .map_err(|e| e.to_string())?;
        for row in rows {
            let row = row.map_err(|e| format!("a damaged row: {e}"))?;
            let (mut word, mut pos, mut level) = (None, None, None);
            for (name, field) in row.get_column_iter() {
                match (name.as_str(), field) {
                    ("word", Field::Str(s)) => word = Some(crate::normalize(s.trim())),
                    ("pos_tag", Field::Str(s)) => pos = Some(s.trim().to_owned()),
                    ("level", f) => level = number(f),
                    _ => {}
                }
            }
            let (Some(word), Some(level)) = (word, level) else {
                continue;
            };
            if word.is_empty() || !level.is_finite() {
                continue;
            }
            let pos = pos.filter(|p| !p.is_empty());
            insert
                .execute(params![word, pos, level])
                .map_err(|e| e.to_string())?;
            kept += 1;
        }
    }
    if kept == 0 {
        return Err("the dataset has no words".into());
    }
    tx.execute_batch("CREATE INDEX cefr_word ON cefr(word);")
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    db.execute_batch(&format!("ANALYZE; PRAGMA user_version = {LAYOUT};"))
        .map_err(|e| e.to_string())?;
    Ok(kept)
}

fn number(field: &Field) -> Option<f64> {
    match field {
        Field::Float(f) => Some(f64::from(*f)),
        Field::Double(d) => Some(*d),
        Field::Int(i) => Some(f64::from(*i)),
        Field::Long(l) => Some(*l as f64),
        _ => None,
    }
}

/// One part of speech of a word and its level.
#[derive(Clone, Debug, PartialEq)]
pub struct Sense {
    pub pos: String,
    pub level: f64,
}

/// An open, finished table.
pub struct Store {
    db: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let db = Connection::open_with_flags(path, flags).map_err(|e| e.to_string())?;
        let layout: i64 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if layout != LAYOUT {
            return Err("the word table is unfinished".into());
        }
        Ok(Self { db })
    }

    /// How many words the table knows.
    pub fn words(&self) -> Result<u64, String> {
        let sql = "SELECT COUNT(DISTINCT word) FROM cefr";
        let n: i64 = self
            .db
            .query_row(sql, [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Each known key's level, averaged over its senses; others absent.
    pub fn levels(&self, keys: &[String]) -> Result<HashMap<String, f64>, String> {
        let mut out = HashMap::with_capacity(keys.len());
        for chunk in keys.chunks(CHUNK) {
            let values = vec!["(?)"; chunk.len()].join(", ");
            let sql = format!(
                "WITH wanted(word) AS (VALUES {values}) \
                 SELECT wanted.word, AVG(c.level) FROM wanted \
                 JOIN cefr c ON c.word = wanted.word GROUP BY wanted.word"
            );
            let mut stmt = self.db.prepare(&sql).map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params_from_iter(chunk.iter()), |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
                })
                .map_err(|e| e.to_string())?;
            for row in rows {
                let (word, level) = row.map_err(|e| e.to_string())?;
                out.insert(word, level);
            }
        }
        Ok(out)
    }

    /// Every part of speech `key` has, with its level, by tag.
    pub fn senses(&self, key: &str) -> Result<Vec<Sense>, String> {
        let sql = "SELECT pos_tag, AVG(level) FROM cefr \
                   WHERE word = ?1 AND pos_tag IS NOT NULL \
                   GROUP BY pos_tag ORDER BY pos_tag";
        let mut stmt = self.db.prepare(sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([key], |r| {
                Ok(Sense {
                    pos: r.get(0)?,
                    level: r.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    /// `key` as `tag`: exact, else the tag's family, else its first.
    pub fn sense(&self, key: &str, tag: &str) -> Result<Option<Sense>, String> {
        let sql = "SELECT AVG(level) FROM cefr WHERE word = ?1 AND pos_tag = ?2";
        let exact: Option<f64> = self
            .db
            .query_row(sql, [key, tag], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?
            .flatten();
        if let Some(level) = exact {
            let pos = tag.to_owned();
            return Ok(Some(Sense { pos, level }));
        }
        let senses = self.senses(key)?;
        let family = tag.chars().next();
        let near = senses
            .iter()
            .find(|s| family.is_some_and(|f| s.pos.starts_with(f)));
        Ok(near.or(senses.first()).cloned())
    }
}

/// Whether `path` holds a finished table, and its word count.
pub fn usable(path: &Path) -> Option<u64> {
    Store::open(path).ok()?.words().ok()
}
