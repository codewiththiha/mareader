//! Target resolution: turning a `ZoomCommand` plus the current context
//! into one concrete scale. Manual steps, fit modes and window constraints
//! all resolve here so they cannot drift apart.
//!
//! Resolution also records the intent: a manual zoom writes `desired` and
//! clears the fit mode (a fit ceiling must never fight a gesture), a fit
//! refresh writes `desired` to the resolved fit, and a window constraint
//! deliberately leaves `desired` untouched — the reader's chosen zoom is
//! the ceiling, so a manual zoom can go as far as the clamp allows and is
//! never shrunk back to the fit width. A container follow answers with
//! whichever of those two owns the scale right now, so the same numbers
//! govern a slide, a drag and a pause after one.

use leptos::prelude::*;

use reader_core::view::ViewMode;
use reader_core::zoom_math::{FitMode, clamp_scale, fit_scale, nearest_zoom};

use crate::state::reader::{ReaderState, ZoomCommand};

use super::config::{SETTLED_EPSILON, ZoomProfile, profile_for};

/// Resolve a command to the scale it wants, or `None` when it must stand
/// down (nothing to re-resolve, an unmeasured container, an unmeasured
/// document).
///
/// `in_flight` is the target of a transition already running; manual steps
/// chain from it so a fast `+ +` advances two presets rather than resolving
/// the same one twice. Every read here is untracked — the caller's effect
/// subscribes to the command signal and nothing else.
pub(crate) fn resolve(state: &ReaderState, cmd: ZoomCommand, in_flight: Option<f64>) -> Option<f64> {
    let zoom = state.viewer.zoom;
    let profile = profile_for(state.viewer.mode.get_untracked());
    match cmd {
        ZoomCommand::Step(dir) => {
            // Step from the in-flight target while a tween runs, else from
            // the settled scale. Mid-animation values are deliberately
            // avoided: nearest_zoom would usually round to the preset the
            // tween is already heading towards and swallow the press.
            let base = in_flight.unwrap_or_else(|| zoom.visual_scale());
            let target = profile.clamp(nearest_zoom(base, dir));
            // At the end of the ladder `nearest_zoom` answers with the same
            // preset it was given, so there is nowhere to go. Bail BEFORE
            // recording any intent: writing `desired` and clearing the fit
            // mode here would silently drop the reader out of Fit Width just
            // because they leaned on a zoom button that had nothing left to
            // do. The coordinator bails on an unchanged target too; this is
            // what keeps the state untouched.
            if (target - base).abs() < SETTLED_EPSILON {
                return None;
            }
            zoom.desired.set(target);
            state.viewer.fit.set(FitMode::None);
            Some(target)
        }
        ZoomCommand::Refit => fit_owned_target(state, &profile),
        ZoomCommand::Constrain => ceiling_target(state, &profile),
        // The space around the page moved. Both watchers' cases are the same
        // question — what does the current width deserve? — and exactly one of
        // them owns the answer: a fit mode does while it is active, the
        // reader's own chosen zoom otherwise. Dispatching here instead of letting
        // the watcher choose keeps the two from disagreeing about who is in
        // charge, which is how a slide used to end with the page at a scale
        // neither of them had asked for.
        ZoomCommand::Follow => {
            fit_owned_target(state, &profile).or_else(|| ceiling_target(state, &profile))
        }
    }
}

/// The scale the active fit mode wants, recorded as the reader's own choice.
///
/// `None` with no fit mode: a refit of a hand-picked zoom would resolve to the
/// current scale AND clobber `desired`, resurrecting an old number as the
/// ceiling. Callers post it only while a fit is active; this is what keeps the
/// resolver safe on its own terms.
fn fit_owned_target(state: &ReaderState, profile: &ZoomProfile) -> Option<f64> {
    let fit = state.viewer.fit.get_untracked();
    if fit == FitMode::None {
        return None;
    }
    let dims = FitDims::of(state)?;
    let target = profile.clamp(dims.fit(fit, state.viewer.zoom.visual_scale()));
    // A fit mode IS a deliberate choice, so it owns the ceiling too. Without
    // this, leaving the fit mode would resurrect a `desired` from some earlier
    // gesture and the page would jump to it.
    state.viewer.zoom.desired.set(target);
    Some(target)
}

/// The ceiling a hand-picked zoom resolves to: the reader's own `desired`,
/// clamped. A manual zoom is authoritative rather than capped at the fit width,
/// so a page the reader zoomed in on stays at that scale (and overflows with a
/// scroll affordance). `desired` is deliberately left alone, which is what
/// makes it stable: the same number governs a slide, a drag and the pause after
/// one, and a container follow resolves to exactly what they chose. Computing
/// from `desired` — never from the live scale times a container ratio — is also
/// why a slide does not accumulate rounding and land somewhere the reader never
/// asked for.
fn ceiling_target(state: &ReaderState, profile: &ZoomProfile) -> Option<f64> {
    if state.viewer.fit.get_untracked() != FitMode::None {
        return None; // a fit mode owns the scale while it is active
    }
    // A hand-picked zoom is authoritative up to the profile's clamp; the old
    // shrink-to-fit ceiling (`min(desired, fit_width)`) quietly locked manual
    // zoom at the page's fit-width scale, so a reader could never look at a
    // page up close. Free zoom lets a too-wide page overflow and scroll — a
    // deliberate affordance — instead of snapping back to the fit width. The
    // reader's own `desired` is the ceiling, so the same number governs a
    // slide, a drag and the pause after one, and a container follow resolves
    // to exactly what they chose rather than a size the app picked.
    Some(profile.clamp(state.viewer.zoom.desired.get_untracked()))
}

/// The plain-geometry inputs of a fit computation, separated from the
/// reactive state so the arithmetic is unit-testable on the host.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FitDims {
    /// Usable container width (margins removed), `>= 1`.
    pub cw_eff: f64,
    /// Usable container height (`>= 1`); the full window height in every
    /// mode, the title bar being a hover-revealed overlay rather than a band.
    pub ch_eff: f64,
    /// Effective page width (doubled in spread mode).
    pub pw_eff: f64,
    /// Effective page height.
    pub ph_eff: f64,
    /// Whether the strip runs horizontally. In that mode Fit Page uses the
    /// viewport height, while Fit Width still means the width of one page.
    pub horizontal: bool,
}

impl FitDims {
    /// Collect the fit inputs from the reader state. `None` while the
    /// container or the document is still unmeasured — fitting to a
    /// placeholder would slam the page to the minimum scale.
    pub(crate) fn of(state: &ReaderState) -> Option<Self> {
        let page = state.viewer.page.get_untracked().max(1);
        let p1 = state.document.content.metrics.page1_size.get_untracked()?;

        // The page under the reader's eyes, not page 1: a landscape plate in
        // an otherwise-portrait book must fit on its own terms.
        let (pw, ph) = state.document.content.metrics.intrinsic.with_untracked(|sizes| {
            match sizes.get((page - 1) as usize) {
                Some(s) if s.width > 0.0 && s.height > 0.0 => (s.width, s.height),
                _ => (p1.width, p1.height),
            }
        });
        Self::from_geometry(
            state.viewer.mode.get_untracked(),
            state.viewer.container_size.get_untracked(),
            state.viewer.page_margin.get_untracked(),
            (pw, ph),
        )
    }

    /// The fit geometry for a page of `(pw, ph)` in a `(cw, ch)` container —
    /// the ONE definition of what a fit measures against, shared by the live
    /// fit and the open flow's seed scale so the first frame and the first
    /// refit agree. The reader margin comes off the width, every mode keeps
    /// the full height (the title bar is an overlay, not a band) and a
    /// spread doubles the page width. `None` when either raw container
    /// dimension is unmeasured.
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
        // Only the spread renders a true two-page spread; the horizontal
        // strip lays out one page per virtual item.
        let pw_eff = if mode == ViewMode::Spread { pw * 2.0 } else { pw };
        Some(Self {
            cw_eff,
            ch_eff,
            pw_eff,
            ph_eff: ph,
            horizontal: mode == ViewMode::ScrollHorizontal,
        })
    }

    /// The scale a fit mode wants. The horizontal strip has one page per
    /// virtual item: Fit Width therefore uses that page's width, while Fit
    /// Page keeps the height-fit behaviour that makes the full page visible.
    /// `None` is included for completeness even though callers only ask this
    /// method to resolve an active fit.
    pub fn fit(&self, fit: FitMode, current: f64) -> f64 {
        if self.horizontal && fit == FitMode::Page {
            return clamp_scale(self.ch_eff / self.ph_eff.max(1.0));
        }
        fit_scale(fit, self.cw_eff, self.ch_eff, self.pw_eff, self.ph_eff, current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dims(mode: ViewMode, cw: f64, ch: f64, pw: f64, ph: f64) -> FitDims {
        FitDims::from_geometry(mode, (cw, ch), 0.0, (pw, ph)).expect("measured container")
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
        assert!((spread.fit(FitMode::Width, 1.0) - single.fit(FitMode::Width, 1.0) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_vertical_fit_is_edge_to_edge_on_both_axes() {
        // Nothing is reserved for the overlay title bar: Fit Width spans the
        // full width and Fit Page the full height.
        let vertical = dims(ViewMode::ScrollVertical, 1000.0, 800.0, 500.0, 700.0);
        let by_w = 1000.0 / 500.0;
        let by_h = 800.0 / 700.0;
        assert!((vertical.fit(FitMode::Width, 1.0) - by_w).abs() < 1e-9);
        assert!((vertical.fit(FitMode::Page, 1.0) - by_w.min(by_h)).abs() < 1e-9);
    }

    #[test]
    fn the_reader_margin_comes_off_the_width_only() {
        let d = FitDims::from_geometry(ViewMode::ScrollVertical, (1000.0, 800.0), 20.0, (500.0, 700.0))
            .unwrap();
        assert!((d.fit(FitMode::Width, 1.0) - 960.0 / 500.0).abs() < 1e-9);
        assert!(FitDims::from_geometry(ViewMode::Single, (0.0, 800.0), 0.0, (500.0, 700.0)).is_none());
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

    use reader_core::zoom_math::{MAX_SCALE, MIN_SCALE};

    #[test]
    fn a_manual_zoom_is_never_capped_at_fit_width() {
        // The ceiling a container follow applies to a hand-picked zoom is the
        // reader's own choice, clamped into the range — not the fit width. A
        // page fit to an 800px container at 612px wide sits at ~1.31, so
        // zooming to 2.0 (well past that "fit width") must survive a follow
        // and a constrain unchanged, so a reader can inspect a page up close.
        // `ceiling_target` is the only place a follow/constrain with no active
        // fit resolves, and it does `profile.clamp(desired)`.
        let profile = profile_for(ViewMode::ScrollVertical);
        assert_eq!(profile.clamp(2.0), 2.0);
        assert_eq!(profile.clamp(5.0), MAX_SCALE);
        assert_eq!(profile.clamp(0.1), MIN_SCALE);
    }
}
