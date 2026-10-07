//! Serde types mirroring the engine's return shapes.
use serde::{Deserialize, Serialize};

pub use reader_core::document::{DocStatus, PageSize};

/// `{ok, width, height}`: one page's intrinsic box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSizeResult {
    pub width: f64,
    pub height: f64,
}

/// One flattened chapter, exactly as the engine resolves it.
pub use pdf_core::outline::OutlineEntry;

/// `{ok, numPages, title, author, fingerprint, outline, ...}`: engine.open().
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenResult {
    pub num_pages: u32,
    pub title: Option<String>,
    pub author: Option<String>,
    /// The document's permanent content fingerprint, the index's key.
    #[serde(default)]
    pub fingerprint: Option<String>,
    pub outline: Vec<OutlineEntry>,
    pub page1_size: PageSize,
    /// Intrinsic height of every page, in document order.
    #[serde(default)]
    pub page_heights: Vec<f64>,
    /// Intrinsic width of every page, in document order.
    #[serde(default)]
    pub page_widths: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

/// The old `cached` flag, left unread; `has_thumb` replaced it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbResult {
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

/// `{ok, dataUrl, width, height}`: engine.coverDataUrl.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverResult {
    pub data_url: String,
    pub width: f64,
    pub height: f64,
}

/// `{ok, page, width, height, data}`: the raw page frame.
pub struct PaperFrame {
    pub page: u32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}
