//! The layout preferences, from the settings that store them to the strips that
//! lay out against them.
//!
//! Two of the reader's layout settings are not read where they are used. The
//! page gap and the page margin are prefs a settings surface writes; the thing
//! that actually has to change is a strip's size model, and only a rescale can
//! change that. So each pref needs an effect, and both effects end the same way
//! — `rescale(1.0, …)` against the vertical strip, the horizontal one, or both.
//!
//! These lived in `features/reader/page.rs`, in the run of effects before that
//! file's `view!`, where a reader who came for the slot wiring had to step over
//! them. They are reader effects, so they live with the reader's other effects
//! and the page installs them.
//!
//! THE ORDER, which is why this is one function rather than two:
//!
//! 1. The margin is seeded from the persisted settings before any effect runs,
//!    so the first frame is laid out with the reader's own margin rather than
//!    the default.
//! 2. The gap effect runs before the margin effect, because the margin's
//!    rescale reads the gap the effect above it has just resolved.
//! 3. Both run before `reflow_layout`, which reads the gap as well and is
//!    installed by the page immediately after this.

use leptos::prelude::*;
use virtual_list_leptos::Virtualizer;

use reader_core::view::{PAGE_GAP, ViewMode};
use reader_core::zoom_math::FitMode;

use crate::state::reader::ZoomCommand;
use crate::state::AppState;

/// Install the gap and margin effects, in the order documented above.
///
/// Both strips are handed in rather than reached for: the page owns the
/// virtualizers, and an effect that rescales a strip ought to say which one.
pub fn layout_prefs(state: AppState, vertical: Virtualizer, horizontal: Virtualizer) {
    let vs = state.reader;

    // Seed margin from persisted settings once the reader mounts. The
    // horizontal strip is the one mode that never carries a page margin, so
    // the seed honours the same mode rule as the sync effect below.
    {
        let m = state.settings.with_untracked(|st| st.layout.page_margin);
        let on_horizontal_strip =
            vs.viewer.mode.get_untracked() == ViewMode::ScrollHorizontal;
        vs.viewer.page_margin.set(if on_horizontal_strip { 0.0 } else { m });
    }

    // No-gap pref → runtime gap + rescale. (The continuous text stream is
    // not party to this: it lays blocks edge to edge with no gap at all,
    // and the vertical page strip it replaced is simply not mounted while
    // a text document streams.)
    {
        let v = vertical.clone();
        Effect::new(move |_| {
            let no_gap = state.settings.with(|st| st.layout.no_gap);
            let gap = if no_gap { 0.0 } else { PAGE_GAP };
            if (vs.viewer.page_gap.get_untracked() - gap).abs() < 1e-9 {
                return;
            }
            vs.viewer.page_gap.set(gap);
            v.rescale(1.0, vs.document.content.metrics.strip_sizes(gap));
        });
    }

    // Page margin pref — cross-axis for the vertical strip and both
    // paginated shells. The horizontal strip is exempt: it lays pages
    // edge-to-edge along the scroll axis, so side air there would read as
    // dead space between pages rather than margin. This effect resolves the
    // stored pref to an effective margin of 0 whenever the mode is
    // ScrollHorizontal — without touching the stored value — and tracks the
    // mode, so leaving the horizontal strip restores whatever the setting
    // holds on the flip itself.
    {
        let (v, hv) = (vertical.clone(), horizontal);
        Effect::new(move |_| {
            let stored = state.settings.with(|st| st.layout.page_margin);
            let on_horizontal_strip = vs.viewer.mode.get() == ViewMode::ScrollHorizontal;
            let m = if on_horizontal_strip { 0.0 } else { stored };
            if (vs.viewer.page_margin.get_untracked() - m).abs() < 1e-9 {
                return;
            }
            vs.viewer.page_margin.set(m);
            let scale = vs.viewer.zoom.visual_scale();
            let gap = vs.viewer.page_gap.get_untracked();
            let widths = vs
                .document
                .content.metrics
                .intrinsic
                .with_untracked(|w| w.iter().map(|s| s.width).collect::<Vec<f64>>());
            // Vertical: margin is cross-axis; sizes unchanged aside from gap.
            v.rescale(1.0, vs.document.content.metrics.strip_sizes(gap));
            // Horizontal: margin is main-axis — which the exempt mode simply
            // never has (m resolves to 0 there).
            hv.rescale(1.0, move |i| widths.get(i).copied().unwrap_or(0.0) * scale + 2.0 * m);
            // A margin change must re-fit the page under the reader: the fit
            // target derives from the usable width (`cw - 2*margin`), so the
            // page only visibly gains side space once that scale is re-resolved
            // against the newly applied margin. Posting here guarantees the
            // refit even if no other watcher happens to fire for a setting-only
            // change, and is a no-op when no fit is active. Entering the
            // horizontal strip skips the post: that switch drops the fit to
            // None anyway, and resolving the OUTGOING fit against the new axis
            // is exactly the zoom jump the mode flip guards against
            // (`crate::effects::reader::mode_change`).
            if !on_horizontal_strip && vs.viewer.fit.get_untracked() != FitMode::None {
                vs.viewer.zoom.post(ZoomCommand::Refit, false);
            }
        });
    }
}
