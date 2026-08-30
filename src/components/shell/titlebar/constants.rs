//! Cross-file numeric contracts between Rust layout code and the CSS
//! classes it measures against. The Tailwind class names each value must
//! stay in sync with are listed so drift is findable in one grep.
//!
//! Values that only Rust consumes stay where they are consumed: the
//! titlebar height already has its source of truth in
//! `pdf_core::layout::TOOLBAR_H` (`h-12`), and the sidebar width lives
//! with the sidebar chrome (`w-72`).

/// The document title is hidden below this width — a useless stub ("P…").
pub const MIN_DOC_TITLE_WIDTH: f64 = 56.0;
