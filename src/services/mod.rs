//! Application services: operations that span state, engine and storage —
//! the "what the app does" layer under the UI.

pub mod ai;
pub mod document;
pub mod tauri_listen;

pub use tauri_listen::tauri_listen;
