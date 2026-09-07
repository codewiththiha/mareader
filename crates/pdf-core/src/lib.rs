//! PDF-specific domain logic: the page frame, its outline, its search index
//! and the grid its rasters are snapped to.
//!
//! Deliberately the smallest crate in the reader: everything a plain-text or
//! Markdown document shares — settings, appearance and tint, the zoom ladder,
//! view modes and spread arithmetic, floating-box geometry, the search result
//! shape, the chapter node — lives in `reader-core`. What stays here is only
//! what is meaningless without a page of PDF on screen.
//!
//! Pure computation: no wasm and no DOM beyond the one device-pixel read at
//! the presentation boundary; unit-testable via `cargo test -p pdf-core`.

pub mod outline;
pub mod pixel_grid;
pub mod search;
