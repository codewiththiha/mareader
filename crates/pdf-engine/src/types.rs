//! Serde types that mirror the pdf.js engine's return shapes.
//!
//! The engine resolves `{ok:true, ...}` objects whose field names are camelCase
//! (they come straight from JS). These structs are deserialized via
//! serde_wasm_bindgen after the `ok` flag is checked in `crate::api::resolve`.
//!
//! CONTRACT: field names are the wire contract with pdfEngine.js.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSize {
    pub width: f64,
    pub height: f64,
}

/// One flattened chapter, exactly as `pdfEngine.js` resolves it.
///
/// The type is `pdf-core`'s rather than the engine's: the entries cross this
/// boundary on the wire and land in the reader's outline with one conversion in
/// between, and a wire shape duplicated on both sides of that conversion is how
/// a field rename becomes a silent `depth: 0` instead of a compile error.
pub use pdf_core::outline::OutlineEntry;

/// `{ok:true, numPages, title, author, outline, page1Size, pageHeights}` — engine.open().
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenResult {
    pub num_pages: u32,
    pub title: Option<String>,
    pub author: Option<String>,
    pub outline: Vec<OutlineEntry>,
    pub page1_size: PageSize,
    /// Intrinsic (scale-1) height of every page, in document order.
    ///
    /// Empty only for engines predating this field; callers fall back to
    /// `page1_size.height` for every page in that case.
    #[serde(default)]
    pub page_heights: Vec<f64>,
    /// Intrinsic (scale-1) width of every page, in document order.
    ///
    /// Fit / shrink-to-fit must use the page the reader is LOOKING AT, not
    /// page 1: a landscape plate in an otherwise-A4 book is cropped if the
    /// ceiling is computed from the letter pages around it. Empty only for
    /// engines predating this field; callers fall back to `page1_size.width`.
    #[serde(default)]
    pub page_widths: Vec<f64>,
}

/// `{ok:true, width, height, scale}` — engine.renderPage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

/// `{ok:true, width, height, scale}` — engine.renderThumb.
///
/// This used to carry a `cached` flag saying the engine had blitted an
/// already-rendered bitmap synchronously, so the cell could skip its loading
/// skeleton. A flag that arrives with the promise is too late for that: the
/// cell's first frame is composited before the render resolves. What actually
/// removed the flicker is `has_thumb` — the same question, asked synchronously
/// while the cell is still being built (`thumbnails/thumbnail_cell.rs`) — and
/// the flag was left behind unread, still advertised as load-bearing by its own
/// doc comment. The blit is still synchronous; only the reporting of it is gone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbResult {
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

/// `{ok:true, dataUrl, width, height}` — engine.coverDataUrl.
///
/// Page 1 of the current document rendered to a small JPEG, for the library
/// shelf's book-cover art. `width`/`height` are CSS px so the shelf can keep
/// the cover's real proportions (portrait vs landscape).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverResult {
    pub data_url: String,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocStatus {
    Idle,
    Opening,
    Ready,
    Error,
}

/// `{ok:true, page, width, height, data}` — the raw page frame the paper
/// pipeline runs on: the raster downscaled to a ≤96px long edge, `data`
/// its RGBA pixels (`width * height * 4`). Produced by `takePaperFrame`
/// (a live render's stash) and `samplePaperPage` (an offscreen sample).
///
/// The pixels travel as a typed array rather than JSON, so this shape is
/// parsed by hand in `crate::api::paper::parse_frame`, not through serde.
pub struct PaperFrame {
    pub page: u32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}
