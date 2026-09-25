//! The chrome both pages wrap themselves in: the layout controller and the
//! app title bar. The rail family (sidebar) is the reader's — it lives in
//! `reader-runtime::components::shell::sidebar` — and the reader-only titles
//! (`document_title`, `floating_document_title`) travel with it.

pub mod controller;
pub mod titlebar;
