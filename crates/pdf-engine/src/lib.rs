//! WASM bridge to the pdf.js engine (`window.PDFReader`).
//!
//! Three layers, one job: `bridge` declares the wasm-bindgen externs (the
//! ONLY place externs live), `types` mirrors the engine's return shapes, and
//! `api` provides typed `Result`-returning wrappers for the rest of the app.
//!
//! `bridge` is private: callers go through `api`, except for the few raw
//! probes/surfaces the app legitimately needs directly (engine version,
//! Tauri window/event access), which are re-exported here.

mod bridge;
mod host;

pub mod api;
pub mod paper;
pub mod types;

pub use host::{has_pdf_reader, has_tauri, listen, tauri_get_current_window, version};
