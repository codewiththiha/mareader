//! The wire contract between the shell's filesystem commands and the
//! frontend that drives them. Both sides depend on this crate, so these types
//! are declared once rather than mirrored.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportPhase {
    /// Walking a folder. The total is not known yet, which the dock reads as
    /// indeterminate.
    Scan,
    Copy,
}

/// One progress beat, emitted on the shell's `library://progress` channel
/// and re-broadcast as a window event by `src/services/library/mod.rs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    /// The import run this beat belongs to, so two runs in flight never mix
    /// their counts.
    pub task: String,
    pub phase: ImportPhase,
    pub done: u32,
    /// `0` during a scan (the count is unknown until the walk ends); the
    /// request count during a copy.
    pub total: u32,
    pub name: String,
}

/// One row per path asked about, in the order asked, so the caller can zip
/// the answer against its own list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathCheck {
    pub path: String,
    /// False for a path that is gone, unreadable, a directory, or refused by
    /// the shell's document gate.
    pub exists: bool,
    pub size: u64,
    pub mtime_ms: u64,
    pub head_hash: u32,
}

impl PathCheck {
    /// The measurement as a fingerprint, or `None` when the address did not
    /// resolve — the caller marks that book `missing` rather than re-stamping
    /// it with zeros.
    pub fn fingerprint(&self) -> Option<crate::book::Fingerprint> {
        self.exists.then_some(crate::book::Fingerprint {
            size: self.size,
            mtime_ms: self.mtime_ms,
            head_hash: self.head_hash,
        })
    }
}

/// One file the library is about to act on: the address the bytes come from
/// and the book's id, which becomes part of the stored name so two books with
/// the same title cannot collide. One shape for both commands that take it —
/// a copy's source and a relocation's current address are the same question —
/// with the field order the pre-unification shapes already spelled, so a
/// relocation's wire bytes are unchanged.
///
/// For a relocation, `from` is the copy's address in the old flat store
/// (`<root>/<format>/<stem>_<id>.<ext>`) that [`crate::store`]'s item layout
/// (`<root>/items/<id>/source.<ext>`) replaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookFileRequest {
    pub from: String,
    pub id: String,
}

/// What a relocation pass produced: one row per request, plus the store root
/// the shell moved them inside. The root rides along because the frontend
/// cannot compute `<app_data_dir>` and needs it to tell a copy still in the
/// old bucket from one already in its item folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelocateResult {
    /// The app's store root, `<app_data_dir>/Library`, or empty when the shell has none.
    pub root: String,
    pub results: Vec<StoreResult>,
}

/// A failure is per-file rather than per-batch: a folder with one locked
/// file in it still imports the other ninety-nine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreResult {
    pub id: String,
    pub src: String,
    pub store: String,
    pub error: Option<String>,
    /// The copy's own measurement, taken by the pass that stamped it: the
    /// backend reads the head anyway, so answering costs no second trip and
    /// the row never wears the source's identity (which stays free for the
    /// folder that reads it). `None` for a failed copy, and for a relocation —
    /// the bytes did not change.
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
        let requests = [BookFileRequest {
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
        let back: Vec<BookFileRequest> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, requests);

        // The answer carries the root beside the rows, because the frontend
        // cannot compute `<app_data_dir>` itself and needs it to recognise a copy
        // that has not moved yet. `measured` is absent from a relocation's rows:
        // the bytes did not change, so the identity the row already carries is
        // the truth, and `#[serde(default)]` reads the gap as `None`.
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
        // The shell stamps a copy and reads its head in one pass; the row lands with
        // the copy's identity rather than the source file's, and no second verify trip
        // follows the batch home.
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
        // A row written before the field existed answers `None`, not an error: the
        // pending flag the startup sweep finishes is exactly what a missing
        // measurement means.
        let legacy: StoreResult = serde_json::from_str(
            r#"{"id":"b1","src":"/downloads/a.pdf",
                "store":"/app/Library/items/b1/source.pdf","error":null}"#,
        )
        .unwrap();
        assert_eq!(legacy.measured, None);
    }
}
