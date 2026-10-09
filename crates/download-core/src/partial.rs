//! The `.part` body and the sidecar that makes resuming it safe.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The partial a transfer writes into; the destination is only ever adopted.
pub fn partial_of(dest: &Path) -> PathBuf {
    suffixed(dest, "part")
}

/// Where the serving mirror's validator is kept beside the partial.
pub fn sidecar_of(dest: &Path) -> PathBuf {
    suffixed(dest, "dmeta")
}

fn suffixed(dest: &Path, suffix: &str) -> PathBuf {
    let mut name = dest
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(".");
    name.push(suffix);
    dest.with_file_name(name)
}

/// What the mirror that wrote the partial said about the body.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Sidecar {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// The mirror index that wrote these bytes.
    pub source: u32,
    pub total: Option<u64>,
}

impl Sidecar {
    /// The `If-Range` validator; `None` means the partial is not provable.
    pub fn validator(&self) -> Option<&str> {
        // A weak etag compares payloads; If-Range needs a strong one.
        self.etag
            .as_deref()
            .filter(|etag| !etag.starts_with("W/"))
            .or(self.last_modified.as_deref())
    }

    /// The sidecar of `dest`; absent, unreadable or corrupt is all `None`.
    pub fn load(dest: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(sidecar_of(dest)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save(&self, dest: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string(self).unwrap_or_default();
        std::fs::write(sidecar_of(dest), text)
    }
}

/// Drop the partial and its sidecar: neither may outlive the other.
pub fn discard(dest: &Path) {
    let _ = std::fs::remove_file(partial_of(dest));
    let _ = std::fs::remove_file(sidecar_of(dest));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helpers_name_beside_the_destination() {
        let dest = Path::new("/data/cefr/cefr.parquet");
        assert_eq!(
            partial_of(dest),
            PathBuf::from("/data/cefr/cefr.parquet.part")
        );
        assert_eq!(
            sidecar_of(dest),
            PathBuf::from("/data/cefr/cefr.parquet.dmeta")
        );
    }

    #[test]
    fn a_strong_etag_wins_and_a_weak_one_is_skipped() {
        let date = "Wed, 21 Oct 2015 07:28:00 GMT";
        let strong = Sidecar {
            etag: Some("\"abc\"".into()),
            last_modified: Some(date.into()),
            ..Default::default()
        };
        assert_eq!(strong.validator(), Some("\"abc\""));
        let weak = Sidecar {
            etag: Some("W/\"abc\"".into()),
            ..strong.clone()
        };
        assert_eq!(weak.validator(), Some(date));
        assert_eq!(Sidecar::default().validator(), None);
    }

    #[test]
    fn a_sidecar_round_trips_and_a_corrupt_one_is_none() {
        let dir = std::env::temp_dir().join(format!("dl_sidecar_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("body.bin");
        assert!(Sidecar::load(&dest).is_none());

        let written = Sidecar {
            etag: Some("\"v1\"".into()),
            last_modified: None,
            source: 2,
            total: Some(4096),
        };
        written.save(&dest).unwrap();
        assert_eq!(Sidecar::load(&dest), Some(written));

        std::fs::write(sidecar_of(&dest), "not json").unwrap();
        assert!(Sidecar::load(&dest).is_none());

        std::fs::write(partial_of(&dest), b"half a body").unwrap();
        discard(&dest);
        assert!(!partial_of(&dest).exists());
        assert!(!sidecar_of(&dest).exists());
        let _ = std::fs::remove_dir(&dir);
    }
}
