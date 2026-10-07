//! Target resolution: one concrete scale from a command plus context,
//! so nothing can drift apart.

use leptos::prelude::*;

use reader_core::view::ViewMode;
use reader_core::zoom_math::{FitMode, clamp_scale, fit_scale, nearest_zoom};

use crate::state::{ReaderState, ZoomCommand};

use super::config::{SETTLED_EPSILON, ZoomProfile, zoom_profile};

/// Resolve a command to its scale, or `None` to stand down; every read
/// is untracked.
pub(crate) fn resolve(
    state: &ReaderState,
    cmd: ZoomCommand,
    in_flight: Option<f64>,
) -> Option<f64> {
    let zoom = state.viewer.zoom;
    let profile = zoom_profile();
    match cmd {
        ZoomCommand::Step(dir) => {
            // Step from the in-flight target while a tween runs, else from
            // the settled scale.
            let base = in_flight.unwrap_or_else(|| zoom.visual_scale());
            let target = profile.clamp(nearest_zoom(base, dir));
            // At the ladder's end there is nowhere to go: bail BEFORE
            // recording intent.
            if (target - base).abs() < SETTLED_EPSILON {
                return None;
            }
            zoom.desired.set(target);
            state.viewer.fit.set(FitMode::None);
            Some(target)
        }
        ZoomCommand::Refit => fit_owned_target(state, &profile),
        ZoomCommand::Constrain => ceiling_target(state, &profile),
        // The space around the page moved: dispatch to whichever of the two
        // owns the scale.
        ZoomCommand::Follow => {
            fit_owned_target(state, &profile).or_else(|| ceiling_target(state, &profile))
        }
    }
}

/// The scale the active fit mode wants, recorded as the reader's own.
fn fit_owned_target(state: &ReaderState, profile: &ZoomProfile) -> Option<f64> {
    let fit = state.viewer.fit.get_untracked();
    if fit == FitMode::None {
        return None;
    }
    let dims = FitDims::of(state)?;
    let target = profile.clamp(dims.fit(fit, state.viewer.zoom.visual_scale()));
    // A fit mode owns the ceiling too, or leaving it would resurrect an
    // old `desired`.
    state.viewer.zoom.desired.set(target);
    Some(target)
}

/// The ceiling a hand-picked zoom resolves to: the reader's own
/// `desired`, clamped and left alone.
fn ceiling_target(state: &ReaderState, profile: &ZoomProfile) -> Option<f64> {
    if state.viewer.fit.get_untracked() != FitMode::None {
        return None; // a fit mode owns the scale while it is active
    }
    // The reader's own `desired` is the ceiling: a too-wide page
    // overflows by design.
    Some(profile.clamp(state.viewer.zoom.desired.get_untracked()))
}

/// A PDF page host's report of a page's true size; an active fit
/// re-resolves.
#[cfg(feature = "pdf")]
pub(crate) fn page_rendered(state: ReaderState) -> Callback<(u32, f64, f64)> {
    Callback::new(move |(page, width, height): (u32, f64, f64)| {
        if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
            return;
        }
        let changed = state
            .document
            .content
            .metrics
            .record_rendered(page, width, height);
        if state.viewer.page.try_get_untracked() != Some(page) {
            return;
        }
        // An unchanged box is still a successful paint; geometry owns the
        // refit.
        let _ = state.viewer.first_paint.try_set(true);
        if !changed {
            return;
        }
        let fitting = state
            .viewer
            .fit
            .try_get_untracked()
            .is_some_and(|fit| fit != FitMode::None);
        if fitting && state.viewer.try_zooming_now() == Some(false) {
            state.viewer.zoom.post(ZoomCommand::Refit, false);
        }
    })
}

/// The size probe before the first raster; `true` means do not
/// rasterise at this scale.
#[cfg(feature = "pdf")]
pub(crate) fn page_sized(state: ReaderState, page: u32, width: f64, height: f64) -> bool {
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return false;
    }
    // Records the page's true box; `changed` is false for a known page.
    if !state
        .document
        .content
        .metrics
        .record_rendered(page, width, height)
    {
        return false;
    }
    // `try_` reads only: the probe can outlive its reader state.
    let Some(fit) = state.viewer.fit.try_get_untracked() else {
        return false;
    };
    // A look-ahead page is recorded, not fitted; a scroll re-asks.
    if state.viewer.page.try_get_untracked() != Some(page) {
        return false;
    }
    if fit == FitMode::None || state.viewer.try_zooming_now() != Some(false) {
        return false;
    }
    let Some(committed) = state.viewer.zoom.committed.try_get_untracked() else {
        return false;
    };
    let profile = zoom_profile();
    let Some(target) = fit_owned_target(&state, &profile) else {
        return false;
    };
    if (target - committed).abs() <= SETTLED_EPSILON {
        return false;
    }
    state.viewer.zoom.post(ZoomCommand::Refit, false);
    true
}

/// [`page_sized`] as the page host's callback.
#[cfg(feature = "pdf")]
pub(crate) fn page_sized_cb(state: ReaderState) -> Callback<(u32, f64, f64), bool> {
    Callback::new(move |(page, width, height)| page_sized(state, page, width, height))
}

/// The plain-geometry inputs of a fit computation, kept testable.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FitDims {
    /// Usable container width (margins removed), `>= 1`.
    pub cw_eff: f64,
    /// Usable container height; the full window height in every mode.
    pub ch_eff: f64,
    pub pw_eff: f64,
    pub ph_eff: f64,
    /// Whether the strip runs horizontally: Fit Page then uses height.
    pub horizontal: bool,
}

impl FitDims {
    /// Collect the fit inputs; `None` while either side is unmeasured.
    pub(crate) fn of(state: &ReaderState) -> Option<Self> {
        let page = state.viewer.page.get_untracked().max(1);
        // The page under the reader's eyes, not page 1: a plate fits on
        // its terms.
        let (pw, ph) = state.document.content.metrics.fit_size(page)?;
        // The column dial is reflowable-only; a PDF page IS the column.
        Self::from_geometry(
            state.viewer.mode.get_untracked(),
            state.viewer.container_size.get_untracked(),
            state.viewer.page_margin.get_untracked(),
            (pw, ph),
        )
    }

    /// The one definition of what a fit measures against, shared
    /// everywhere.
    pub(crate) fn from_geometry(
        mode: ViewMode,
        (cw, ch): (f64, f64),
        margin: f64,
        (pw, ph): (f64, f64),
    ) -> Option<Self> {
        if !(cw > 1.0 && ch > 1.0) {
            return None;
        }
        let cw_eff = (cw - 2.0 * margin).max(1.0);
        let ch_eff = ch.max(1.0);
        // Only the spread renders two pages; the strip lays out one.
        let pw_eff = if mode == ViewMode::Spread {
            pw * 2.0
        } else {
            pw
        };
        Some(Self {
            cw_eff,
            ch_eff,
            pw_eff,
            ph_eff: ph,
            horizontal: mode == ViewMode::ScrollHorizontal,
        })
    }

    /// The scale a fit mode wants; the horizontal strip fits one page per
    /// item.
    pub fn fit(&self, fit: FitMode, current: f64) -> f64 {
        if self.horizontal && fit == FitMode::Page {
            return clamp_scale(self.ch_eff / self.ph_eff.max(1.0));
        }
        fit_scale(
            fit,
            self.cw_eff,
            self.ch_eff,
            self.pw_eff,
            self.ph_eff,
            current,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dims(mode: ViewMode, cw: f64, ch: f64, pw: f64, ph: f64) -> FitDims {
        FitDims::from_geometry(mode, (cw, ch), 0.0, (pw, ph)).expect("measured container")
    }

    #[cfg(feature = "pdf")]
    #[test]
    fn unchanged_geometry_still_releases_current_page_paint_in_every_mode() {
        let owner = Owner::new();
        owner.with(|| {
            for mode in [
                ViewMode::Single,
                ViewMode::Spread,
                ViewMode::ScrollVertical,
                ViewMode::ScrollHorizontal,
            ] {
                let pane = crate::pane::handle::PaneHandle::new(
                    crate::host::model::PaneId::for_tests(1),
                    crate::runtime::ReaderRuntime::new(),
                );
                let state = ReaderState::new(pane);
                let size = reader_core::document::PageSize {
                    width: 612.0,
                    height: 792.0,
                };
                state
                    .document
                    .content
                    .metrics
                    .publish_uniform(2, &size, 792.0);
                state.viewer.mode.set(mode);
                let paint = page_rendered(state);
                paint.run((1, 612.0, 792.0));
                assert!(state.viewer.first_paint.get_untracked(), "{mode:?}");
                state.viewer.first_paint.set(false);
                paint.run((2, 792.0, 612.0));
                assert!(
                    !state.viewer.first_paint.get_untracked(),
                    "look-ahead {mode:?}"
                );
                paint.run((1, 0.0, 792.0));
                assert!(!state.viewer.first_paint.get_untracked(), "empty {mode:?}");
            }
        });
    }

    #[test]
    fn horizontal_fit_width_uses_one_page_while_fit_page_uses_height() {
        let d = dims(ViewMode::ScrollHorizontal, 1200.0, 600.0, 612.0, 792.0);
        let by_width = d.fit(FitMode::Width, 1.0);
        let by_page = d.fit(FitMode::Page, 1.0);
        assert!((by_width - 1200.0 / 612.0).abs() < 1e-9);
        assert!((by_page - 600.0 / 792.0).abs() < 1e-9);
        assert!(by_width > by_page);
    }

    #[test]
    fn a_spread_fits_two_pages_across() {
        let single = dims(ViewMode::Single, 1024.0, 768.0, 612.0, 792.0);
        let spread = dims(ViewMode::Spread, 1024.0, 768.0, 612.0, 792.0);
        assert!(
            (spread.fit(FitMode::Width, 1.0) - single.fit(FitMode::Width, 1.0) / 2.0).abs() < 1e-9
        );
    }

    #[test]
    fn a_vertical_fit_is_edge_to_edge_on_both_axes() {
        // Nothing is reserved for the overlay title bar: both axes span it.
        let vertical = dims(ViewMode::ScrollVertical, 1000.0, 800.0, 500.0, 700.0);
        let by_w = 1000.0 / 500.0;
        let by_h = 800.0 / 700.0;
        assert!((vertical.fit(FitMode::Width, 1.0) - by_w).abs() < 1e-9);
        assert!((vertical.fit(FitMode::Page, 1.0) - by_w.min(by_h)).abs() < 1e-9);
    }

    #[test]
    fn the_reader_margin_comes_off_the_width_only() {
        let d = FitDims::from_geometry(
            ViewMode::ScrollVertical,
            (1000.0, 800.0),
            20.0,
            (500.0, 700.0),
        )
        .unwrap();
        assert!((d.fit(FitMode::Width, 1.0) - 960.0 / 500.0).abs() < 1e-9);
        assert!(
            FitDims::from_geometry(ViewMode::Single, (0.0, 800.0), 0.0, (500.0, 700.0)).is_none()
        );
    }

    #[test]
    fn a_pdf_fit_width_spans_the_container_with_no_leftover_pan_space() {
        // The contract the column dial used to break: a width fit spans the
        // row exactly.
        let d = dims(ViewMode::ScrollVertical, 1000.0, 800.0, 500.0, 700.0);
        let scale = d.fit(FitMode::Width, 1.0);
        assert!((scale - 2.0).abs() < 1e-9);
        assert!(
            (500.0 * scale - 1000.0).abs() < 1e-9,
            "page must exactly fill the row"
        );
    }

    #[test]
    fn a_measured_narrow_container_keeps_the_effective_width_fallback() {
        let d = FitDims::from_geometry(
            ViewMode::ScrollVertical,
            (30.0, 800.0),
            20.0,
            (500.0, 700.0),
        )
        .expect("the raw container is measured");
        assert_eq!(d.cw_eff, 1.0);
        assert_eq!(d.ch_eff, 800.0);
    }
}
