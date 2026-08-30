//! Paper backdrop driver: wires the reader's settings and scroll geometry to
//! the paper session (`pdf_engine::paper`, the state machine over the
//! `pdf-paper` crate).
//!
//! The session owns every COLOUR decision — detection off raw frames, the
//! fixed scan, the per-page palette, the cache. The shell owns the GEOMETRY,
//! and reports it as ONE number: the viewport's position along the page
//! ladder, the visible-paint-weighted mean page index. Resting on page N it
//! is exactly `N.0`; straddling pages N and N+1 at 40/60 it is `N + 0.6`,
//! carrying BOTH pages' shares.
//!
//! That one number is what the old page-pair blend lacked. A pair
//! `(dominant, dominant + 1)` is blind to the page BEFORE the dominant one,
//! so right after a handover — when the previous page still fills half the
//! window — the backdrop snapped to the new page's colour while the eye
//! still saw the old one: the "slightly mismatched" seam. The weighted
//! position has no seam: it moves continuously through the handover and is
//! exactly the dominant page's index at rest, so the palette's ladder
//! interpolation meets the pages where they actually are.
//!
//! The position math uses the virtualizer's own coordinate convention
//! (viewport = `[scroll, scroll + height]` against item offsets whose sizes
//! fold in the trailing gap) — the same convention the dominant tracker
//! uses, so the backdrop and the page counter always agree on which page is
//! current.
//!
//! The two halves are wired at different levels on purpose:
//! [`paper_settings`] at the APP root, because the session must know the
//! real blend switch and detection area BEFORE the first document opens —
//! the open flow's cache lookup consults the area and publishes on
//! `blend_on`, and that happens before any reader mounts. [`blend_backdrop`]
//! (geometry) stays with ReaderPage, where the virtualizer's scroll lives.

use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use pdf_paper::{PaperConfig, DEFAULT_EDGE_WIDTH};

use crate::state::AppState;

/// Settings → the session: the blend switch plus the mode / area / scan
/// budget. Sent whether a document is open or not — the session keeps the
/// configuration for the next book and idles otherwise.
///
/// Wired at the APP root, not the reader: on a fresh launch this runs before
/// the first `document_open`, so the cache lookup answers under the reader's
/// real detection area and a hit publishes against a session that already
/// knows blend is on — the alternative (first wiring at reader mount) is why
/// the first open of a session used to flash the theme paper first.
pub fn paper_settings(state: AppState) {
    let settings = state.settings;
    Effect::new(move |_| {
        let (blend_on, mode, area, scan_pages) = settings.with(|st| {
            (
                st.layout.blend_mode,
                st.layout.blend_scope,
                st.layout.blend_area,
                st.layout.blend_scan_pages,
            )
        });
        pdf_engine::paper::configure(
            blend_on,
            PaperConfig {
                mode,
                area,
                scan_pages,
                edge_width: DEFAULT_EDGE_WIDTH,
            },
        );
    });
}

/// Geometry → the session. Called once from ReaderPage, alongside the other
/// reader effects; see the module doc for why this half waits for the reader.
pub fn blend_backdrop(state: AppState) {
    let settings = state.settings;
    let viewer = state.reader.viewer;
    let heights = state.reader.document.metrics.css_heights;

    // The viewport's weighted position along the page ladder. Per scroll
    // tick, and only while blend mode is actually driving a backdrop — the
    // session ignores the number otherwise, so there is nothing to compute.
    Effect::new(move |_| {
        if !settings.with(|st| st.layout.blend_mode) {
            return;
        }
        let page = viewer.page.get();
        if page == 0 {
            return;
        }
        // The paged modes and the horizontal strip have no "between" to
        // report: the position is the page itself, and the backdrop switches
        // with the page turn.
        if viewer.mode.get() != ViewMode::ScrollVertical {
            pdf_engine::paper::position(f64::from(page));
            return;
        }
        let scroll = viewer.scroll_top.get();
        let (_, viewport_h) = viewer.container_size.get();
        let gap = viewer.page_gap.get();
        // Borrow, don't clone: this effect runs on every scroll tick while
        // blend is on, and the column can be a thousand heights deep.
        let pos = heights.with(|column| paper_position(column, gap, scroll, viewport_h));
        pdf_engine::paper::position(if pos > 0.0 { pos } else { f64::from(page) });
    });
}

/// The viewport's position along the page ladder: the visible-paint-weighted
/// mean 1-based page index. `0` when no page paint is visible (the caller
/// holds its last position instead of guessing).
pub(crate) fn paper_position(heights: &[f64], gap: f64, scroll: f64, viewport: f64) -> f64 {
    let view_bottom = scroll + viewport;
    let mut top = 0.0; // the main-axis offset of page `i`'s paint
    let mut weight = 0.0; // total visible page paint
    let mut moment = 0.0; // Σ visible_i × page_i (1-based)
    for (i, &h) in heights.iter().enumerate() {
        let vis = visible_paint(top, h, scroll, view_bottom);
        if vis > 0.0 {
            weight += vis;
            moment += vis * (i as f64 + 1.0);
        }
        top += h + gap; // the trailing gap is chrome, not paint
        if top >= view_bottom {
            break; // no page below this offset can intersect the viewport
        }
    }
    if weight <= f64::EPSILON {
        return 0.0;
    }
    moment / weight
}

fn visible_paint(top: f64, height: f64, view_top: f64, view_bottom: f64) -> f64 {
    (view_bottom.min(top + height) - view_top.max(top)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page alone on screen reports its own index.
    #[test]
    fn a_window_on_one_page_is_exactly_that_page() {
        let heights = [800.0, 800.0];
        // The window exactly covers page 1's paint.
        assert_eq!(paper_position(&heights, 24.0, 0.0, 800.0), 1.0);
        // Deep inside page 2, page 3 out of sight.
        assert_eq!(paper_position(&heights, 24.0, 900.0, 800.0), 2.0);
    }

    /// The load-bearing case: mid-transition, the position carries both
    /// pages' visible shares. 100px of page 1 + 676px of page 2 → page
    /// 1 + 676/776, continuously through the handover.
    #[test]
    fn a_straddled_window_weighs_both_pages_shares() {
        // Page 1 paints [0, 800], page 2 [824, 1624]. Window [700, 1500]:
        // page 1 shows 100px, page 2 shows 676px.
        let heights = [800.0, 800.0];
        let pos = paper_position(&heights, 24.0, 700.0, 800.0);
        assert!((pos - (100.0 + 676.0 * 2.0) / 776.0).abs() < 1e-9, "{pos}");
    }

    /// The regression the old pair blend could not pass: JUST after the
    /// dominant handover, the previous page still owns 45% of the window,
    /// and the position — unlike the old pair flip — still carries it.
    #[test]
    fn just_after_the_handover_the_old_page_still_counts() {
        // Window [720, 1520]: page 1 shows 80px, page 2 shows 696px — page 2
        // is dominant, but the position is 1.9, not 2.0.
        let heights = [800.0, 800.0];
        let pos = paper_position(&heights, 24.0, 720.0, 800.0);
        assert!((pos - (80.0 + 696.0 * 2.0) / 776.0).abs() < 1e-9, "{pos}");
        assert!(pos < 2.0 && pos > 1.5);
    }

    /// A window standing entirely in the gap (or on an unmeasured column)
    /// has no paint to weigh: 0, so the caller holds its last position
    /// instead of twitching.
    #[test]
    fn a_window_without_paint_holds_still() {
        let heights = [800.0, 800.0];
        assert_eq!(paper_position(&heights, 24.0, 805.0, 10.0), 0.0);
        assert_eq!(paper_position(&[], 24.0, 0.0, 800.0), 0.0);
        assert_eq!(paper_position(&heights, 24.0, 0.0, 0.0), 0.0);
    }

    /// No Gap mode is the same math with the pages contiguous.
    #[test]
    fn zero_gap_weighs_across_the_seam() {
        let heights = [800.0, 800.0];
        // Window [750, 1550]: page 1 shows 50px, page 2 shows 750px.
        let pos = paper_position(&heights, 0.0, 750.0, 800.0);
        assert!((pos - (50.0 + 750.0 * 2.0) / 800.0).abs() < 1e-9, "{pos}");
    }

    /// Offsets count every preceding page's trailing gap: deep in a book the
    /// window sits exactly where the strip laid it — and every visible sliver
    /// carries its share, even a 20px corner of the page above.
    #[test]
    fn offsets_count_the_gaps_above() {
        // Pages of 100 with gap 20: page 2 paints [120, 220], page 3 [240,
        // 340], page 4 [360, 460]. Window [200, 400]: page 2 shows 20px,
        // page 3 shows 100px, page 4 shows 40px → (20×2 + 100×3 + 40×4)/160.
        let heights = [100.0; 5];
        let pos = paper_position(&heights, 20.0, 200.0, 200.0);
        assert!((pos - (20.0 * 2.0 + 100.0 * 3.0 + 40.0 * 4.0) / 160.0).abs() < 1e-9, "{pos}");
    }

    /// A window past the last page (over-scroll into the strip's padding)
    /// clamps to the last page, not past it.
    #[test]
    fn overscroll_clamps_to_the_last_page() {
        let heights = [800.0];
        assert_eq!(paper_position(&heights, 24.0, 0.0, 800.0), 1.0);
        // Window [700, 1500]: page 1 still shows 100px of paint.
        let pos = paper_position(&heights, 24.0, 700.0, 800.0);
        assert_eq!(pos, 1.0);
    }

    /// A tall window over three small pages weighs them all.
    #[test]
    fn a_tall_window_weighs_every_visible_page() {
        // Pages of 100, gap 0: window [0, 300] sees pages 1..3 equally.
        let heights = [100.0; 4];
        let pos = paper_position(&heights, 0.0, 0.0, 300.0);
        assert!((pos - 2.0).abs() < 1e-9, "{pos}");
    }
}
