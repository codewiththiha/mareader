//! PDF-specific domain logic: the page frame, its outline, its search index
//! and the grid its rasters are snapped to.
//!
//! This is the smallest crate in the reader on purpose. Everything that a plain
//! text or Markdown document shares — the settings model, the appearance and
//! tint pipeline, the zoom ladder, the view modes and the spread arithmetic, the
//! floating-box geometry, the search result shape, the chapter node — lives in
//! `reader-core`, because a format that is not PDF needs it too and used to have
//! to reach through this crate's name to say so. What stays here is only what is
//! meaningless without a page of PDF on screen.
//!
//! Pure computation, as before: no wasm and no DOM beyond the one device-pixel
//! read at the presentation boundary, and unit-testable on the host via
//! `cargo test -p pdf-core`.

pub mod layout;
pub mod outline;
pub mod pixel_grid;
pub mod search;
