//! Serde types that mirror the pdf.js engine's return shapes.
//!
//! The engine resolves `{ok:true, ...}` objects whose field names are camelCase
//! (they come straight from JS). These structs are deserialized via
//! serde_wasm_bindgen after the `ok` flag is checked in crate::api::engine.
//!
//! CONTRACT: field names are the wire contract with pdfEngine.js.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSize {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutlineNode {
    pub title: String,
    pub page: u32,
    pub depth: u32,
}

/// `{ok:true, numPages, title, author, outline, page1Size, pageHeights}` — engine.open().
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenResult {
    pub num_pages: u32,
    pub title: Option<String>,
    pub author: Option<String>,
    pub outline: Vec<OutlineNode>,
    pub page1_size: PageSize,
    /// Intrinsic (scale-1) height of every page, in document order.
    ///
    /// Empty only for engines predating this field; callers fall back to
    /// `page1_size.height` for every page in that case.
    #[serde(default)]
    pub page_heights: Vec<f64>,
}

/// `{ok:true, width, height, scale}` — engine.renderPage / renderPages / updatePage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

/// `{ok:true, width, height, scale, cached}` — engine.renderThumb.
///
/// `cached` is the load-bearing field: `true` means the engine blitted an
/// already-rendered bitmap into the canvas SYNCHRONOUSLY (before the promise
/// ever suspended), so the thumbnail is painted on the very first frame the
/// cell is mounted. The cell uses it to skip its loading skeleton entirely —
/// covering an already-painted thumbnail and then crossfading the cover away
/// is precisely the per-row flicker seen when scrolling a virtualized grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbResult {
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    #[serde(default)]
    pub cached: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocStatus {
    Idle,
    Opening,
    Ready,
    Error,
}
