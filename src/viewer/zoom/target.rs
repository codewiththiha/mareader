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

use pdf_core::layout::{TOOLBAR_H, ViewMode};
use pdf_core::math::{clamp_scale, fit_scale, nearest_zoom, FitMode};

use crate::state::reader::{ZoomCommand, ReaderState};

use super::config::{profile_for, SETTLED_EPSILON, ZoomProfile};

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
    /// Usable container height (`>= 1`; the toolbar band is reserved only
    /// for the vertical strip, which scrolls under a fixed bar).
    pub ch_eff: f64,
    /// Effective page width (doubled in spread mode).
    pub pw_eff: f64,
    /// Effective page height.
    pub ph_eff: f64,
    /// Vertical air a fit must leave in scrolling-vertical mode.
    pub pad: f64,
    /// Whether the strip runs horizontally. In that mode Fit Page uses the
    /// viewport height, while Fit Width still means the width of one page.
    pub horizontal: bool,
}

impl FitDims {
    /// Collect the fit inputs from the reader state. `None` while the
    /// container or the document is still unmeasured — fitting to a
    /// placeholder would slam the page to the minimum scale.
    pub(crate) fn of(state: &ReaderState) -> Option<Self> {
        let mode = state.viewer.mode.get_untracked();
        let (cw, ch) = state.viewer.container_size.get_untracked();
        let margin = state.viewer.page_margin.get_untracked();
        let page = state.viewer.page.get_untracked().max(1);
        let p1 = state.document.page1_size.get_untracked()?;

        // The page under the reader's eyes, not page 1: a landscape plate in
        // an otherwise-portrait book must fit on its own terms.
        let (pw, ph) = state.document.metrics.intrinsic.with_untracked(|sizes| {
            match sizes.get((page - 1) as usize) {
                Some(s) if s.width > 0.0 && s.height > 0.0 => (s.width, s.height),
                _ => (p1.width, p1.height),
            }
        });

        let horizontal = mode == ViewMode::ScrollHorizontal;
        let cw_eff = (cw - 2.0 * margin).max(1.0);
        // The horizontal strip joins the paginated modes: it owns the full
        // window height and the auto-hiding title bar overlays it. Reserving
        // that band would leave a permanent dead strip above the pages.
        let ch_eff = if mode.is_paginated() || horizontal {
            ch.max(1.0)
        } else {
            (ch - TOOLBAR_H).max(1.0)
        };
        // Only the spread renders a true two-page spread; the horizontal
        // strip lays out one page per virtual item.
        let spread = matches!(mode, ViewMode::Spread);
        let (pw_eff, ph_eff) = if spread { (pw * 2.0, ph) } else { (pw, ph) };
        let pad = if mode.is_paginated() || horizontal {
            0.0
        } else {
            TOOLBAR_H
        };

        if cw_eff <= 1.0 || ch_eff <= 1.0 {
            return None;
        }

        Some(Self {
            cw_eff,
            ch_eff,
            pw_eff,
            ph_eff,
            pad,
            horizontal,
        })
    }

    /// The scale a fit mode wants. The horizontal strip has one page per
    /// virtual item: Fit Width therefore uses that page's width, while Fit
    /// Page keeps the height-fit behaviour that makes the full page visible.
    /// `None` is included for completeness even though callers only ask this
    /// method to resolve an active fit.
    pub fn fit(&self, fit: FitMode, current: f64) -> f64 {
        if self.horizontal {
            return match fit {
                FitMode::Width => clamp_scale(self.cw_eff.max(1.0) / self.pw_eff.max(1.0)),
                FitMode::Page => clamp_scale(self.ch_eff.max(1.0) / self.ph_eff.max(1.0)),
                FitMode::None => clamp_scale(current),
            };
        }
        fit_scale(
            fit,
            self.cw_eff,
            self.ch_eff,
            self.pw_eff,
            self.ph_eff,
            self.pad,
            current,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dims(horizontal: bool, spread: bool, cw: f64, ch: f64, pw: f64, ph: f64, pad: f64) -> FitDims {
        FitDims {
            cw_eff: cw,
            ch_eff: ch,
            pw_eff: if spread { pw * 2.0 } else { pw },
            ph_eff: ph,
            pad,
            horizontal,
        }
    }

    #[test]
    fn horizontal_fit_width_uses_one_page_while_fit_page_uses_height() {
        let d = dims(true, false, 1200.0, 600.0, 612.0, 792.0, 0.0);
        let by_width = d.fit(FitMode::Width, 1.0);
        let by_page = d.fit(FitMode::Page, 1.0);
        assert!((by_width - 1200.0 / 612.0).abs() < 1e-9);
        assert!((by_page - 600.0 / 792.0).abs() < 1e-9);
        assert!(by_width > by_page);
    }

    #[test]
    fn a_spread_fits_two_pages_across() {
        let single = dims(false, false, 1024.0, 768.0, 612.0, 792.0, 0.0);
        let spread = dims(false, true, 1024.0, 768.0, 612.0, 792.0, 0.0);
        assert!((spread.fit(FitMode::Width, 1.0) - single.fit(FitMode::Width, 1.0) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn the_toolbar_band_is_reserved_only_where_the_strip_scrolls_under_it() {
        // Vertical: fit-page leaves the toolbar band as air on the height.
        let vertical = dims(false, false, 1000.0, 800.0, 500.0, 700.0, 56.0);
        assert!((vertical.fit(FitMode::Page, 1.0) - (800.0 - 56.0) / 700.0).abs() < 1e-9);
        // fit_scale subtracts the pad from the constrained axis in both
        // modes, so the width fit keeps it as side air too.
        assert!((vertical.fit(FitMode::Width, 1.0) - (1000.0 - 56.0) / 500.0).abs() < 1e-9);
    }

    use pdf_core::math::{MAX_SCALE, MIN_SCALE};

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
