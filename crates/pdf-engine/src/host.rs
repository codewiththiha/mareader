//! Host/window accessors re-exported at the crate root.
//!
//! These don't follow the normal `api` path on purpose: they expose raw
//! probes/surfaces (engine version, Tauri window/event handles) that the
//! app calls directly, even though nothing inside this crate uses them.
//! Keeping the re-export here makes that contract visible instead of buried
//! in `lib.rs`.

pub use crate::bridge::{has_pdf_reader, has_tauri, listen, tauri_get_current_window, version};
