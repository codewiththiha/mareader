//! WASM bridge to the pdf.js engine (`window.PDFReader`).
//!
//! Four layers, one job: `bridge` declares the wasm-bindgen externs (the
//! ONLY place externs live — typed, so a mount/render call allocates no
//! payload objects), `types` mirrors the engine's return shapes, `api`
//! provides typed `Result`-returning wrappers for the rest of the app (one
//! focused module per surface: document, render, search, paper, dialog,
//! theme, window), and `paper` is the paper session state machine.
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
