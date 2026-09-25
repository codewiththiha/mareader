//! The serialized boundary between the Shell and the runtimes.
//!
//! Everything that crosses a runtime edge goes through here as data — the
//! phase's rule that "library receives `&AppState`" is exactly what may not
//! happen. The runtimes see the Shell as [`ShellApi`] (a set of typed
//! commands); the Shell sees a runtime as its module exports (start / dispose
//! / command), which carry JSON payloads defined by the same types.

use reader_core::settings::Settings;
use serde::{Deserialize, Serialize};

/// The minimal launch descriptor the Shell hands a starting reader runtime
/// (§13): identity and address, plus the resume point the LIBRARY computed
/// from its own state — the reader does not reach into library state to find
/// where the reader left off.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchDocument {
    /// The library row the open was named by, when it was named by one: the
    /// key the highlights live under and the row a read point belongs to.
    #[serde(default)]
    pub book_id: Option<String>,
    pub path: String,
    /// Where to resume: the page (clamped by the reader's own open rules) and
    /// the continuous stream's fractional position, when the format keeps one.
    #[serde(default)]
    pub resume_page: u32,
    #[serde(default)]
    pub saved_fraction: Option<f64>,
    /// The test hook's `?blend=1`: the appearance write the menu itself would
    /// make, applied at session start. Inert in the packaged app.
    #[serde(default)]
    pub blend_override: bool,
    /// The book's cover, resolved by the library while it was still active,
    /// so the reader's document-info panel never needs the cover map.
    #[serde(default)]
    pub cover_data_url: Option<String>,
    /// The library's display name for the row this open belongs to, so the
    /// title bar never falls back to an address stem for a stored book.
    #[serde(default)]
    pub display_name: Option<String>,
}

/// A durable write-through: where the reader got to in the open document.
/// The reader sends this on its progress debounce and — unconditionally —
/// on close/dispose; the Shell applies it to the persisted library blob
/// (`persist data ≠ retain live object`, §15).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReadPoint {
    #[serde(default)]
    pub book_id: Option<String>,
    pub path: String,
    pub page: u32,
    #[serde(default)]
    pub num_pages: u32,
    #[serde(default)]
    pub fraction: Option<f64>,
    /// The document's title/author when the library may have to mint a row
    /// for a file it never knew (a drop-open of an unshelved file).
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
}

/// The document status as the reader session reports it. The Shell keeps the
/// URL ⇄ document semantics the old `RouteSync` effect owned: Ready ⇒ the
/// reader route is real, Idle ⇒ the shelf. Serialized as plain strings so
/// the two sides need no shared enum crate beyond the pdf-engine types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DocStatusReport {
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
}

/// The typed command surface a runtime calls the Shell through. One
/// implementation per deployment: the hosted frame (`ApiHandle::Frame`, the
/// boundary wire over the frame's port) and the standalone substitute
/// (direct storage writes, no navigation to own).
pub trait ShellApi {
    /// Library → Shell: open this document. The Shell navigates to `/reader`
    /// and starts the reader runtime with this descriptor.
    fn open_document(&self, launch: &LaunchDocument);
    /// Reader → Shell: the reader session is handing control back. The Shell
    /// disposes the reader runtime and starts the library.
    fn navigate_library(&self);
    /// Reader → Shell: a durable read-point write (debounce or flush).
    fn read_point(&self, point: &ReadPoint);
    /// Runtime → Shell: persist the settings blob (the Shell owns the key).
    fn save_settings(&self, settings: &Settings);
    /// Library → Shell: persist the library blob (the Shell owns the key).
    fn save_library(&self, blob: &library_core::blob::LibraryBlob);
    /// Library → Shell: persist the cover cache (the Shell owns the key).
    fn save_covers(&self, covers: &crate::covers::CoverMap);
    /// Reader → Shell: one generated cover (the reader owns the engine that
    /// bakes it; the Shell owns the persisted map it lands in).
    fn save_cover(&self, path: &str, image: &crate::covers::CoverImage);
    /// Library → Shell: bake page 1 of the book at `path` into cover art.
    ///
    /// A COMMAND, not a call: the library artifact owns no engine, so the
    /// Shell bakes it for the shelf's import queue. The answer comes back as
    /// the session command `coverBaked { path, image }` — `image: None` when
    /// the bake failed — and only ever into the LIVE session's generation: a
    /// bake that outlived its frame is dropped by the Shell, never misfiled
    /// into a replacement session's state.
    fn bake_cover(&self, path: &str);
    /// Reader → Shell: the document status changed (URL policy + probe).
    fn doc_status(&self, report: &DocStatusReport);
    /// Reader → Shell: the diagnostics digest (the Shell's probe merges it).
    fn publish_digest(&self, json: String);
    /// Reader → Shell: reload the webview (the reader has already flushed).
    fn reload(&self);
    /// Reader → Shell: resolve an in-session open (drop, dialog) against the
    /// persisted library: the row id, resume point, cover and display name.
    /// A query, answered synchronously from the browser store — the reader
    /// never holds library state, only the descriptor this returns.
    fn resolve_launch(&self, path: &str) -> Option<LaunchDocument>;
}

/// A no-Shell default for host tests: every command is recorded, nothing
/// more. The runtimes' host tests install this so the boundary is exercised
/// even off-wasm.
#[derive(Default)]
pub struct RecordApi {
    pub launches: std::cell::RefCell<Vec<LaunchDocument>>,
    pub read_points: std::cell::RefCell<Vec<ReadPoint>>,
    pub library_calls: std::cell::RefCell<u32>,
    pub settings_saves: std::cell::RefCell<u32>,
    pub bakes: std::cell::RefCell<Vec<String>>,
}

impl ShellApi for RecordApi {
    fn open_document(&self, launch: &LaunchDocument) {
        self.launches.borrow_mut().push(launch.clone());
    }
    fn navigate_library(&self) {}
    fn read_point(&self, point: &ReadPoint) {
        self.read_points.borrow_mut().push(point.clone());
    }
    fn save_settings(&self, _settings: &Settings) {
        *self.settings_saves.borrow_mut() += 1;
    }
    fn save_library(&self, _blob: &library_core::blob::LibraryBlob) {
        *self.library_calls.borrow_mut() += 1;
    }
    fn save_covers(&self, _covers: &crate::covers::CoverMap) {}
    fn save_cover(&self, _path: &str, _image: &crate::covers::CoverImage) {}
    fn bake_cover(&self, path: &str) {
        self.bakes.borrow_mut().push(path.to_string());
    }
    fn doc_status(&self, _report: &DocStatusReport) {}
    fn publish_digest(&self, _json: String) {}
    fn reload(&self) {}
    fn resolve_launch(&self, _path: &str) -> Option<LaunchDocument> {
        None
    }
}
