//! The zoom tween.
//!
//! Every animation frame does exactly one thing: work out the scale the eye
//! should be at and hand the actuator the ratio between it and the scale the
//! layout has. The actuator rescales the strips and holds the document point
//! under the viewport centre where it is, while the page hosts stretch the
//! bitmap they already hold — the reader watches the paper itself change size,
//! with nothing to capture before the gesture and nothing to restore after.
//!
//! The layout is what animates, on purpose. One CSS transform over a frozen
//! surface looks stable while it runs and jumps at the end: a transform scales
//! the page gaps with the pages, the layout deliberately does not, so the
//! accumulated gap error lands at once at the commit.
//!
//! The paginated modes have no strip to rescale; there the frame just moves
//! the display scale and the single mounted host stretches to it. The
//! interpolation is an out-cubic — covers ground early, decelerates into the
//! target instead of stopping dead.
//!
//! The loop reads the live `zoom.transition` signal each frame, so a retarget
//! mid-flight (a burst of `+`, a sidebar still sliding) is adopted seamlessly:
//! the tween continues from wherever the eye is towards the new target, on a
//! restarted clock.
//!
//! An untweened zoom — animation off, reduced motion, or a poster that asked
//! for none — never comes through this loop at all: [`commit_instant`] runs
//! the whole transaction in the task that posted it.
//!
//! A container follow does not normally come through this loop either: its target is
//! whatever the container allows RIGHT NOW, so the controller lands it in the
//! frame the new size was reported and holds the commit for the burst's end —
//! easing towards a moving target has the page visibly chasing the window. The
//! loop can still be handed one (a follow taking over mid-tween), so it knows
//! how to land it without committing it.

use leptos::prelude::*;

use app_chrome::hooks::use_raf::FrameLoop;

use crate::state::{ReaderState, ZoomTransition};
use crate::zoom::actuator::ZoomActuator;
use app_ui::components::primitives::motion::reduced_motion::prefers_reduced_motion;

use super::config;
use super::coordinator::finish_transition;

/// Land a transaction: relay the layout out to its target, then show it.
///
/// Answers `false` when the scale has nowhere to go — the target is the one on
/// screen — in which case NOTHING was written. Not a micro-optimisation: a
/// Leptos `set` notifies even when unchanged, so an unconditional write on a
/// settled target re-runs every mounted page's stretch effect and rebuilds
/// both strips for a factor of one. A container follow asks for the landing on
/// every frame of a burst, so it hits that case whenever the scale is pinned.
///
/// Callers: the controller in the task that reported a new container size,
/// [`commit_instant`] for every untweened zoom, and the tween loop for a
/// follow that took over mid-tween. All go
/// through here so "the layout moved and the display scale agrees" stays one
/// rule rather than two that can drift.
pub(crate) fn land(state: &ReaderState, actuator: &ZoomActuator, t: &ZoomTransition) -> bool {
    let cur = state.viewer.zoom.visual_scale();
    if (t.to - cur).abs() < config::SETTLED_EPSILON {
        return false;
    }
    show(state, actuator, t.to);
    true
}

/// The pair every change of the scale on screen is: relay the layout out to
/// `to`, then show it. The actuator reads `display` to work out the horizontal
/// strip's exact widths, so the relayout must come first.
///
/// Only the scrolling modes have a strip to rescale; for the paginated ones
/// the single mounted host stretches to the display scale on its own.
fn show(state: &ReaderState, actuator: &ZoomActuator, to: f64) {
    let cur = state.viewer.zoom.visual_scale();
    if !state.viewer.mode.get_untracked().is_paginated() {
        actuator.relayout_to(state, to / cur);
    }
    state.viewer.zoom.display.set(to);
}

/// The tween's progress curve: covers ground early, decelerates onto the
/// target scale instead of stopping dead on it.
fn ease_out_cubic(t: f64) -> f64 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

/// Whether a transition eases between its endpoints over animation frames.
///
/// Five reasons not to: the poster asked for none, it is a container follow
/// (it must sit in the window, not chase it), the profile has no duration,
/// the reader switched zoom animation off, or the OS asked for reduced
/// motion. Every consumer decides through here — the command effect when it
/// opens a transaction, and the tween loop on every frame, so a preference
/// that flips mid-flight lands the tween instead of finishing it.
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

/// Run a whole untweened transaction in the calling task: open it, land the
/// layout on the target, commit.
///
/// This is what "animation off" means for a zoom — one discrete change, not a
/// tween with its frames removed. Nothing is deferred to a later frame, so
/// there is no window in which the view could show a half-applied state and
/// no frame count standing in for a correctness condition:
///
/// * the actuator brings the strip's scroll extent to the new total BEFORE
///   its scroll write, so the anchored offset is exact in this tick (no
///   clamped write, no correction a frame later — see `relayout_vertical`);
/// * the page hosts stretch their CURRENT bitmaps to the target size, and the
///   commit's renders draw into scratches and swap in one blit each, so no
///   page is ever blank while its crisp raster is pending;
/// * everything that runs afterwards — the scroll event, the hosts'
///   ResizeObserver reports, the render completions — observes the committed
///   geometry, and a render issued before the zoom is refused at landing
///   (`judge_completion` in the page host).
///
/// The transition still goes up before anything moves and comes down in
/// [`finish_transition`]: the freeze bookkeeping in the controller watches
/// that signal, and it is what re-arms the retention-grace reset.
pub(crate) fn commit_instant(state: &ReaderState, actuator: &ZoomActuator, t: &ZoomTransition) {
    state.viewer.zoom.transition.set(Some(*t));
    land(state, actuator, t);
    finish_transition(state, t);
}

/// The single tween loop owned by the zoom controller. A thin wrapper over
/// [`FrameLoop`]: the machinery (re-arm slot, alive flag, owner cleanup) is
/// the primitive's; what is left here is the one thing only the tween knows —
/// what a frame does. `arm` is idempotent: a running loop adopts whatever
/// transition is on the signal, so retargets never stack a second loop.
///
/// Only interpolating transactions come here; an untweened one never reaches
/// the loop (see [`commit_instant`]).
pub(crate) struct Tween {
    frames: FrameLoop,
}

impl Tween {
    /// Build from the reader's owner — this is called in a component body, next
    /// to `drive`, and the loop's cleanup is registered here.
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
                    // A follow that took over mid-tween LANDS but must not
                    // commit: its burst has another frame coming, and a raster
                    // pass per frame of a slide is the storm the held
                    // transaction exists to avoid. The controller's settle
                    // deadline commits it once the container stops moving.
                    // Going idle here instead of re-arming lets the next frame
                    // own the next rAF: `arm` adopts whatever transition is on
                    // the signal.
                    land(&state, &actuator, &t);
                    return false;
                }
                // Animation switched off (or reduced motion turned on) while
                // this tween ran: finish it the way an untweened zoom runs,
                // in one step, right now.
                commit_instant(&state, &actuator, &t);
                return false;
            }
            let duration = config::zoom_profile().duration_ms();
            let progress = ((js_sys::Date::now() - t.start_ms) / duration).clamp(0.0, 1.0);
            let visual = t.from + (t.to - t.from) * ease_out_cubic(progress);
            show(&state, &actuator, visual);
            if progress >= 1.0 {
                finish_transition(&state, &t);
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
    fn a_plain_zoom_interpolates_when_everything_allows_it() {
        assert!(interpolates(&transition(true, false), 180.0, true, false));
    }

    #[test]
    fn animation_off_never_interpolates() {
        // The reader's switch, reduced motion, a zero-length profile and a
        // poster that asked for none each land the zoom in one step.
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
