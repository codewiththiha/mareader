//! Application services: operations that span state, engine and storage —
//! the "what the app does" layer under the UI.

pub mod ai;
pub mod document;
#[cfg(feature = "library")]
pub mod library;
pub mod reload;
pub mod tauri_listen;
pub mod window;

pub use tauri_listen::tauri_listen;
