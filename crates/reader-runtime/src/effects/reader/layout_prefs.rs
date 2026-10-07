//! The layout preferences, from settings to the strips.

use leptos::prelude::*;
use virtual_list_leptos::Virtualizer;

use reader_core::view::{PAGE_GAP, ViewMode};
use reader_core::zoom_math::FitMode;

use crate::state::ZoomCommand;
use app_ui::theme_paint::html_style;

/// Install the gap and margin effects, in that order.
pub fn layout_prefs(
    state: crate::context::ReaderContext,
    vertical: Virtualizer,
    horizontal: Virtualizer,
) {
    let vs = state.reader;

    // Seed margin from settings once the reader mounts.
    {
        let m = state.settings.with_untracked(|st| st.layout.page_margin);
        let on_horizontal_strip = vs.viewer.mode.get_untracked() == ViewMode::ScrollHorizontal;
        vs.viewer
            .page_margin
            .set(if on_horizontal_strip { 0.0 } else { m });
    }

    // No-gap pref → runtime gap + rescale.
    {
        let v = vertical.clone();
        Effect::new(move |_| {
            let no_gap = state.settings.with(|st| st.layout.no_gap);
            let gap = if no_gap { 0.0 } else { PAGE_GAP };
            // The CSS half of the gap, published with the value it mirrors.
            if let Some(style) = html_style() {
                let _ = style.set_property("--page-gap", &format!("{gap}px"));
            }
            if (vs.viewer.page_gap.get_untracked() - gap).abs() < 1e-9 {
                return;
            }
            vs.viewer.page_gap.set(gap);
            v.rescale(1.0, vs.document.content.metrics.strip_sizes(gap));
        });
    }

    // Margin pref: cross-axis for the vertical strip; the horizontal
    // strip is exempt.
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
                .content
                .metrics
                .intrinsic
                .with_untracked(|w| w.iter().map(|s| s.width).collect::<Vec<f64>>());
            // Vertical: margin is cross-axis; sizes unchanged aside from gap.
            v.rescale(1.0, vs.document.content.metrics.strip_sizes(gap));
            // Horizontal: margin is main-axis, so it resolves to 0.
            hv.rescale(1.0, move |i| {
                widths.get(i).copied().unwrap_or(0.0) * scale + 2.0 * m
            });
            // A margin change must re-fit the page; entering the strip
            // skips it.
            if !on_horizontal_strip && vs.viewer.fit.get_untracked() != FitMode::None {
                vs.viewer.zoom.post(ZoomCommand::Refit, false);
            }
        });
    }

    // The column dial's runtime mirror, a plain tracked sync.
    Effect::new(move |_| {
        let pct = state.settings.with(|st| st.layout.column_width_pct);
        if (vs.viewer.column_width_pct.get_untracked() - pct).abs() > 1e-9 {
            vs.viewer.column_width_pct.set(pct);
        }
    });
}
