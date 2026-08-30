//! Entering a scrolling mode: align the virtualized scroll position to the
//! current page.
//!
//! The restore is a multi-frame dance: remeasure the freshly-mounted
//! container, then scroll to the preserved page. During those frames the
//! scroll→page sync would read the not-yet-restored strip (still at its
//! initial/zero offset) and "correct" the page back off the preserved one —
//! the reader would slip to page 1 (or a transient 0) the moment a mode
//! changed. `defer` is the flag that tells that sync to stand down until the
//! restore's scroll has landed.

use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use virtual_list_leptos::{Align, ScrollMode, Virtualizer};

use crate::state::ReaderState;

/// The shared "a mode restore is in flight" flag. Borrowed by the scroll→page
/// sync (`navigation_sync`), which refuses to correct the page while it is set.
///
/// Reactive so that dropping it re-runs the scroll→page arm on the restored
/// dominant, rather than leaving it standing down until the reader scrolls.
pub(super) type DeferFlag = RwSignal<bool>;

pub(super) fn on_mode_change(
    state: ReaderState,
    virtualizer: Virtualizer,
    h_virtualizer: Virtualizer,
    defer: DeferFlag,
) {
    let mut was_continuous = state.viewer.mode.get_untracked() == ViewMode::ScrollVertical;
    let mut was_horizontal = state.viewer.mode.get_untracked() == ViewMode::ScrollHorizontal;
    Effect::new(move |_| {
        let continuous = state.viewer.mode.get() == ViewMode::ScrollVertical;
        let horizontal = state.viewer.mode.get() == ViewMode::ScrollHorizontal;
        if continuous && !was_continuous {
            // Raise the flag FIRST (same flush as the mode flip, so the
            // scroll→page sync that re-runs for the mode change sees it and
            // stands down) and keep it up until the restore has scrolled.
            defer.set(true);
            let page = state.viewer.page.get_untracked();
            let v = virtualizer.clone();
            let defer = defer.clone();
            request_animation_frame(move || {
                // Fresh container: recalibrate the viewport before anchoring,
                // otherwise the window is computed against stale geometry.
                v.remeasure_container();
                if page > 0 {
                    let index = (page - 1) as usize;
                    v.scroll_to_index(index, Align::Start, ScrollMode::Instant);
                }
                // The restore's scroll has been commanded; hand the page back
                // to the sync. It now reads a strip anchored at the restored
                // page, so it agrees instead of clobbering.
                defer.set(false);
            });
        }
        if horizontal && !was_horizontal {
            defer.set(true);
            let page = state.viewer.page.get_untracked();
            let v = h_virtualizer.clone();
            let defer = defer.clone();
            request_animation_frame(move || {
                v.remeasure_container();
                if page > 0 {
                    let index = (page - 1) as usize;
                    v.scroll_to_index(index, Align::Center, ScrollMode::Instant);
                }
                // Measure a SECOND time, one frame later. On the very first
                // switch the strip's flex row has not always committed its
                // layout by this frame, so `remeasure_container` reads a
                // clientWidth of 0: the core gets a zero viewport, the
                // window resolves to nothing, and no page mounts. The
                // ResizeObserver cannot rescue it either — it was attached
                // before the element had a layout box, so its first entry
                // reports 0 as well. Going out of the mode and back in
                // remounts after layout has settled, which is exactly why
                // that worked around it. One more frame guarantees real
                // geometry and the window mounts its pages.
                let v2 = v.clone();
                request_animation_frame(move || {
                    v2.remeasure_container();
                });
                // Release the flag on the same frame the restore landed.
                defer.set(false);
            });
        }
        was_continuous = continuous;
        was_horizontal = horizontal;
        // A mode flip leaves the outgoing view's rasters behind and nothing
        // necessarily renders right after, so the engine's own sweep (which
        // only runs inside a render) would never fire. Release now.
        pdf_engine::api::sweep();
    });
}
