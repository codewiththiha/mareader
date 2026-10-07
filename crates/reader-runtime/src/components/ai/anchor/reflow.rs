//! The reflowable anchor half: a thin adapter over `reflow_anchor`.

use ai_core::gloss::{GlossBox, PageAnchor, ReflowSpot};
use reader_core::view::ViewMode;

use crate::components::ai::reflow_anchor;
use crate::state::ReaderState;

use super::FormatAnchorBridge;

/// The reflowable bridge, shared by plain text and Markdown.
#[derive(Clone, Copy)]
pub struct ReflowAnchorBridge {
    pub state: ReaderState,
    /// The spot to project, `None` for a mark without one.
    pub spot: Option<ReflowSpot>,
    /// The view mode, which says which host element carries the page.
    pub mode: ViewMode,
}

impl FormatAnchorBridge for ReflowAnchorBridge {
    fn screen_box(&self, _anchor: &PageAnchor, _scale: f64) -> Option<GlossBox> {
        // The spot IS the anchor: the captured box is a stale snapshot.
        let spot = self.spot?;
        reflow_anchor::spot_screen_box_in(self.state, &spot, self.mode)
    }

    fn capture(&self, _scale: f64) -> Option<PageAnchor> {
        // The second path: the same walk, app-side, with no spot in hand.
        if self.spot.is_some() {
            return None;
        }
        reflow_anchor::capture_selection(self.state).map(|(_, anchor)| anchor)
    }
}
