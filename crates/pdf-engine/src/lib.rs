//! WASM bridge to the pdf.js engine (`window.PDFReader`).
mod bridge;

pub mod api;
pub mod backdrop;
pub mod session;
pub mod types;

pub use session::{PageElements, PdfSession};
