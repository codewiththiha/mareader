//! The zoom tween.
//!
//! Every animation frame does exactly one thing: work out the scale the eye
//! should be at, and hand the engine the ratio between that scale and the
//! scale the layout currently has. The engine rescales the strips and holds
//! the document point under the viewport centre exactly where it is, while
//! the page hosts stretch the bitmap they already hold to the new size — so
//! the reader watches the paper itself change size, with nothing to capture
//! before the gesture and nothing to restore after it.
//!
//! The layout is what animates, on purpose. Scaling a frozen surface with one
//! CSS transform instead looks stable while it runs and jumps at the end: a
//! transform scales the page gaps along with the pages, and the layout
//! deliberately does not, so every gap above the reader accumulates error
//! through the tween and the whole sum lands at once at the commit.
//!
//! The paginated modes have no strip to rescale; for them the frame just
//! moves the display scale and the single mounted host stretches to it.
//!
//! The interpolation is an out-cubic, which is what the smooth zoom has
//! always felt like: it covers ground early and decelerates into the target
//! instead of stopping dead on it.
//!
//! The loop reads the live `zoom.transition` signal each frame, so a
//! retarget mid-flight (a burst of `+` presses, a sidebar still sliding) is
//! adopted seamlessly: the tween continues from wherever the eye currently
//! is towards the new target, on a restarted clock.
//!
//! A container follow does not normally come through this loop: its target is
//! whatever the container allows RIGHT NOW, so the controller lands it in the
//! very frame the new size was reported and holds the commit for the end of the
//! burst. Easing towards a moving target would have the page visibly chasing
//! the window — covering a fraction of each new gap per frame and never quite
//! arriving. The loop can still be handed one (a follow taking over while a
//! tween is already running), so it knows how to land it without committing it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use leptos::prelude::*;

use crate::components::primitives::motion::reduced_motion::prefers_reduced_motion;
use crate::state::reader::{ReaderState, ZoomTransition};
use crate::viewer::engine::ViewerEngine;

use super::config;
use super::coordinator::finish_transition;

/// rAF step that can re-arm itself.
type StepSlot = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// Land a transaction: relay the layout out to its target, then show it.
///
/// Answers `false` when the scale has nowhere to go — the target is the one
/// already on screen — in which case NOTHING was written. That is not a micro-
/// optimisation: a Leptos `set` notifies even when the value is unchanged, so an
/// unconditional write on a settled target re-runs every mounted page's stretch
/// effect and rebuilds both strips for a factor of one. A container follow asks
/// for the landing on every frame of a burst, so it hits that case whenever the
/// scale is pinned (the minimum, or a hand-picked zoom capped at `desired`).
///
/// Two callers, deliberately: the controller calls it in the task that reported
/// a new container size, and the tween loop calls it for every untweened
/// landing. Both go through here so that "the layout moved and the display scale
/// agrees with it" stays one rule rather than two that can drift apart.
pub(crate) fn land(state: &ReaderState, engine: &ViewerEngine, t: &ZoomTransition) -> bool {
    let cur = state.viewer.zoom.display.get_untracked();
    if (t.to - cur).abs() < config::SETTLED_EPSILON {
        return false;
    }
    // Only the scrolling modes have a strip to rescale; for the paginated ones
    // the single mounted host stretches to the display scale on its own.
    if !state.viewer.mode.get_untracked().is_paginated() {
        engine.relayout_to(state, t.to / cur);
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

/// The single tween loop owned by the zoom controller.
///
/// The slot is stored here — not in a local of `arm` — so the running step
/// closure keeps itself alive across frames (each step hands the next frame
/// the `Rc` it clones out of the slot). `arm` is idempotent: while a loop
/// is alive it adopts whatever transition is on the signal, so retargets
/// never stack a second loop.
pub(crate) struct Tween {
    alive: Rc<Cell<bool>>,
    slot: StepSlot,
}

impl Tween {
    pub(crate) fn new() -> Self {
        let alive = Rc::new(Cell::new(false));
        // Teardown has to be able to stop this loop, because the loop is driven
        // from a rAF callback and an owner's cleanup cannot cancel one of those:
        // the callback is already queued when the page goes away. The flag is
        // what the queued frame checks before it reads anything.
        //
        // Parked through a stored id rather than captured directly for the same
        // reason `use_debounce` does it: a cleanup closure may not hold an `Rc`,
        // and the store is dropped before the closure could reach it, hence the
        // `try_` read. Called from the reader's owner — this is built in a
        // component body, next to `drive`.
        let store = StoredValue::new_local(Some(alive.clone()));
        on_cleanup(move || {
            if let Some(flag) = store.try_get_value().flatten() {
                flag.set(false);
            }
        });
        Self {
            alive,
            slot: Rc::new(RefCell::new(None)),
        }
    }

    /// Ensure a loop is running for the current transition.
    pub(crate) fn arm(&self, state: ReaderState, engine: ViewerEngine) {
        if self.alive.get() {
            return; // the live loop reads the signal; it will pick this up
        }
        self.alive.set(true);

        let alive = self.alive.clone();
        let weak = Rc::downgrade(&self.slot);
        let step: Rc<dyn Fn()> = Rc::new(move || {
            // Stopped, or disposed? Both answers mean the same thing here: do
            // not touch a signal. The check has to come first because reading a
            // graph whose owner has been cleaned up does not hand back `None`, it
            // unwinds through a callback nobody owns — and `!alive` is the one
            // piece of evidence that is still safe to read, since the flag lives
            // in the loop's own `Rc` and `new` clears it on cleanup.
            if !alive.get() {
                return;
            }
            // Idle? The loop dies here until the next `arm`.
            let Some(t) = state.viewer.zoom.transition.get_untracked() else {
                alive.set(false);
                return;
            };
            let mode = state.viewer.mode.get_untracked();
            // Only the scrolling modes have a strip to rescale.
            let scrolls = !mode.is_paginated();
            let duration = config::profile_for(mode).duration_ms();
            // Three reasons not to interpolate: the poster asked for the first
            // frame, this is a container follow (it must sit in the window, not
            // chase it), the profile has no duration, the OS asked for reduced
            // motion, or the reader switched zoom animation off. The last two
            // are read here rather than at every `post` so the settings cannot
            // be bypassed by a surface that forgot to ask.
            let reader_allows = state.viewer.motion.get_untracked().zoom;
            if !t.animate
                || t.following
                || duration <= 0.0
                || !reader_allows
                || prefers_reduced_motion()
            {
                // Landing without a tween: one relayout to the target, then
                // the commit.
                land(&state, &engine, &t);
                if t.following {
                    // A held follow LANDS but must not commit: the burst it
                    // is riding has another frame coming, and a raster pass per
                    // frame of a slide is the storm the held transaction exists
                    // to avoid. The controller's settle
                    // deadline commits it once the container stops moving. Going
                    // idle here instead of re-arming is what lets the next frame
                    // own the next rAF: `arm` adopts whatever transition is on the
                    // signal.
                    alive.set(false);
                    return;
                }
                finish_transition(&state, &t);
                alive.set(false);
                return;
            }
            let progress = ((js_sys::Date::now() - t.start_ms) / duration).clamp(0.0, 1.0);
            let visual = t.from + (t.to - t.from) * ease_out_cubic(progress);
            // The per-frame pair: relay the layout out by the ratio the
            // display scale is about to move through, then show it. The
            // engine reads `display` to work out the horizontal strip's
            // exact widths, so the relayout must come first.
            if scrolls {
                let cur = state.viewer.zoom.display.get_untracked();
                engine.relayout_to(&state, visual / cur);
            }
            state.viewer.zoom.display.set(visual);
            if progress >= 1.0 {
                finish_transition(&state, &t);
                alive.set(false);
                return;
            }
            if let Some(next) = weak.upgrade().and_then(|s| s.borrow().clone()) {
                request_animation_frame(move || next());
            }
        });
        *self.slot.borrow_mut() = Some(step.clone());
        request_animation_frame(move || step());
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
