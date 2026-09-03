//! Pure PDF-reader domain logic: no wasm, no DOM, no leptos.
//!
//! The four axes of the codebase, this crate is axis 1 (pure computation).
//! Everything here is unit-testable on the host via `cargo test -p pdf-core`.

pub mod appearance;
pub mod documents;
pub mod filename;
pub mod floating;
pub mod layout;
pub mod math;
pub mod oklch;
pub mod presets;
pub mod search;
pub mod settings;

/// The AI word-card domain (the gloss box geometry, the persisted mark, the
/// anchor trait) now lives in the `ai-core` crate; re-exported here so the
/// old `pdf_core::gloss` path keeps resolving. New code should import from
/// `ai_core::gloss` directly.
pub use ai_core::gloss;
