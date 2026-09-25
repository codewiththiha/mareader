//! The reflowable half of the anchor contract: a thin adapter over
//! [`crate::components::ai::reflow_anchor`].
//!
//! A document that re-lays itself out has no durable page-space rect, so both
//! answers here are projections of a [`ReflowSpot`] — a block and a character
//! range — asked of the DOM again. The projection itself, the spot envelope a
//! mark carries and the stroke geometry all live in `reflow_anchor`; this module
//! is the bridge that lets the format-blind watchers call it.

use ai_core::gloss::{GlossBox, PageAnchor, ReflowSpot};
use reader_core::view::ViewMode;

use crate::components::ai::reflow_anchor;
use crate::state::ReaderState;

use super::FormatAnchorBridge;

/// The reflowable bridge (plain text and Markdown share it — they differ in
/// how a block is painted, never in where one lives).
///
/// The block-and-character identity rides in
/// [`ai_core::gloss::GlossMark::context`] as a tagged envelope, so this bridge
/// is reached with the mark in hand rather than with a bare anchor: see
/// [`crate::components::ai::reflow_anchor`].
#[derive(Clone, Copy)]
pub struct ReflowAnchorBridge {
    pub state: ReaderState,
    /// The spot to project. `None` for a mark captured before the tracker could
    /// walk its offsets (a legacy mark, or a selection whose block could not be
    /// identified), which then has nothing to project and says so.
    pub spot: Option<ReflowSpot>,
    /// The view mode, which says which host element carries the page.
    pub mode: ViewMode,
}

impl FormatAnchorBridge for ReflowAnchorBridge {
    fn screen_box(&self, _anchor: &PageAnchor, _scale: f64) -> Option<GlossBox> {
        // The spot IS the anchor for a document made of type. The box a mark was
        // captured with is a viewport snapshot: it is stale after one scroll, and
        // re-using it would move the card and the Explain pill onto whatever words
        // happen to be there now. So a spot that cannot be resolved — a block
        // virtualized away, one a re-parse orphaned, or an envelope from a
        // version this build cannot read — answers `None`, which is the same
        // thing a PDF says about an unmounted page, and the watchers already
        // treat it as "the origin left the viewport".
        let spot = self.spot?;
        reflow_anchor::spot_screen_box_in(self.state, &spot, self.mode)
    }

    fn capture(&self, _scale: f64) -> Option<PageAnchor> {
        // The engine's selection tracker normally hands the spot over with the
        // event, so this is the second path: the same walk, app-side, for a
        // selection that arrived without one. A reflowable bridge with a spot
        // already in hand has nothing to capture and says so.
        if self.spot.is_some() {
            return None;
        }
        reflow_anchor::capture_selection(self.state).map(|(_, anchor)| anchor)
    }
}
