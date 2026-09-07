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
//! A container follow does not normally come through this loop: its target is
//! whatever the container allows RIGHT NOW, so the controller lands it in the
//! frame the new size was reported and holds the commit for the burst's end —
//! easing towards a moving target has the page visibly chasing the window. The
//! loop can still be handed one (a follow taking over mid-tween), so it knows
//! how to land it without committing it.

use leptos::prelude::*;

use app_chrome::hooks::use_raf::FrameLoop;

use crate::components::primitives::motion::reduced_motion::prefers_reduced_motion;
use crate::state::reader::{ReaderState, ZoomTransition};
use crate::zoom::actuator::ZoomActuator;

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
/// Two callers, deliberately: the controller in the task that reported a new
/// container size, and the tween loop for every untweened landing. Both go
/// through here so "the layout moved and the display scale agrees" stays one
/// rule rather than two that can drift.
pub(crate) fn land(state: &ReaderState, actuator: &ZoomActuator, t: &ZoomTransition) -> bool {
    let cur = state.viewer.zoom.visual_scale();
    if (t.to - cur).abs() < config::SETTLED_EPSILON {
        return false;
    }
    // Only the scrolling modes have a strip to rescale; for the paginated ones
    // the single mounted host stretches to the display scale on its own.
    if !state.viewer.mode.get_untracked().is_paginated() {
        actuator.relayout_to(state, t.to / cur);
    }
    state.viewer.zoom.display.set(t.to);
    true
}

/// The tween's progress curve: covers ground early, decelerates onto the
/// target scale instead of stopping dead on it.
fn ease_out_cubic(t: f64) -> f64 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

/// The single tween loop owned by the zoom controller. A thin wrapper over
/// [`FrameLoop`]: the machinery (re-arm slot, alive flag, owner cleanup) is
/// the primitive's; what is left here is the one thing only the tween knows —
/// what a frame does. `arm` is idempotent: a running loop adopts whatever
/// transition is on the signal, so retargets never stack a second loop.
pub(crate) struct Tween {
    frames: FrameLoop,
}

impl Tween {
    /// Build from the reader's owner — this is called in a component body, next
    /// to `drive`, and the loop's cleanup is registered here.
    pub(crate) fn new() -> Self {
        Self { frames: FrameLoop::new() }
    }

    /// Ensure a loop is running for the current transition.
    pub(crate) fn arm(&self, state: ReaderState, actuator: ZoomActuator) {
        self.frames.arm(move || {
            // Idle? The loop dies here until the next `arm`.
            let Some(t) = state.viewer.zoom.transition.get_untracked() else {
                return false;
            };
            let mode = state.viewer.mode.get_untracked();
            // Only the scrolling modes have a strip to rescale.
            let scrolls = !mode.is_paginated();
            let duration = config::profile_for(mode).duration_ms();
            // Five reasons not to interpolate: the poster asked for the first
            // frame, this is a container follow (it must sit in the window,
            // not chase it), the profile has no duration, the OS asked for
            // reduced motion, or the reader switched zoom animation off. The
            // last two are read here rather than at every `post`, so no
            // surface can bypass them by forgetting to ask.
            let reader_allows = state.viewer.motion.get_untracked().zoom;
            if !t.animate
                || t.following
                || duration <= 0.0
                || !reader_allows
                || prefers_reduced_motion()
            {
                // Landing without a tween: one relayout to the target, then
                // the commit.
                land(&state, &actuator, &t);
                if t.following {
                    // A held follow LANDS but must not commit: its burst has
                    // another frame coming, and a raster pass per frame of a
                    // slide is the storm the held transaction exists to avoid.
                    // The controller's settle deadline commits it once the
                    // container stops moving. Going idle here instead of
                    // re-arming lets the next frame own the next rAF: `arm`
                    // adopts whatever transition is on the signal.
                                        return false;
                }
                finish_transition(&state, &t);
                                return false;
            }
            let progress = ((js_sys::Date::now() - t.start_ms) / duration).clamp(0.0, 1.0);
            let visual = t.from + (t.to - t.from) * ease_out_cubic(progress);
            // The per-frame pair: relay the layout out by the ratio the
            // display scale is about to move through, then show it. The
            // actuator reads `display` to work out the horizontal strip's
            // exact widths, so the relayout must come first.
            if scrolls {
                let cur = state.viewer.zoom.visual_scale();
                actuator.relayout_to(&state, visual / cur);
            }
            state.viewer.zoom.display.set(visual);
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
