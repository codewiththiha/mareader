//! Entering continuous mode: align the virtualized scroll position to the
//! current page.

use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use virtual_list_leptos::{Align, ScrollMode, Virtualizer};

use crate::state::ReaderState;

pub(super) fn mode_flip(state: ReaderState, virtualizer: Virtualizer) {
    let mut was_continuous = state.viewer.mode.get_untracked() == ViewMode::Continuous;
    Effect::new(move |_| {
        let continuous = state.viewer.mode.get() == ViewMode::Continuous;
        if continuous && !was_continuous {
            // A fit debounce interrupted by the flip can strand `zoom_animating`
            // true, which would keep every re-mounted page's render effect in
            // the suspended branch — a blank viewer. The flip is an explicit
            // user action, so the flag is released before the re-entry render.
            state.viewer.zoom_animating.set(false);
            let page = state.viewer.page.get_untracked();
            let v = virtualizer.clone();
            request_animation_frame(move || {
                // Fresh container: recalibrate the viewport before anchoring,
                // otherwise the window is computed against stale geometry.
                v.remeasure_container();
                if page > 0 {
                    let index = (page - 1) as usize;
                    v.scroll_to_index(index, Align::Start, ScrollMode::Instant);
                }
            });
        }
        was_continuous = continuous;
    });
}
