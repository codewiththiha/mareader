//! The two files this feature fetches, and what proves each one landed.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use download_core::Job;

/// The directory under the app's data dir that both files land in.
pub const FEATURE: &str = "cefr";

/// The downloader's key for the word-level dataset.
pub const DATASET_ID: &str = "cefr-dataset";

/// The word-level dataset, as the cefr repository serves it.
const DATASET_URL: &str =
    "https://raw.githubusercontent.com/codewiththiha/cefr-rs/main/data/cefr.zstd.parquet";

/// Further hosts serving the same bytes, for a firewalled client.
const DATASET_MIRRORS: &[&str] = &[
    "https://cdn.jsdelivr.net/gh/codewiththiha/cefr-rs@main/data/cefr.zstd.parquet",
    "https://fastly.jsdelivr.net/gh/codewiththiha/cefr-rs@main/data/cefr.zstd.parquet",
];

/// The adopted parquet's name; the rebuild consumes it and deletes it.
const DATASET_FILE: &str = "cefr.parquet";

/// The downloader's key for the click-time tagger model.
pub const TAGGER_ID: &str = "cefr-model";

/// The runtime tagger model the click-time POS reads.
const TAGGER_URL: &str =
    "https://raw.githubusercontent.com/codewiththiha/cefr-rs/main/models/en_tokenizer.bin.zst";

/// Further hosts serving the same model.
const TAGGER_MIRRORS: &[&str] = &[
    "https://cdn.jsdelivr.net/gh/codewiththiha/cefr-rs@main/models/en_tokenizer.bin.zst",
    "https://fastly.jsdelivr.net/gh/codewiththiha/cefr-rs@main/models/en_tokenizer.bin.zst",
];

/// The adopted model's name, which is also the extension `cefr` decodes by.
const TAGGER_FILE: &str = "en_tokenizer.bin.zst";

/// The rebuilt database, and the name it is built under first.
const DB_FILE: &str = "cefr.db";
const DB_BUILDING: &str = "cefr.db.tmp";

/// The signatures the two bodies carry and an HTML error page does not.
static PARQUET_MAGIC: [u8; 4] = *b"PAR1";
static ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

pub fn dataset_file(dir: &Path) -> PathBuf {
    dir.join(DATASET_FILE)
}

pub fn tagger_file(dir: &Path) -> PathBuf {
    dir.join(TAGGER_FILE)
}

pub fn db_final(dir: &Path) -> PathBuf {
    dir.join(DB_FILE)
}

pub fn db_building(dir: &Path) -> PathBuf {
    dir.join(DB_BUILDING)
}

/// The dataset's download: named, mirrored and checked before adoption.
pub fn dataset_job(dir: PathBuf) -> Job {
    Job::new(DATASET_ID, dir, DATASET_URL)
        .mirrors(DATASET_MIRRORS)
        .named(DATASET_FILE)
        .verified_by(|path| bracketed_by(path, &PARQUET_MAGIC, "a parquet"))
}

/// The tagger model's download.
pub fn tagger_job(dir: PathBuf) -> Job {
    Job::new(TAGGER_ID, dir, TAGGER_URL)
        .mirrors(TAGGER_MIRRORS)
        .named(TAGGER_FILE)
        .verified_by(|path| starts_with(path, &ZSTD_MAGIC, "a zstd frame"))
}

/// A parquet carries its signature at both ends, so a truncated body fails.
fn bracketed_by(path: &Path, magic: &[u8; 4], what: &str) -> Result<(), String> {
    starts_with(path, magic, what)?;
    let mut file = std::fs::File::open(path).map_err(|e| format!("reopen {what}: {e}"))?;
    let len = file
        .metadata()
        .map_err(|e| format!("stat {what}: {e}"))?
        .len();
    let mut tail = [0u8; 4];
    file.seek(SeekFrom::Start(len.saturating_sub(4)))
        .map_err(|e| format!("seek {what}: {e}"))?;
    file.read_exact(&mut tail)
        .map_err(|e| format!("read {what}: {e}"))?;
    (tail == *magic)
        .then_some(())
        .ok_or_else(|| format!("{what} is truncated"))
}

fn starts_with(path: &Path, magic: &[u8; 4], what: &str) -> Result<(), String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("open {what}: {e}"))?;
    let len = file
        .metadata()
        .map_err(|e| format!("stat {what}: {e}"))?
        .len();
    if len < 8 {
        return Err(format!("{what} is too small ({len} bytes)"));
    }
    let mut head = [0u8; 4];
    file.read_exact(&mut head)
        .map_err(|e| format!("read {what}: {e}"))?;
    (head == *magic)
        .then_some(())
        .ok_or_else(|| format!("not {what}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cefr_fetch_{tag}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_signature_check_rejects_an_error_page() {
        let dir = temp("magic");
        let page = write(&dir, "page.parquet", b"<html>Not Found</html>");
        let error = bracketed_by(&page, &PARQUET_MAGIC, "a parquet").unwrap_err();
        assert!(error.contains("not a parquet"), "{error}");

        // A real head with nothing behind it is a truncation, not a parquet.
        let head_only = write(&dir, "head.parquet", b"PAR1PAR1");
        assert!(bracketed_by(&head_only, &PARQUET_MAGIC, "a parquet").is_ok());
        let short = write(&dir, "short.parquet", b"PAR1");
        assert!(bracketed_by(&short, &PARQUET_MAGIC, "a parquet").is_err());

        let model = write(&dir, "model.zst", &[0x28, 0xb5, 0x2f, 0xfd, 0, 0, 0, 0]);
        assert!(starts_with(&model, &ZSTD_MAGIC, "a zstd frame").is_ok());
        assert!(starts_with(&page, &ZSTD_MAGIC, "a zstd frame").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_job_is_named_and_aims_at_its_own_directory() {
        let job = dataset_job(PathBuf::from("/data/cefr"));
        assert_eq!(job.dest(), Some(PathBuf::from("/data/cefr/cefr.parquet")));
        assert_eq!(
            job.partial(),
            Some(PathBuf::from("/data/cefr/cefr.parquet.part"))
        );
        assert!(job.check().is_ok());

        let model = tagger_job(PathBuf::from("/data/cefr"));
        assert_eq!(
            model.dest(),
            Some(PathBuf::from("/data/cefr/en_tokenizer.bin.zst"))
        );
        assert!(model.check().is_ok());
    }

    #[test]
    fn the_paths_a_lifecycle_moves_between_are_distinct() {
        let dir = PathBuf::from("/data/cefr");
        assert_ne!(db_final(&dir), db_building(&dir));
        assert_ne!(dataset_file(&dir), tagger_file(&dir));
        assert_eq!(FEATURE, "cefr");
    }
}
