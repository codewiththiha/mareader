//! The reflowable formats' shared machinery: page host, continuous stream,
//! virtualized strip, the column that measures the real heights, the walk that
//! finds a block's characters in the DOM ([`spot`]), and what gets painted over
//! them — a block's gloss strokes ([`gloss`]) and its search hits
//! ([`highlight`]).
//!
//! Not one of these components knows whether the document it is laying out came
//! from `txt-core` or `md-core`. The blocks are the same shape, the
//! geometry is the same A4 sheet, the page cut is the same greedy pack — so the
//! layout code is written once, and the ONE decision a format forces is which
//! view paints a block ([`block_render`], which feeds
//! [`BlockView`](super::block_render::BlockView)). That is the whole extent of
//! format awareness in this module: it selects a renderer, it never selects a
//! pipeline.
//!
//! Deliberately NOT here: the `Format` enum itself, the parsing, the
//! pagination maths (all `reflow-core`, `txt-core`, `md-core`), and the page
//! numbers/zoom/navigation the reflowable formats share with the PDF (the
//! reader's own `viewer` state).

mod gloss;
mod highlight;
mod measure;
mod page;
pub(crate) mod spot;
mod strip;
mod stream;

pub use gloss::ReflowGlossLayer;
pub use highlight::BlockSearchHits;
pub use measure::ReflowMeasureColumn;
pub use page::ReflowPage;
pub use strip::ReflowPageStrip;
pub use stream::ReflowStreamLayout;

use super::block_render::BlockRender;
use crate::state::ReaderState;

/// Which block renderer the open document's blocks get.
///
/// One read of `document.format`, tracked, so a document of the other kind
/// swapping in rebuilds the pages. Every surface that paints a block — a paged
/// host, a stream row, the measure column — asks this rather than deciding for
/// itself, which is what keeps a page and its measurement in agreement.
pub(crate) fn block_render(state: ReaderState) -> BlockRender {
    BlockRender::of_format(state.format())
}
