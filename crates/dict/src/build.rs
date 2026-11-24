//! A downloaded parquet becomes a pack's SQLite table, streamed by
//! column name.

use std::fs::File;
use std::path::Path;

use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::Field;
use rusqlite::{Connection, params};

use crate::search::fold;
use crate::store::{INDEXES, LAYOUT, SCHEMA};

/// More rows than this is a mistake.
const MAX_ROWS: i64 = 5_000_000;
/// A longer cell is cut; no definition needs more.
const MAX_CELL: usize = 2_000;

/// The columns a pack may carry, by name; `word` and `definition` must.
#[derive(Default)]
struct Cells {
    word: Option<String>,
    pos: Option<String>,
    definition: Option<String>,
    romanization: Option<String>,
    sense: Option<String>,
}

/// Builds `out` from `parquet`; the rows kept. `out` is replaced.
pub fn build(parquet: &Path, out: &Path) -> Result<u64, String> {
    let file = File::open(parquet).map_err(|e| format!("open: {e}"))?;
    let reader = SerializedFileReader::new(file).map_err(|e| format!("not a pack: {e}"))?;
    let total = reader.metadata().file_metadata().num_rows();
    if total > MAX_ROWS {
        return Err(format!("{total} rows is more than a dictionary holds"));
    }
    let names = column_names(&reader);
    for needed in ["word", "definition"] {
        if !names.iter().any(|n| n == needed) {
            return Err(format!("the pack has no {needed} column"));
        }
    }
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
        let mut insert = tx
            .prepare(
                "INSERT INTO entries (word, word_fold, pos, definition, def_fold, \
                 romanization, sense) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(|e| e.to_string())?;
        let rows = reader.get_row_iter(None).map_err(|e| e.to_string())?;
        for row in rows {
            let row = row.map_err(|e| format!("a damaged row: {e}"))?;
            let mut cells = Cells::default();
            for (name, field) in row.get_column_iter() {
                let slot = match name.as_str() {
                    "word" => &mut cells.word,
                    "pos" => &mut cells.pos,
                    "definition" => &mut cells.definition,
                    "romanization" => &mut cells.romanization,
                    "sense" => &mut cells.sense,
                    _ => continue,
                };
                *slot = text(field);
            }
            let (Some(word), Some(definition)) = (cells.word, cells.definition) else {
                continue;
            };
            let args = params![
                word,
                fold(&word),
                cells.pos,
                definition,
                fold(&definition),
                cells.romanization,
                cells.sense
            ];
            insert.execute(args).map_err(|e| e.to_string())?;
            kept += 1;
        }
    }
    if kept == 0 {
        return Err("the pack has no entries".into());
    }
    tx.execute_batch(INDEXES).map_err(|e| e.to_string())?;
    let meta = "INSERT INTO meta (key, value) VALUES (?1, ?2)";
    tx.execute(meta, ["layout", LAYOUT])
        .map_err(|e| e.to_string())?;
    tx.execute(meta, ["rows", &kept.to_string()])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(kept)
}

fn column_names(reader: &SerializedFileReader<File>) -> Vec<String> {
    let schema = reader.metadata().file_metadata().schema_descr();
    schema
        .columns()
        .iter()
        .map(|c| c.name().to_owned())
        .collect()
}

/// A cell as trimmed text, cut to `MAX_CELL`; blank is none.
fn text(field: &Field) -> Option<String> {
    let s = match field {
        Field::Str(s) => s.trim(),
        _ => return None,
    };
    if s.is_empty() {
        return None;
    }
    Some(match s.char_indices().nth(MAX_CELL) {
        Some((at, _)) => s.get(..at).unwrap_or(s).to_owned(),
        None => s.to_owned(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn both_codecs_convert_and_blank_rows_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("my.db");
        // Snappy, three columns, a missing word and a missing definition.
        assert_eq!(build(&fixture("en-my.parquet"), &db), Ok(10));
        assert_eq!(Store::open(&db).unwrap().rows(), Ok(10));
        // Zstd, every column, three row groups; built again over the old.
        assert_eq!(build(&fixture("en-fr.parquet"), &db), Ok(7));
    }

    #[test]
    fn a_file_that_is_not_a_pack_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let page = dir.path().join("page.parquet");
        std::fs::write(&page, b"<html>Not Found</html>").unwrap();
        let err = build(&page, &dir.path().join("x.db")).unwrap_err();
        assert!(err.starts_with("not a pack"), "{err}");
    }

    #[test]
    fn long_cells_are_cut() {
        let long = "a".repeat(MAX_CELL + 5);
        assert_eq!(text(&Field::Str(long)).map(|s| s.len()), Some(MAX_CELL));
        assert_eq!(text(&Field::Str("  ".into())), None);
        assert_eq!(text(&Field::Null), None);
    }
}
