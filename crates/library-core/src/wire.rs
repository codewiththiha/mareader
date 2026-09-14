//! The wire contract between the shell's filesystem commands and the frontend
//! that drives them. Both sides depend on this crate, so these types are
//! declared ONCE rather than mirrored (the AI chunk stream's envelope is written
//! twice and held together by a contract test).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportPhase {
    /// Walking a folder. The total is not known yet, which is what the dock reads as "indeterminate".
    Scan,
    Copy,
}

/// One progress beat, emitted on the shell's `library://progress` channel and re-broadcast as a window event by `src/services/library/mod.rs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    /// The import run this beat belongs to, so two runs in flight never have their counts mixed.
    pub task: String,
    pub phase: ImportPhase,
    pub done: u32,
    /// `0` during a scan (the count is not known until the walk ends), the request count during a copy.
    pub total: u32,
    pub name: String,
}

/// One row per path asked about, in the order asked, so the caller can zip the answer against its own list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathCheck {
    pub path: String,
    /// False for a path that is gone, unreadable, a directory, or refused by the shell's document gate.
    pub exists: bool,
    pub size: u64,
    pub mtime_ms: u64,
    pub head_hash: u32,
}

impl PathCheck {
    /// The measurement as a fingerprint, or `None` when the address did not resolve:
    /// the caller marks that book `missing` rather than re-stamping it with zeros.
    pub fn fingerprint(&self) -> Option<crate::book::Fingerprint> {
        self.exists.then_some(crate::book::Fingerprint {
            size: self.size,
            mtime_ms: self.mtime_ms,
            head_hash: self.head_hash,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreRequest {
    pub path: String,
    /// The book's id, which becomes part of the stored name so two books with the same title cannot collide.
    pub id: String,
}

/// One stored copy to move into its own item folder, for `relocate_stored`. The
/// old flat store named a copy after the file it came from
/// (`<root>/<format>/<stem>_<id>.<ext>`); [`crate::store`] names it after the
/// book (`<root>/items/<id>/source.<ext>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelocateRequest {
    pub from: String,
    pub id: String,
}

/// What a relocation pass produced: one row per request, plus the store root the
/// shell moved them inside. The root rides along because the frontend cannot
/// compute it — `<app_data_dir>` is the shell's answer — and it needs it to tell
/// a copy still in the old bucket from one already in its item folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelocateResult {
    /// The app's store root, `<app_data_dir>/Library`, or empty when the shell has none.
    pub root: String,
    pub results: Vec<StoreResult>,
}

/// A failure is per-file rather than per-batch: a folder with one locked file in it should still import the other ninety-nine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreResult {
    pub id: String,
    pub src: String,
    pub store: String,
    pub error: Option<String>,
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
        // No snake_case keys: the JS side of Tauri's IPC speaks camelCase, and
        // a mismatched name deserialises as a default rather than as an error.
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
        let requests = [RelocateRequest {
            from: "/app/Library/pdf/dune_ab12.pdf".into(),
            id: "ab12".into(),
        }];
        let json = serde_json::to_string(&requests).unwrap();
        assert!(json.contains("\"from\""), "{json}");
        assert!(json.contains("\"id\""), "{json}");
        // The keys are the contract; the values are paths a reader owns, which
        // carry underscores of their own, so the check names the keys it wants.
        assert!(!json.contains("\"from_\""), "no snake_case keys: {json}");
        assert!(!json.contains("\"_id\""), "no snake_case keys: {json}");
        assert!(json.starts_with("[{\"from\""), "{json}");
        let back: Vec<RelocateRequest> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, requests);

        // The answer carries the root beside the rows, because the frontend
        // cannot compute `<app_data_dir>` itself and needs it to recognise a copy
        // that has not moved yet.
        let answer: RelocateResult = serde_json::from_str(
            r#"{"root":"/app/Library","results":[
                {"id":"ab12","src":"/app/Library/pdf/dune_ab12.pdf",
                 "store":"/app/Library/items/ab12/source.pdf","error":null}]}"#,
        )
        .unwrap();
        assert_eq!(answer.root, "/app/Library");
        assert_eq!(answer.results.len(), 1);
        assert!(answer.results[0].is_ok());
        let none: RelocateResult =
            serde_json::from_str(r#"{"root":"","results":[]}"#).unwrap();
        assert!(none.root.is_empty() && none.results.is_empty());
    }

    #[test]
    fn a_store_result_is_only_ok_when_it_has_an_address() {
        let ok = StoreResult {
            id: "b1".into(),
            src: "/downloads/a.pdf".into(),
            store: "/app/Library/pdf/a_b1.pdf".into(),
            error: None,
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
        };
        assert!(!empty.is_ok());
    }
}
