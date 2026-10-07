//! Paper backdrop driver: settings and scroll geometry into this pane's
//! paper session.

use leptos::prelude::*;

use pdf_paper::{DEFAULT_EDGE_WIDTH, PaperConfig};
use reader_core::settings::LayoutSettings;
use reader_core::view::ViewMode;

/// Settings → the pane's session: blend switch plus detection area.
pub fn paper_settings(state: crate::context::ReaderContext) {
    let settings = state.settings;
    let pane = state.pane;
    Effect::new(move |_| {
        let layout = settings.with(|st| st.layout);
        configure(&pane.pdf(), layout);
    });
}

/// State the settings to a freshly opened session, from the seed.
pub fn configure_session(state: &crate::context::ReaderContext) {
    let layout = state.settings.with_untracked(|st| st.layout);
    configure(&state.pane.pdf(), layout);
}

/// Hand one snapshot of the layout settings to a session.
fn configure(pdf: &crate::pane::engine::PdfPane, layout: LayoutSettings) {
    pdf.paper_configure(
        layout.blend_mode,
        PaperConfig {
            area: layout.blend_area,
            edge_width: DEFAULT_EDGE_WIDTH,
        },
    );
}

/// Geometry → the session, once per pane from its mount.
pub fn blend_backdrop(state: crate::context::ReaderContext) {
    let settings = state.settings;
    let viewer = state.reader.viewer;
    let heights = state.reader.document.content.metrics.css_heights;
    let pane = state.pane;

    // The viewport's weighted position along the page ladder, per scroll.
    Effect::new(move |_| {
        // Text documents want none of this; their session is closed.
        if state.reader.reflowable() {
            return;
        }
        if !settings.with(|st| st.layout.blend_mode) {
            return;
        }
        let page = viewer.page.get();
        if page == 0 {
            return;
        }
        // Paged modes have no "between": the position is the page.
        if viewer.mode.get() != ViewMode::ScrollVertical {
            pane.pdf().paper_position(f64::from(page));
            return;
        }
        let scroll = viewer.scroll_top.get();
        let (_, viewport_h) = viewer.container_size.get();
        let gap = viewer.page_gap.get();
        // Borrow, don't clone: the column can be a thousand deep.
        let pos = heights.with(|column| paper_position(column, gap, scroll, viewport_h));
        pane.pdf()
            .paper_position(if pos > 0.0 { pos } else { f64::from(page) });
    });
}

/// The viewport's position along the ladder, weighted by visible paint.
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

    /// Mid-transition, the position carries both pages' shares.
    #[test]
    fn a_straddled_window_weighs_both_pages_shares() {
        // Page 1 paints [0, 800], page 2 [824, 1624]; window [700, 1500].
        let heights = [800.0, 800.0];
        let pos = paper_position(&heights, 24.0, 700.0, 800.0);
        assert!((pos - (100.0 + 676.0 * 2.0) / 776.0).abs() < 1e-9, "{pos}");
    }

    /// Just after the handover the old page still counts in the position.
    #[test]
    fn just_after_the_handover_the_old_page_still_counts() {
        // Window [720, 1520]: page 2 is dominant but the position is 1.9.
        let heights = [800.0, 800.0];
        let pos = paper_position(&heights, 24.0, 720.0, 800.0);
        assert!((pos - (80.0 + 696.0 * 2.0) / 776.0).abs() < 1e-9, "{pos}");
        assert!(pos < 2.0 && pos > 1.5);
    }

    /// A window with no paint weighs 0, so the caller holds still.
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

    /// Offsets count every preceding page's trailing gap.
    #[test]
    fn offsets_count_the_gaps_above() {
        // Pages of 100 with gap 20: window [200, 400] sees 20+100+40px.
        let heights = [100.0; 5];
        let pos = paper_position(&heights, 20.0, 200.0, 200.0);
        assert!(
            (pos - (20.0 * 2.0 + 100.0 * 3.0 + 40.0 * 4.0) / 160.0).abs() < 1e-9,
            "{pos}"
        );
    }

    /// Over-scroll clamps to the last page, not past it.
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
