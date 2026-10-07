//! WASM bridge to the pdf.js engine (`window.PDFReader`).
//!
//! Five layers, one job: `bridge` declares the wasm-bindgen externs (the ONLY
//! place they live — typed, so a mount/render call allocates no payload
//! objects), `types` mirrors the engine's return shapes, `session` is the
//! [`PdfSession`] — the one owner of an open document and the only way to
//! reach one (open, pages, renders, thumbnails, prefetch, search, paper,
//! teardown) — `api` holds the realm-level calls that name no document (the
//! appearance broadcast, diagnostics) and the shared envelope parser, and
//! `backdrop` is each session's paper state machine — the `pdf-paper`
//! crate's brain, wired to the engine's eyes. The `window.__TAURI__` externs this crate touches come
//! from the `tauri-bridge` crate, which owns that surface so no format crate
//! does — and the native open-file dialog is not an engine surface at all
//! anymore (it lives in `app-chrome`).
//!
//! `bridge` is private: callers go through `session` and `api`.

mod bridge;

pub mod api;
pub mod backdrop;
pub mod session;
pub mod types;

pub use session::{PageElements, PdfSession};
