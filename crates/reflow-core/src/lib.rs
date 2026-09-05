//! The shared maths of laying reflowable text out, whatever wrote it.
//!
//! A raw file becomes [`block`](block)s — that part belongs to the format, so
//! it lives in `txt-core` and `md-core` — and from there everything is common:
//! the blocks are packed into fixed-size [`page`](pager)s by a greedy cutter,
//! the [geometry](geometry) says how wide a page's text column is and where a
//! book's gutter falls, [`typography`](typography) resolves the reader's
//! persisted knobs into the CSS the interface paints and into the one number
//! the height estimate needs, and [`search`](search) is the substring index
//! over the same blocks.
//!
//! Nothing here knows what Markdown *is*. That is the seam: a format crate
//! parses and classifies, this crate measures, paginates and paints. A third
//! reflowable format (epub's HTML, once there is one) brings a parser and
//! reuses all of this.
//!
//! Pure computation, as the other cores are: no wasm, no DOM, no leptos, and
//! unit-testable on the host via `cargo test -p reflow-core`. The one
//! `reader-core` dependency is the persisted typography schema, whose
//! resolution this crate owns.

#![forbid(unsafe_code)]

pub mod block;
pub mod geometry;
pub mod pager;
pub mod search;
pub mod source;
pub mod typography;

pub use block::{BlockKind, FenceTracker, TextBlock, SPLIT_MAX_LINES, split_blocks, subdivide_with};
pub use geometry::{PageGeometry, SpineSide, PAGE_HEIGHT, PAGE_WIDTH, geometry};
pub use pager::{
    block_page_index, estimate_block_height, estimate_heights, first_block_of_page, paginate,
    BlockMetrics, PageCut,
};
pub use search::{find_matches, TextHit};
pub use source::normalize;
