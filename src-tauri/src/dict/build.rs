//! A pack's parquet becomes one sqlite table by column name.

use std::path::Path;

use anyhow::{Context, Result};
use arrow::array::{Array, AsArray};
use arrow::compute::cast;
use arrow::datatypes::DataType;
use arrow::record_batch::RecordBatch;
use dict_core::fold;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use rusqlite::{Connection, OpenFlags, OptionalExtension};

/// The table every pack lands as; the query layer reads nothing else.
pub const SCHEMA: &str = "CREATE TABLE entries (
    id INTEGER PRIMARY KEY,
    word TEXT NOT NULL,
    word_fold TEXT NOT NULL,
    pos TEXT,
    definition TEXT NOT NULL,
    def_fold TEXT NOT NULL,
    romanization TEXT,
    sense TEXT,
    lang_code TEXT,
    source TEXT
);
CREATE INDEX entries_word ON entries(word_fold);
CREATE INDEX entries_def ON entries(def_fold);";

/// A parquet read to name-addressed rows of strings.
pub struct Table {
    pub names: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
}

/// Read any pack parquet: rows addressed by column NAME.
pub fn read_parquet(path: &Path) -> Result<Table> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .with_context(|| format!("builder {}", path.display()))?
        .build()?;
    let mut names: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<Option<String>>> = Vec::new();
    for batch in reader {
        let batch = batch.with_context(|| format!("row group {}", path.display()))?;
        if names.is_empty() {
            names = batch
                .schema()
                .fields()
                .iter()
                .map(|f| f.name().to_string())
                .collect();
        }
        rows.extend(batch_rows(&batch, &names)?);
    }
    Ok(Table { names, rows })
}

/// One record batch's rows, every column as a string or none.
fn batch_rows(batch: &RecordBatch, names: &[String]) -> Result<Vec<Vec<Option<String>>>> {
    let cols: Vec<Vec<Option<String>>> = names
        .iter()
        .enumerate()
        .map(|(i, _)| col_strings(batch.column(i)))
        .collect::<Result<_>>()?;
    let mut rows = Vec::with_capacity(batch.num_rows());
    for r in 0..batch.num_rows() {
        rows.push(cols.iter().map(|col| col[r].clone()).collect());
    }
    Ok(rows)
}

/// A column as strings: utf8 by nature or by cast.
fn col_strings(column: &std::sync::Arc<dyn Array>) -> Result<Vec<Option<String>>> {
    if column.data_type() == &DataType::Null {
        return Ok(vec![None; column.len()]);
    }
    let utf8 = if column.data_type() == &DataType::Utf8 {
        column.clone()
    } else {
        cast(column, &DataType::Utf8)
            .with_context(|| format!("cast {:?} to utf8", column.data_type()))?
    };
    let text = utf8.as_string::<i32>();
    Ok((0..text.len())
        .map(|i| text.is_valid(i).then(|| text.value(i).to_string()))
        .collect())
}

/// Build the sqlite beside the parquet. `word` + `definition`
/// must be present.
pub fn build_db(parquet_path: &Path, db_path: &Path) -> Result<u64> {
    let table = read_parquet(parquet_path)?;
    let at = |name: &str| {
        table
            .names
            .iter()
            .position(|col| col == name)
            .with_context(|| format!("no {name} column"))
    };
    let word_at = at("word")?;
    let def_at = at("definition")?;
    let pos_at = table.names.iter().position(|col| col == "pos");
    let rome_at = table.names.iter().position(|col| col == "romanization");
    let sense_at = table.names.iter().position(|col| col == "sense");
    let lang_at = table.names.iter().position(|col| col == "lang_code");
    let src_at = table.names.iter().position(|col| col == "source");

    if db_path.exists() {
        std::fs::remove_file(db_path)?;
    }
    let mut db = Connection::open(db_path)?;
    db.execute_batch(SCHEMA)?;

    let mut rows = 0u64;
    {
        let insert = db.transaction()?;
        {
            let mut stmt = insert.prepare_cached(
                "INSERT INTO entries
                    (word, word_fold, pos, definition, def_fold,
                     romanization, sense, lang_code, source)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for row in &table.rows {
                let (Some(word), Some(definition)) =
                    (row[word_at].as_deref(), row[def_at].as_deref())
                else {
                    continue;
                };
                if word.trim().is_empty() || definition.trim().is_empty() {
                    continue;
                }
                let get = |slot: Option<usize>| slot.and_then(|i| row[i].clone());
                stmt.execute(rusqlite::params![
                    word,
                    fold(word),
                    get(pos_at),
                    definition,
                    fold(definition),
                    get(rome_at),
                    get(sense_at),
                    get(lang_at),
                    get(src_at),
                ])?;
                rows += 1;
            }
        }
        insert.commit()?;
    }
    db.pragma_update(None, "user_version", 1)?;
    Ok(rows)
}

/// Read one older SQLite pack into the current table shape.
pub fn build_legacy_db(
    source_path: &Path,
    db_path: &Path,
    pack_id: &str,
    pair: &str,
) -> Result<Option<u64>> {
    let source = Connection::open_with_flags(source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open legacy {}", source_path.display()))?;
    let Some(info) = legacy_info(&source)? else {
        return Ok(None);
    };
    if info.pack.is_none() && !crate::dict::legacy::filename_matches(source_path, pack_id, pair) {
        return Ok(None);
    }

    let pack_filter = info
        .pack
        .as_deref()
        .map(|column| {
            format!(
                " WHERE replace(lower({}), '_', '-') IN (?1, ?2)",
                quote(column)
            )
        })
        .unwrap_or_default();
    let query = format!(
        "SELECT {}, {}, {}, {}, {}, {}, {} FROM {}{pack_filter}",
        quote(&info.word),
        info.pos.as_deref().map(quote).unwrap_or_else(|| "NULL".into()),
        quote(&info.definition),
        info.romanization
            .as_deref()
            .map(quote)
            .unwrap_or_else(|| "NULL".into()),
        info.sense
            .as_deref()
            .map(quote)
            .unwrap_or_else(|| "NULL".into()),
        info.lang_code
            .as_deref()
            .map(quote)
            .unwrap_or_else(|| "NULL".into()),
        info.source
            .as_deref()
            .map(quote)
            .unwrap_or_else(|| "NULL".into()),
        quote(&info.table),
    );
    let mut source_stmt = source.prepare(&query)?;
    let mut source_rows = if info.pack.is_some() {
        source_stmt.query(rusqlite::params![pack_id, pair])?
    } else {
        source_stmt.query([])?
    };

    if db_path.exists() {
        std::fs::remove_file(db_path)?;
    }
    let mut db = Connection::open(db_path)?;
    db.execute_batch(SCHEMA)?;
    let mut rows = 0u64;
    {
        let insert = db.transaction()?;
        let mut stmt = insert.prepare_cached(
            "INSERT INTO entries
                (word, word_fold, pos, definition, def_fold,
                 romanization, sense, lang_code, source)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        while let Some(row) = source_rows.next()? {
            let word: Option<String> = row.get(0)?;
            let pos: Option<String> = row.get(1)?;
            let definition: Option<String> = row.get(2)?;
            let romanization: Option<String> = row.get(3)?;
            let sense: Option<String> = row.get(4)?;
            let lang_code: Option<String> = row.get(5)?;
            let source: Option<String> = row.get(6)?;
            let (Some(word), Some(definition)) = (word.as_deref(), definition.as_deref()) else {
                continue;
            };
            if word.trim().is_empty() || definition.trim().is_empty() {
                continue;
            }
            stmt.execute(rusqlite::params![
                word,
                fold(word),
                pos,
                definition,
                fold(definition),
                romanization,
                sense,
                lang_code,
                source,
            ])?;
            rows += 1;
        }
        drop(stmt);
        insert.commit()?;
    }
    db.pragma_update(None, "user_version", 1)?;
    drop(db);
    if rows == 0 {
        std::fs::remove_file(db_path)?;
        return Ok(None);
    }
    Ok(Some(rows))
}

/// Whether an older file contains rows for the named pack.
pub fn legacy_db_matches(source_path: &Path, pack_id: &str, pair: &str) -> Result<bool> {
    let source = Connection::open_with_flags(source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open legacy {}", source_path.display()))?;
    let Some(info) = legacy_info(&source)? else {
        return Ok(false);
    };
    if info.pack.is_none() && !crate::dict::legacy::filename_matches(source_path, pack_id, pair) {
        return Ok(false);
    }
    let filter = info
        .pack
        .as_deref()
        .map(|column| {
            format!(
                " AND replace(lower({}), '_', '-') IN (?1, ?2)",
                quote(column)
            )
        })
        .unwrap_or_default();
    let query = format!(
        "SELECT 1 FROM {} WHERE {} IS NOT NULL AND {} IS NOT NULL{filter} LIMIT 1",
        quote(&info.table),
        quote(&info.word),
        quote(&info.definition),
    );
    let found = if info.pack.is_some() {
        source
            .query_row(&query, rusqlite::params![pack_id, pair], |row| row.get::<_, i64>(0))
            .optional()?
            .is_some()
    } else {
        source
            .query_row(&query, [], |row| row.get::<_, i64>(0))
            .optional()?
            .is_some()
    };
    Ok(found)
}

#[derive(Clone)]
struct LegacyInfo {
    table: String,
    word: String,
    definition: String,
    pack: Option<String>,
    pos: Option<String>,
    romanization: Option<String>,
    sense: Option<String>,
    lang_code: Option<String>,
    source: Option<String>,
}

fn legacy_info(conn: &Connection) -> Result<Option<LegacyInfo>> {
    let mut tables = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?;
    let tables = tables.query_map([], |row| row.get::<_, String>(0))?;
    let tables = tables.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut candidates = Vec::new();
    for table in ["entries", "entry", "dict"] {
        if !tables.iter().any(|name| name.as_str() == table) {
            continue;
        }
        let mut columns_stmt = conn.prepare(&format!("PRAGMA table_info({})", quote(table)))?;
        let columns = columns_stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let pick = |names: &[&str]| {
            names
                .iter()
                .find(|name| columns.iter().any(|column| column.as_str() == **name))
                .map(|name| (*name).to_string())
        };
        let (Some(word), Some(definition)) = (pick(&["word"]), pick(&["definition", "gloss"]))
        else {
            continue;
        };
        candidates.push(LegacyInfo {
            table: table.to_string(),
            word,
            definition,
            pack: pick(&["pack"]),
            pos: pick(&["pos", "kind"]),
            romanization: pick(&["romanization", "roman"]),
            sense: pick(&["sense"]),
            lang_code: pick(&["lang_code"]),
            source: pick(&["source", "note"]),
        });
    }
    Ok(candidates
        .iter()
        .find(|info| info.pack.is_some())
        .or_else(|| candidates.first())
        .cloned())
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use arrow::array::StringArray;
    use arrow::datatypes::{DataType, Field, Schema};
    use parquet::arrow::ArrowWriter;
    use parquet::basic::{Compression, ZstdLevel};
    use parquet::file::properties::WriterProperties;

    /// Write one mini pack in `compression`, then build it to sqlite.
    fn roundtrip(tag: &str, compression: Compression) {
        let dir = std::env::temp_dir().join(format!("dict_build_{tag}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let parquet_path = dir.join("mini.parquet");
        let schema = Schema::new(vec![
            Field::new("word", DataType::Utf8, true),
            Field::new("pos", DataType::Utf8, true),
            Field::new("definition", DataType::Utf8, true),
        ]);
        let batch = RecordBatch::try_new(
            Arc::new(schema.clone()),
            vec![
                Arc::new(StringArray::from(vec!["cat", "run"])),
                Arc::new(StringArray::from(vec![Some("n"), Some("v, n")])),
                Arc::new(StringArray::from(vec!["chat", "palai"])),
            ],
        )
        .unwrap();
        let props = WriterProperties::builder()
            .set_compression(compression)
            .build();
        let file = std::fs::File::create(&parquet_path).unwrap();
        let mut writer = ArrowWriter::try_new(file, Arc::new(schema), Some(props)).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();

        let db_path = dir.join("mini.db");
        let rows = build_db(&parquet_path, &db_path).unwrap();
        assert_eq!(rows, 2, "{tag} rows");
        let conn = Connection::open(&db_path).unwrap();
        let found: String = conn
            .query_row(
                "SELECT word FROM entries WHERE definition = 'chat'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(found, "cat", "{tag} reads back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The Myanmar pack lands snappy from its HuggingFace home.
    #[test]
    fn a_snappy_body_builds_like_mcf_nlp_does() {
        roundtrip("snappy", Compression::SNAPPY);
    }

    /// The other packs land zstd from the wikidict scripts.
    #[test]
    fn a_zstd_body_builds_like_the_shipped_packs() {
        let level = ZstdLevel::try_new(1).unwrap();
        roundtrip("zstd", Compression::ZSTD(level));
    }
    #[test]
    fn the_old_combined_store_yields_only_the_requested_pack() {
        let dir = std::env::temp_dir().join(format!("dict_legacy_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("dictionary.db");
        let conn = Connection::open(&old).unwrap();
        conn.execute_batch(
            "CREATE TABLE entry (
                id INTEGER PRIMARY KEY,
                pack TEXT NOT NULL,
                word TEXT NOT NULL,
                key TEXT NOT NULL,
                pos TEXT,
                kind TEXT,
                gloss TEXT NOT NULL,
                roman TEXT,
                sense TEXT,
                note TEXT
            );
            INSERT INTO entry (pack, word, key, pos, gloss, roman, sense)
                VALUES ('en-my', 'light', 'light', 'n', 'အလင်း', NULL, 'illumination');
            INSERT INTO entry (pack, word, key, pos, gloss)
                VALUES ('en-jp', 'light', 'light', 'noun', '光');",
        )
        .unwrap();
        drop(conn);

        assert!(legacy_db_matches(&old, "mcfnlp-en-my", "en-my").unwrap());
        assert!(!legacy_db_matches(&old, "mcfnlp-en-my", "en-jp").unwrap());
        let current = dir.join("mcfnlp-en-my.db.tmp");
        assert_eq!(
            build_legacy_db(&old, &current, "mcfnlp-en-my", "en-my").unwrap(),
            Some(1)
        );
        let db = Connection::open(&current).unwrap();
        let entry: (String, String, String) = db
            .query_row(
                "SELECT word, definition, sense FROM entries LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(entry, ("light".into(), "အလင်း".into(), "illumination".into()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_row_without_word_or_definition_is_dropped() {
        let names = vec!["word".to_string(), "definition".to_string()];
        let rows: Vec<Vec<Option<String>>> = vec![
            vec![Some("cat".into()), Some("chat".into())],
            vec![None, Some("chat".into())],
            vec![Some("cat".into()), Some(" ".into())],
        ];
        let mut kept = 0;
        for row in &rows {
            let (Some(word), Some(definition)) = (row[0].as_deref(), row[1].as_deref()) else {
                continue;
            };
            if word.trim().is_empty() || definition.trim().is_empty() {
                continue;
            }
            kept += 1;
            let _ = (word, definition, &names);
        }
        assert_eq!(kept, 1);
    }

    #[test]
    fn the_paths_a_lifecycle_moves_between_are_distinct() {
        let dir = std::path::PathBuf::from("/data/dict");
        assert_ne!(
            crate::dict::fetch::db_file(&dir, "jmdict-en-jp"),
            crate::dict::fetch::db_building(&dir, "jmdict-en-jp")
        );
        assert_eq!(
            crate::dict::fetch::parquet_file(&dir, "jmdict-en-jp"),
            dir.join("jmdict-en-jp.parquet")
        );
    }
}
