//! Fit-mode effect: recomputes the render scale while FitMode::Width or
//! FitMode::Page is active. Runs once from the app root (ReaderView) so fit
//! works in BOTH the single and continuous views, whichever is mounted.
//!
//! Reactively tracks `fit`, `container_size`, and `page1_size`; when a fit mode
//! is active it computes the matching scale and writes `viewer.scale` +
//! `viewer.render_scale`. `scale` is read untracked so the write-back does not
//! retrigger this effect (no loop).
//!
//! The scale write is DEBOUNCED: the sidebar `<aside>` animates its width over
//! 300ms, emitting a burst of `container_size` writes, and an immediate
//! recompute per frame would cancel every in-flight render (`PageCanvas`
//! re-renders on a scale change), flashing the visible pages. Scheduling the
//! recompute 120ms after the size has been stable yields exactly one re-render
//! per sidebar toggle, at the end of the slide. `container_size` itself stays
//! live — page tracking and the visible-page math still need it — only the
//! scale write is debounced. Changes that leave the fit scale essentially
//! unchanged are skipped: if the recomputed scale is within `0.0005` of the
//! current `render_scale`, the write is a no-op and does not force a
//! full re-render of every `PageCanvas`.

use std::time::Duration;

use leptos::prelude::*;

use crate::core::math::{fit_scale, FitMode};
use crate::core::state::AppState;

/// Must be called once from the app root (ReaderView).
pub fn fit_effect(state: AppState) {
    Effect::new(move |_| {
        let fit = state.viewer.fit.get();
        if fit == FitMode::None {
            return;
        }
        let (cw, ch) = state.viewer.container_size.get();
        let Some(p) = state.doc.page1_size.get() else {
            return;
        };

        // Debounce: each `container_size` change re-runs this effect, which
        // clears the previous timer (same pattern as the toast auto-dismiss in
        // organisms/toast.rs), so the recompute only fires once the size has
        // settled for ~120ms.
        let handle = set_timeout_with_handle(
            move || {
                let s = fit_scale(
                    fit,
                    cw,
                    ch,
                    p.width,
                    p.height,
                    48.0,
                    state.viewer.scale.get_untracked(),
                );
                let prev = state.viewer.render_scale.get_untracked();
                if (s - prev).abs() >= 0.0005 {
                    state.viewer.scale.set(s);
                    state.viewer.render_scale.set(s);
                }
            },
            Duration::from_millis(120),
        )
        .ok();
        on_cleanup(move || {
            if let Some(h) = handle {
                h.clear();
            }
        });
    });
}