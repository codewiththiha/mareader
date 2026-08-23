//! Host/window accessors re-exported at the crate root.
//!
//! These don't follow the normal `api` path on purpose: they expose raw
//! probes/surfaces (engine version, Tauri window/event handles) that the
//! app calls directly, and the bridge declares them as externs it does not
//! itself use — hence the `#[allow(dead_code)]` on the extern. Keeping the
//! re-export here makes that contract visible instead of buried in `lib.rs`.

pub use crate::bridge::{has_pdf_reader, has_tauri, listen, tauri_get_current_window, version};
