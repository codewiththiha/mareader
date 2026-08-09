//! Fit-mode effect: recomputes the render scale while FitMode::Width or
//! FitMode::Page is active. Runs once from the app root (ReaderView) so fit
//! works in BOTH the single and continuous views, whichever is mounted.
//!
//! Reactively tracks `fit`, `container_size`, and `page1_size`; when a fit mode
//! is active it computes the matching scale and writes `viewer.scale` +
//! `viewer.render_scale`. `scale` is read untracked so the write-back does not
//! retrigger this effect (no loop).

use leptos::prelude::*;

use crate::core::math::{fit_scale, FitMode};
use crate::core::state::AppState;

/// Must be called once from the app root (ReaderView).
pub fn fit_effect(state: AppState) {
    Effect::new(move || {
        let fit = state.viewer.fit.get();
        if fit == FitMode::None {
            return;
        }
        let (cw, ch) = state.viewer.container_size.get();
        let Some(p) = state.doc.page1_size.get() else {
            return;
        };
        let s = fit_scale(
            fit,
            cw,
            ch,
            p.width,
            p.height,
            48.0,
            state.viewer.scale.get_untracked(),
        );
        state.viewer.scale.set(s);
        state.viewer.render_scale.set(s);
    });
}
