//! Pure PDF-reader domain logic: no wasm, no DOM, no leptos.
//!
//! The four axes of the codebase, this crate is axis 1 (pure computation).
//! Everything here is unit-testable on the host via `cargo test -p pdf-core`.

pub mod appearance;
pub mod filename;
pub mod layout;
pub mod math;
pub mod oklch;
pub mod presets;
pub mod search;
pub mod settings;
