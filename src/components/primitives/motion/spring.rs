//! The spring, as a Leptos effect, generic over any 5-field [`SpringValue`]
//! (the floating box, first and foremost: [`app_chrome::floating::types::FloatBox`]).
//!
//! Springs `value` toward `target`; while `snap` is true (dragging / a forced
//! beat / reduced-motion) it jumps instead of wobbling.
//!
//! The frame machinery is [`FrameLoop`], the primitive the zoom tween rides
//! too: one loop, whose step is REPLACED when `target` changes rather than
//! stacked, so a retarget mid-flight carries the spring's velocity over
//! instead of restarting it. `vel` and `last_ms` live outside the Effect for
//! the same reason — velocity survives a retarget, keeping the morph
//! continuous — unless a caller hard-resets via [`SpringBox::reset_to`] when a
//! *new* anchor opens.
//!
//! This module implements [`SpringValue`] for the floating box and for nothing
//! else. A domain type that wants to ride the spring brings its own adapter —
//! the gloss box's is `crate::components::ai::gloss::spring` — because a
//! primitive that imported a feature crate's type would be breakable by that
//! crate, and would make every other consumer of the primitive depend on the
//! feature too.

use leptos::prelude::*;

use app_chrome::floating::types::FloatBox;
use app_chrome::hooks::use_raf::FrameLoop;

use super::frame::frame_delta;

/// The largest field magnitude that counts as "stopped" for loop teardown.
const SETTLE_EPS: f64 = 0.6;

/// The longest frame the integrator trusts, in seconds. Tighter than a
/// scrolling loop's bound (see [`super::frame`]) because this is a stability
/// limit, not a legibility one: a step much larger than a frame makes the
/// spring overshoot its target and wobble on the way back.
const MAX_INTEGRATOR_FRAME_S: f64 = 0.032;

/// A value the spring can drive: five numeric fields with a step, a closeness
/// test and a magnitude test.
///
/// `Send + Sync` mirrors what reactive signals stored in `Signal<T>` require
/// (default storage); plain data types like the boxes qualify trivially.
pub trait SpringValue: Copy + Send + Sync + 'static {
    /// The all-zero value (rest).
    fn zero() -> Self;
    /// Field-wise closeness to `other` within `epsilon`.
    fn close(&self, other: &Self, epsilon: f64) -> bool;
    /// One spring step toward `target` from `self` at velocity `vel`.
    fn step(&self, vel: &Self, target: &Self, dt: f64) -> (Self, Self);
    /// Whether every field is below `epsilon` in magnitude.
    fn all_small(&self, epsilon: f64) -> bool;
}

impl SpringValue for FloatBox {
    fn zero() -> Self {
        FloatBox::default()
    }
    fn close(&self, other: &Self, epsilon: f64) -> bool {
        self.close(other, epsilon)
    }
    fn step(&self, vel: &Self, target: &Self, dt: f64) -> (Self, Self) {
        self.step(vel, target, dt)
    }
    fn all_small(&self, epsilon: f64) -> bool {
        self.all_small(epsilon)
    }
}

/// Handle returned by [`use_spring_box`]: the live sprung value plus a way to
/// hard-jump onto a new anchor so the next morph starts from there.
#[derive(Clone, Copy)]
pub struct SpringBox<T: SpringValue> {
    pub value: RwSignal<Option<T>>,
    /// Hard-jump to a box and zero the velocity. Called when a NEW anchor is
    /// opened so the next morph starts from exactly that anchor instead of
    /// wherever the previous surface settled.
    pub reset_to: Callback<T>,
}

/// Springs `value` toward `target`. Setting `target` to `None` clears the
/// value and stops the loop.
///
/// This is the reusable MORPH primitive — any floating surface that grows
/// out of one rectangle and lands on another can drive it with two inputs:
///
/// * `target` — the END point (a `Signal<Option<T>>`; retargeting
///   mid-flight is fine, the spring carries its velocity over).
/// * `snap` — while true (dragging, reduced motion, a forced beat) the
///   value jumps to the target instead of wobbling.
///
/// and one handle output: `SpringBox::reset_to` is the START point — call it
/// when a new origin appears (a fresh anchor rect) so the morph begins from
/// there. The value signal renders straight into geometry (left/top/width/
/// height/radius); write it to style per frame and put NO transition on
/// those properties — the spring is the animation.
///
/// Bring your own five-field type by implementing [`SpringValue`]
/// (or use the generic `FloatBox`); the gloss word card is the reference
/// consumer (`ai/gloss/targeting.rs`), and its rustdoc history is the
/// recipe.
pub fn use_spring_box<T: SpringValue>(target: Signal<Option<T>>, snap: Signal<bool>) -> SpringBox<T> {
    let value = RwSignal::new(target.get_untracked());
    let vel = StoredValue::new_local(T::zero());
    let last_ms = StoredValue::new_local(f64::NAN);
    // One loop for the life of this hook: a retarget replaces its step rather
    // than starting a second loop, and it stops with the owner that built it.
    let frames = FrameLoop::new();

    let reset_to = Callback::new(move |b: T| {
        value.set(Some(b));
        vel.set_value(T::zero());
        last_ms.set_value(f64::NAN);
    });

    Effect::new(move |_| {
        // A new target. Reading it here both tracks it (so this re-runs) and
        // gates the run: no target means clear and stop.
        if target.get().is_none() {
            value.set(None);
            vel.set_value(T::zero());
            last_ms.set_value(f64::NAN);
            frames.stop();
            return;
        }

        frames.arm(move || {
            let dest = match target.get_untracked() {
                Some(d) => d,
                // Target cleared mid-flight: stop.
                None => return false,
            };

            let now = js_sys::Date::now();
            // Long frames clamp to the integrator's stability bound, and the
            // first frame after arming (the `NAN` above) passes no time.
            let dt = frame_delta(last_ms.get_value(), now, MAX_INTEGRATOR_FRAME_S);
            last_ms.set_value(now);

            if snap.get_untracked() {
                // No wobble while dragging or during a forced beat.
                vel.set_value(T::zero());
                let already = value.get_untracked().is_some_and(|v| v.close(&dest, 0.25));
                if !already {
                    value.set(Some(dest));
                }
                // Snapped to target; target changes re-run the Effect to move
                // again, so no further frames are needed here.
                return false;
            }

            let cur = value.get_untracked().unwrap_or(dest);
            let (next, next_vel) = cur.step(&vel.get_value(), &dest, dt);
            vel.set_value(next_vel);

            // Settled: park exactly on the target and stop scheduling.
            if next.close(&dest, SETTLE_EPS) && next_vel.all_small(SETTLE_EPS) {
                value.set(Some(dest));
                vel.set_value(T::zero());
                return false;
            }

            value.set(Some(next));
            true
        });
    });

    SpringBox { value, reset_to }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_step_through_the_trait_settles_on_the_target() {
        // The adapter path end-to-end: a few hundred stable 60fps steps must
        // land within the loop's settle epsilon with a dead velocity — the
        // same condition use_spring_box uses to stop scheduling frames.
        let mut cur = FloatBox::default();
        let mut vel = FloatBox::default();
        let target = FloatBox {
            x: 40.0,
            y: 400.0,
            w: 360.0,
            h: 240.0,
            r: 18.0,
        };
        for _ in 0..600 {
            let (next, next_vel) = cur.step(&vel, &target, 1.0 / 60.0);
            cur = next;
            vel = next_vel;
        }
        assert!(cur.close(&target, SETTLE_EPS), "did not settle: {cur:?}");
        assert!(vel.all_small(SETTLE_EPS), "velocity survived: {vel:?}");
    }
}
