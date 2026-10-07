//! The reflowable formats' shared machinery: page host, stream, strip, paint.

mod gloss;
mod highlight;
mod page;
pub(crate) mod spot;
mod stream;
mod strip;

pub use gloss::ReflowGlossLayer;
pub use highlight::BlockSearchHits;
pub use page::ReflowPage;
pub use stream::ReflowStreamLayout;
pub use strip::ReflowPageStrip;

use super::block_render::BlockRender;
use crate::state::ReaderState;

/// Which block renderer the open document's blocks get, read tracked.
pub(crate) fn block_render(state: ReaderState) -> BlockRender {
    BlockRender::of_format(state.format())
}
