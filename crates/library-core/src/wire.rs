//! The wire contract between the shell's filesystem commands and the
//! frontend.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportPhase {
    /// Walking a folder. The total is not known yet, which the dock reads as
    /// indeterminate.
    Scan,
    Copy,
}

/// One progress beat on the shell's `library://progress` channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    /// The import run this beat belongs to.
    pub task: String,
    pub phase: ImportPhase,
    pub done: u32,
    /// `0` during a scan; the request count during a copy.
    pub total: u32,
    pub name: String,
}

/// One row per path asked about, in the order asked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathCheck {
    pub path: String,
    /// False for a path that is gone or refused by the shell's gate.
    pub exists: bool,
    pub size: u64,
    pub mtime_ms: u64,
    pub head_hash: u32,
}

impl PathCheck {
    /// The measurement as a fingerprint, or `None` unresolved.
    pub fn fingerprint(&self) -> Option<crate::book::Fingerprint> {
        self.exists.then_some(crate::book::Fingerprint {
            size: self.size,
            mtime_ms: self.mtime_ms,
            head_hash: self.head_hash,
        })
    }
}

/// One file to act on: its address and the book's id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookFileRequest {
    pub from: String,
    pub id: String,
}

/// What a relocation produced: one row per request, plus the store root.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelocateResult {
    /// The app's store root, or empty when the shell has none.
    pub root: String,
    pub results: Vec<StoreResult>,
}

/// A failure is per-file: one locked file does not sink the batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreResult {
    pub id: String,
    pub src: String,
    pub store: String,
    pub error: Option<String>,
    /// The copy's own measurement, taken by the pass that stamped it.
    #[serde(default)]
    pub measured: Option<crate::book::Fingerprint>,
}

impl StoreResult {
    pub fn is_ok(&self) -> bool {
        self.error.is_none() && !self.store.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_beat_crosses_the_wire_in_camel_case() {
        let beat = ImportProgress {
            task: "t1".into(),
            phase: ImportPhase::Copy,
            done: 12,
            total: 48,
            name: "dune.pdf".into(),
        };
        let json = serde_json::to_string(&beat).unwrap();
        assert!(json.contains("\"phase\":\"copy\""), "{json}");
        // No snake_case keys: Tauri's IPC speaks camelCase.
        assert!(!json.contains('_'), "{json}");
        let back: ImportProgress = serde_json::from_str(&json).unwrap();
        assert_eq!(back, beat);
    }

    #[test]
    fn a_path_check_only_becomes_a_fingerprint_when_it_resolved() {
        let live = PathCheck {
            path: "/books/a.pdf".into(),
            exists: true,
            size: 10,
            mtime_ms: 20,
            head_hash: 30,
        };
        assert_eq!(
            live.fingerprint(),
            Some(crate::book::Fingerprint {
                size: 10,
                mtime_ms: 20,
                head_hash: 30
            })
        );
        let gone = PathCheck {
            exists: false,
            ..live
        };
        assert_eq!(gone.fingerprint(), None);
    }

    #[test]
    fn the_path_check_parses_the_shape_the_shell_emits() {
        let check: PathCheck = serde_json::from_str(
            r#"{"path":"/a.pdf","exists":true,"size":1,"mtimeMs":2,"headHash":3}"#,
        )
        .unwrap();
        assert_eq!(check.mtime_ms, 2);
        assert_eq!(check.head_hash, 3);
    }

    #[test]
    fn a_relocation_crosses_the_wire_in_camel_case_and_comes_back_in_order() {
        let requests = [BookFileRequest {
            from: "/app/Library/pdf/dune_ab12.pdf".into(),
            id: "ab12".into(),
        }];
        let json = serde_json::to_string(&requests).unwrap();
        assert!(json.contains("\"from\""), "{json}");
        assert!(json.contains("\"id\""), "{json}");
        // The check names the keys it wants.
        assert!(!json.contains("\"from_\""), "no snake_case keys: {json}");
        assert!(!json.contains("\"_id\""), "no snake_case keys: {json}");
        assert!(json.starts_with("[{\"from\""), "{json}");
        let back: Vec<BookFileRequest> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, requests);

        // The answer carries the store root beside the rows.
        let answer: RelocateResult = serde_json::from_str(
            r#"{"root":"/app/Library","results":[
                {"id":"ab12","src":"/app/Library/pdf/dune_ab12.pdf",
                 "store":"/app/Library/items/ab12/source.pdf","error":null}]}"#,
        )
        .unwrap();
        assert_eq!(answer.root, "/app/Library");
        assert_eq!(answer.results.len(), 1);
        assert!(answer.results[0].is_ok());
        assert_eq!(answer.results[0].measured, None);
        let none: RelocateResult = serde_json::from_str(r#"{"root":"","results":[]}"#).unwrap();
        assert!(none.root.is_empty() && none.results.is_empty());
    }

    #[test]
    fn a_store_result_is_only_ok_when_it_has_an_address() {
        let ok = StoreResult {
            id: "b1".into(),
            src: "/downloads/a.pdf".into(),
            store: "/app/Library/pdf/a_b1.pdf".into(),
            error: None,
            measured: None,
        };
        assert!(ok.is_ok());
        let failed = StoreResult {
            store: String::new(),
            error: Some("locked".into()),
            ..ok.clone()
        };
        assert!(!failed.is_ok());
        let empty = StoreResult {
            id: "b1".into(),
            src: "/downloads/a.pdf".into(),
            store: String::new(),
            error: None,
            measured: None,
        };
        assert!(!empty.is_ok());
    }

    #[test]
    fn a_copys_own_measurement_crosses_with_it() {
        // One pass stamps the copy and reads its head.
        let landed: StoreResult = serde_json::from_str(
            r#"{"id":"b1","src":"/downloads/a.pdf",
                "store":"/app/Library/items/b1/source.pdf","error":null,
                "measured":{"size":10,"mtimeMs":20,"headHash":30}}"#,
        )
        .unwrap();
        assert_eq!(
            landed.measured,
            Some(crate::book::Fingerprint {
                size: 10,
                mtime_ms: 20,
                head_hash: 30
            })
        );
        // An older row answers `None`, not an error.
        let legacy: StoreResult = serde_json::from_str(
            r#"{"id":"b1","src":"/downloads/a.pdf",
                "store":"/app/Library/items/b1/source.pdf","error":null}"#,
        )
        .unwrap();
        assert_eq!(legacy.measured, None);
    }
}
