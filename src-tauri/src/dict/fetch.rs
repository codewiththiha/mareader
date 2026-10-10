//! Each pack's download: named, mirrored, checked before
//! adoption.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use download_core::Job;

use super::packs::{PackDef, pack};

/// The cache folder every pack lives under.
pub const FEATURE: &str = "dict";

/// A parquet carries its signature at both ends; an error page does not.
static PARQUET_MAGIC: [u8; 4] = *b"PAR1";

/// The parquet a pack downloads to.
pub fn parquet_file(dir: &Path, pack_id: &str) -> PathBuf {
    dir.join(format!("{pack_id}.parquet"))
}

/// The sqlite a pack converts to.
pub fn db_file(dir: &Path, pack_id: &str) -> PathBuf {
    dir.join(format!("{pack_id}.db"))
}

/// The half-built sqlite a crash leaves behind.
pub fn db_building(dir: &Path, pack_id: &str) -> PathBuf {
    dir.join(format!("{pack_id}.db.tmp"))
}

/// One pack's job: its url, mirrors, and the check
/// an error page cannot pass.
pub fn pack_job(dir: PathBuf, pack: &PackDef) -> Job {
    Job::new(pack.id, dir, pack.url)
        .mirrors(pack.mirrors.iter().copied())
        .named(pack.file)
        .verified_by(|path| bracketed_by(path, &PARQUET_MAGIC, "a parquet"))
}

/// The job for an id, if the id names an offered pack.
pub fn job_for(dir: PathBuf, pack_id: &str) -> Option<Job> {
    pack(pack_id).map(|pack| pack_job(dir, pack))
}

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

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dict_fetch_{tag}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_pack_job_aims_at_its_own_stem() {
        let dir = temp("job");
        let job = job_for(dir.clone(), "jmdict-en-jp").expect("the jmdict pack");
        assert_eq!(job.dest(), Some(dir.join("jmdict-en-jp.parquet")));
        assert!(job.check().is_ok());
        assert!(job_for(dir.clone(), "blorp").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_error_page_never_adopts() {
        let dir = temp("magic");
        let page = write(&dir, "page.parquet", b"<html>Not Found</html>");
        assert!(bracketed_by(&page, &PARQUET_MAGIC, "a parquet").is_err());
        let head_only = write(&dir, "head.parquet", b"PAR1PAR1");
        assert!(bracketed_by(&head_only, &PARQUET_MAGIC, "a parquet").is_ok());
        let short = write(&dir, "short.parquet", b"PAR1");
        assert!(bracketed_by(&short, &PARQUET_MAGIC, "a parquet").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
