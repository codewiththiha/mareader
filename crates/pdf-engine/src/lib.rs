//! WASM bridge to the pdf.js engine (`window.PDFReader`).
//!
//! Four layers, one job: `bridge` declares the `window.PDFReader` wasm-bindgen
//! externs (the ONLY place they live — typed, so a mount/render call
//! allocates no payload objects), `types` mirrors the engine's return shapes,
//! `api` provides typed `Result`-returning wrappers for the rest of the app
//! (one focused module per surface: document, render, search, paper, dialog,
//! theme), and `paper` is the paper session state machine. The `window.__TAURI__` externs this crate still touches (the
//! file dialog) come from the `tauri-bridge` crate, which owns that surface
//! so no format crate does.
//!
//! `bridge` is private: callers go through `api`, except for the raw engine
//! probes the app legitimately needs directly (engine version,
//! `window.PDFReader` presence), which are re-exported here.

mod bridge;
mod host;

pub mod api;
pub mod paper;
pub mod types;

pub use host::{has_pdf_reader, version};
