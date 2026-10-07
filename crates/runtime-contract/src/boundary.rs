//! The serialized boundary between the Shell and the runtimes.
use reader_core::settings::Settings;
use serde::{Deserialize, Serialize};

/// The minimal launch descriptor the Shell hands a starting reader.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchDocument {
    /// The library row the open was named by, when it was.
    #[serde(default)]
    pub book_id: Option<String>,
    pub path: String,
    /// Where to resume: the page and the stream's fraction.
    #[serde(default)]
    pub resume_page: u32,
    #[serde(default)]
    pub saved_fraction: Option<f64>,
    /// The test hook's `?blend=1` appearance write.
    #[serde(default)]
    pub blend_override: bool,
    /// The book's cover, resolved by the library.
    #[serde(default)]
    pub cover_data_url: Option<String>,
    /// The library's display name for this open's row.
    #[serde(default)]
    pub display_name: Option<String>,
}

/// A durable write-through: where the reader got to.
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
    /// The document's title and author, when a row may be minted.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
}

/// The document status as the reader session reports it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DocStatusReport {
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
}

/// The typed command surface a runtime calls the Shell through.
pub trait ShellApi {
    /// Library to Shell: open this document.
    fn open_document(&self, launch: &LaunchDocument);
    /// Reader to Shell: the session is handing control back.
    fn navigate_library(&self);
    /// Reader → Shell: a durable read-point write (debounce or flush).
    fn read_point(&self, point: &ReadPoint);
    /// Runtime → Shell: persist the settings blob (the Shell owns the key).
    fn save_settings(&self, settings: &Settings);
    /// Reader to Shell: one generated cover.
    fn save_cover(&self, path: &str, image: &crate::covers::CoverImage);
    /// Reader to Shell: persist one document's gloss marks.
    fn save_gloss(&self, key: &str, marks: String);
    /// Library to Shell: bake page 1 of the book into cover art.
    fn bake_cover(&self, path: &str);
    /// Reader → Shell: the document status changed (URL policy + probe).
    fn doc_status(&self, report: &DocStatusReport);
    /// Reader → Shell: the diagnostics digest (the Shell's probe merges it).
    fn publish_digest(&self, json: String);
}
