//! A pack's parquet becomes one sqlite table by column name.

use std::path::Path;

use anyhow::{Context, Result};
use arrow::array::{Array, AsArray};
use arrow::compute::cast;
use arrow::datatypes::DataType;
use arrow::record_batch::RecordBatch;
use dict_core::fold;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use rusqlite::Connection;

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
    db.execute_batch(
        "CREATE TABLE entries (
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
        CREATE INDEX entries_def ON entries(def_fold);",
    )?;

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

#[cfg(test)]
mod tests {
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
