//! PDF documents through hayro: pages, rasters, text and links (L1).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod cache;
mod check;
mod dest;
mod document;
mod links;
mod outline;
mod paper;
pub mod sample;
mod strings;
mod text;
mod tiles;
mod title;

pub use cache::{RenderSession, describe, first_page};
pub use document::{
    Document, MAX_PAGES, MAX_RASTER_PIXELS, MAX_RASTER_SIDE, OpenError, PageSize, Raster,
    RenderError,
};
pub use links::{Link, LinkTarget, MAX_LINKS, web_address};
pub use outline::{MAX_DEPTH, OutlineEntry, active_entry};
pub use paper::{EDGE, Palette, Paper, SAMPLE_SIDE, SHARE, position};
pub use text::{Bounds, MAX_CHARS, PageText, Passage, PassageRun, TextChar, TextLayer};
pub use tiles::{MAX_TILED_SIDE, TILE_SIDE, Tiling};
pub use title::{display_name, file_stem, is_usable_title};
