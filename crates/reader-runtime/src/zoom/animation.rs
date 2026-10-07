//! The zoom tween: the layout is what animates.

use leptos::prelude::*;

use app_chrome::hooks::use_raf::FrameLoop;

use crate::state::{ReaderState, ZoomTransition};
use crate::zoom::actuator::{PendingScroll, ZoomActuator};
use app_ui::components::primitives::motion::reduced_motion::prefers_reduced_motion;

use super::config;
use super::coordinator::finish_transition;

/// Whether the displayed scale already sits at `to`.
fn settled(state: &ReaderState, to: f64) -> bool {
    (to - state.viewer.zoom.visual_scale()).abs() < config::SETTLED_EPSILON
}

/// Relay the layout at `to`, then show it.
fn relay(
    state: &ReaderState,
    actuator: &ZoomActuator,
    to: f64,
    detached: bool,
) -> Option<PendingScroll> {
    // A detached relay hands the scroll write back.
    let cur = state.viewer.zoom.visual_scale();
    let pending = if state.viewer.mode.get_untracked().is_paginated() {
        None
    } else if detached {
        actuator.relayout_detached(state, to / cur)
    } else {
        actuator.relayout_to(state, to / cur);
        None
    };
    state.viewer.zoom.display.set(to);
    pending
}

/// Land a transaction: relay the layout to its target, then show it.
pub(crate) fn land(state: &ReaderState, actuator: &ZoomActuator, t: &ZoomTransition) -> bool {
    if settled(state, t.to) {
        return false;
    }
    relay(state, actuator, t.to, false);
    true
}

/// The tween's curve: covers ground early, decelerates onto the
/// target.
fn ease_out_cubic(t: f64) -> f64 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

/// Whether a transition eases between its endpoints over frames.
pub(crate) fn interpolates(
    t: &ZoomTransition,
    duration_ms: f64,
    reader_allows: bool,
    reduced_motion: bool,
) -> bool {
    t.animate && !t.following && duration_ms > 0.0 && reader_allows && !reduced_motion
}

/// [`interpolates`], against the live preferences.
pub(crate) fn interpolates_now(state: &ReaderState, t: &ZoomTransition) -> bool {
    interpolates(
        t,
        config::zoom_profile().duration_ms(),
        state.viewer.motion.get_untracked().zoom,
        prefers_reduced_motion(),
    )
}

/// Whether `a` and `b` are the same transaction.
fn same_transaction(a: &ZoomTransition, b: &ZoomTransition) -> bool {
    a.start_ms == b.start_ms && a.from == b.from && a.to == b.to && a.following == b.following
}

/// Run a whole untweened transaction: open, move, write scroll,
/// commit.
pub(crate) fn commit_instant(state: &ReaderState, actuator: &ZoomActuator, t: &ZoomTransition) {
    state.viewer.zoom.transition.set(Some(*t));
    let pending = land_detached(state, actuator, t);
    let (state, actuator, t) = (*state, actuator.clone(), *t);
    leptos::task::spawn_local(async move {
        let current = state.viewer.zoom.transition.try_get_untracked().flatten();
        if !current.is_some_and(|open| same_transaction(&open, &t)) {
            return;
        }
        if let Some(pending) = pending {
            actuator.write_scroll(pending);
        }
        finish_transition(&state, &t);
    });
}

/// `land` without touching the scroll surface.
fn land_detached(
    state: &ReaderState,
    actuator: &ZoomActuator,
    t: &ZoomTransition,
) -> Option<PendingScroll> {
    if settled(state, t.to) {
        return None;
    }
    relay(state, actuator, t.to, true)
}

/// The single tween loop owned by the zoom controller.
pub(crate) struct Tween {
    frames: FrameLoop,
}

impl Tween {
    /// Build from the reader's owner.
    pub(crate) fn new() -> Self {
        Self {
            frames: FrameLoop::new(),
        }
    }

    /// Ensure a loop is running for the current transition.
    pub(crate) fn arm(&self, state: ReaderState, actuator: ZoomActuator) {
        self.frames.arm(move || {
            // Idle? The loop dies here until the next `arm`.
            let Some(t) = state.viewer.zoom.transition.get_untracked() else {
                return false;
            };
            if !interpolates_now(&state, &t) {
                if t.following {
                    // A follow that took over mid-tween
                    // LANDS but does not commit.
                    land(&state, &actuator, &t);
                    return false;
                }
                // Animation switched off while this tween ran: finish it in one
                // step.
                commit_instant(&state, &actuator, &t);
                return false;
            }
            let duration = config::zoom_profile().duration_ms();
            let progress = ((js_sys::Date::now() - t.start_ms) / duration).clamp(0.0, 1.0);
            let visual = t.from + (t.to - t.from) * ease_out_cubic(progress);
            relay(&state, &actuator, visual, false);
            if progress >= 1.0 {
                // `show` can swap the signal for a newer transition;
                // finishing follows it.
                let Some(cur) = state.viewer.zoom.transition.get_untracked() else {
                    return false;
                };
                if !same_transaction(&cur, &t) {
                    return true;
                }
                finish_transition(&state, &cur);
                return false;
            }
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transition(animate: bool, following: bool) -> ZoomTransition {
        ZoomTransition {
            from: 1.0,
            to: 1.25,
            start_ms: 0.0,
            animate,
            following,
        }
    }

    #[test]
    fn a_retarget_is_a_different_transaction_even_in_the_same_millisecond() {
        let first = transition(false, false);
        let retarget = ZoomTransition {
            from: first.to,
            to: 1.5,
            ..first
        };
        assert!(same_transaction(&first, &first));
        assert!(!same_transaction(&first, &retarget));
        // A follow taking over at the same scales is a different owner too.
        let follow = ZoomTransition {
            following: true,
            ..first
        };
        assert!(!same_transaction(&first, &follow));
    }

    #[test]
    fn a_plain_zoom_interpolates_when_everything_allows_it() {
        assert!(interpolates(&transition(true, false), 180.0, true, false));
    }

    #[test]
    fn animation_off_never_interpolates() {
        // The reader's switch, reduced motion, a zero-length profile: one
        // step.
        assert!(!interpolates(&transition(true, false), 180.0, false, false));
        assert!(!interpolates(&transition(true, false), 180.0, true, true));
        assert!(!interpolates(&transition(true, false), 0.0, true, false));
        assert!(!interpolates(&transition(false, false), 180.0, true, false));
    }

    #[test]
    fn a_follow_never_interpolates() {
        assert!(!interpolates(&transition(true, true), 180.0, true, false));
        assert!(!interpolates(&transition(false, true), 180.0, true, false));
    }

    #[test]
    fn ease_starts_fast_and_lands_exactly() {
        assert!(ease_out_cubic(0.1) > 0.27); // out-cubic covers ground early
        assert!((ease_out_cubic(1.0) - 1.0).abs() < 1e-12);
        assert_eq!(ease_out_cubic(0.0), 0.0);
        // Out-of-range inputs must not overshoot the endpoints.
        assert_eq!(ease_out_cubic(-1.0), 0.0);
        assert_eq!(ease_out_cubic(2.0), 1.0);
    }
}
