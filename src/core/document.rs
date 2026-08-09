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

/// `{ok:true, numPages, title, author, outline, page1Size}` — engine.open().
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenResult {
    pub num_pages: u32,
    pub title: Option<String>,
    pub author: Option<String>,
    pub outline: Vec<OutlineNode>,
    pub page1_size: PageSize,
}

/// `{ok:true, width, height, scale}` — engine.renderPage / renderPages / updatePage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocStatus {
    Idle,
    Opening,
    Ready,
    Error,
}
