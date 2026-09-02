//! Engine accessors re-exported at the crate root.
//!
//! These don't follow the normal `api` path on purpose: they expose the raw
//! engine probe (version, `window.PDFReader` presence) that the app calls
//! directly, even though nothing inside this crate uses them. Keeping the
//! re-export here makes that contract visible instead of buried in `lib.rs`.
//! (The Tauri probes/surfaces are not here — they live in the
//! `tauri-bridge` crate.)

pub use crate::bridge::{has_pdf_reader, version};
