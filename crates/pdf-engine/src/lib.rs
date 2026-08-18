//! WASM bridge to the pdf.js engine (`window.PDFReader`).
//!
//! Three layers, one job: `bridge` declares the wasm-bindgen externs (the
//! ONLY place externs live), `types` mirrors the engine's return shapes, and
//! `api` provides typed `Result`-returning wrappers for the rest of the app.

pub mod api;
pub mod bridge;
pub mod types;
