//! Reusable Leptos PDF viewer: page canvases, continuous/single views,
//! thumbnails, outline, search overlay, and their effects. Depends on
//! `pdf-core` (pure math) and `pdf-engine` (the pdf.js bridge) — never on app
//! chrome or Tauri.

pub mod components;
pub mod dom;
pub mod effects;
pub mod state;
