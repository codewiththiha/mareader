//! The spring over any 5-field [`SpringValue`], on one [`FrameLoop`] whose
//! step a retarget replaces.

use leptos::prelude::*;

use app_chrome::floating::types::FloatBox;
use app_chrome::hooks::use_raf::FrameLoop;
use ui_geom::spring::MAX_FRAME_S;

use super::frame::frame_delta;

/// The largest field magnitude that counts as "stopped" for loop teardown.
const SETTLE_EPS: f64 = 0.6;

/// A value the spring can drive: five numeric fields and their tests.
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

/// The live sprung value, plus a way to hard-jump to a new anchor.
#[derive(Clone, Copy)]
pub struct SpringBox<T: SpringValue> {
    pub value: RwSignal<Option<T>>,
    /// Hard-jump to a box and zero the velocity.
    pub reset_to: Callback<T>,
}

/// Springs `value` toward `target`; `None` clears it and stops the loop.
pub fn use_spring_box<T: SpringValue>(
    target: Signal<Option<T>>,
    snap: Signal<bool>,
) -> SpringBox<T> {
    let value = RwSignal::new(target.get_untracked());
    let vel = StoredValue::new_local(T::zero());
    let last_ms = StoredValue::new_local(f64::NAN);
    // One loop per hook: a retarget replaces its step, and the owner stops it.
    let frames = FrameLoop::new();

    let reset_to = Callback::new(move |b: T| {
        value.set(Some(b));
        vel.set_value(T::zero());
        last_ms.set_value(f64::NAN);
    });

    Effect::new(move |_| {
        // A new target: read here to track it and gate the run.
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
            // Long frames clamp to the stability bound; the first passes none.
            let dt = frame_delta(last_ms.get_value(), now, MAX_FRAME_S);
            last_ms.set_value(now);

            if snap.get_untracked() {
                // No wobble while dragging or during a forced beat.
                vel.set_value(T::zero());
                let already = value.get_untracked().is_some_and(|v| v.close(&dest, 0.25));
                if !already {
                    value.set(Some(dest));
                }
                // Snapped to target; a later target re-runs the Effect.
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

// The gloss card rides the same spring; the impl lives here (orphan rule).
impl SpringValue for ai_core::gloss::GlossBox {
    fn zero() -> Self {
        ai_core::gloss::GlossBox::default()
    }
    fn close(&self, other: &Self, epsilon: f64) -> bool {
        ai_core::gloss::boxes_close(*self, *other, epsilon)
    }
    fn step(&self, vel: &Self, target: &Self, dt: f64) -> (Self, Self) {
        ai_core::gloss::step_spring(*self, *vel, *target, dt)
    }
    fn all_small(&self, epsilon: f64) -> bool {
        // A small velocity is a velocity close to zero: reuse `boxes_close`.
        ai_core::gloss::boxes_close(*self, ai_core::gloss::GlossBox::default(), epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_step_through_the_trait_settles_on_the_target() {
        // The adapter path end-to-end: stable steps must settle dead.
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
