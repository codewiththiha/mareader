//! The spring, as a Leptos effect, generic over any 5-field [`SpringValue`]
//! (the floating box, first and foremost: [`crate::components::primitives::floating::types::FloatBox`]).
//! Springs `value` toward `target`; while `snap` is true (dragging / a forced
//! beat / reduced-motion) it jumps instead of wobbling.
//!
//! Mirrors the self-referencing rAF loop in `effects/reader/zoom.rs`: one
//! `StepSlot` (`Rc<RefCell<Option<Rc<dyn Fn()>>>>`) owns the step closure, the
//! closure holds a *weak* ref back to it to re-arm, and replacing the slot
//! (when `target` changes) drops the old loop's only strong ref so it dies.
//! `vel` and `last_ms` live outside the Effect so velocity survives a
//! retarget, keeping the morph continuous — unless a caller hard-resets via
//! [`SpringBox::reset_to`] when a *new* anchor opens.

use std::cell::RefCell;
use std::rc::Rc;

use leptos::prelude::*;
use pdf_core::gloss::GlossBox;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::components::primitives::floating::types::FloatBox;

fn cancel_raf(id: Option<i32>) {
    if let Some(id) = id
        && let Some(w) = web_sys::window()
    {
        let _ = w.cancel_animation_frame(id);
    }
}

fn schedule_raf(raf_id: StoredValue<Option<i32>, LocalStorage>, f: impl FnOnce() + 'static) {
    cancel_raf(raf_id.try_get_value().flatten());
    let Some(w) = web_sys::window() else {
        return;
    };
    let cb = Closure::once_into_js(f);
    if let Ok(id) = w.request_animation_frame(cb.as_ref().unchecked_ref()) {
        raf_id.set_value(Some(id));
    }
}

type StepSlot = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// The largest field magnitude that counts as "stopped" for loop teardown.
const SETTLE_EPS: f64 = 0.6;

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

/// The domain gloss box rides the same spring. `pdf_core::gloss` owns the
/// math (via FloatBox delegation); this adapter is the only seam between the
/// generic spring and the gloss domain type.
impl SpringValue for GlossBox {
    fn zero() -> Self {
        GlossBox::default()
    }
    fn close(&self, other: &Self, epsilon: f64) -> bool {
        pdf_core::gloss::boxes_close(*self, *other, epsilon)
    }
    fn step(&self, vel: &Self, target: &Self, dt: f64) -> (Self, Self) {
        pdf_core::gloss::step_spring(*self, *vel, *target, dt)
    }
    fn all_small(&self, epsilon: f64) -> bool {
        self.w.abs() < epsilon
            && self.x.abs() < epsilon
            && self.y.abs() < epsilon
            && self.h.abs() < epsilon
            && self.r.abs() < epsilon
    }
}

fn velocity_small<T: SpringValue>(v: T, eps: f64) -> bool {
    v.all_small(eps)
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
    // The owner-scoped holder for the current step closure, so it outlives the
    // rAF callbacks between Effect re-runs (same shape as zoom.rs's anim_slot).
    let anim_slot = StoredValue::new_local(None::<StepSlot>);
    let raf_id = StoredValue::new_local(None::<i32>);
    on_cleanup(move || cancel_raf(raf_id.try_get_value().flatten()));

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
            cancel_raf(raf_id.try_get_value().flatten());
            raf_id.set_value(None);
            return;
        }

        let slot: StepSlot = Rc::new(RefCell::new(None));
        let weak = Rc::downgrade(&slot);

        let step: Rc<dyn Fn()> = Rc::new(move || {
            let dest = match target.get_untracked() {
                Some(d) => d,
                // Target cleared mid-flight: stop.
                None => return,
            };

            let now = js_sys::Date::now();
            let mut last = last_ms.get_value();
            if last.is_nan() {
                last = now;
            }
            // Clamp long frames to the integrator's stability bound.
            let dt = ((now - last) / 1000.0).min(0.032);
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
                return;
            }

            let cur = value.get_untracked().unwrap_or(dest);
            let (next, next_vel) = cur.step(&vel.get_value(), &dest, dt);
            vel.set_value(next_vel);

            // Settled: park exactly on the target and stop scheduling.
            if next.close(&dest, SETTLE_EPS) && velocity_small(next_vel, SETTLE_EPS) {
                value.set(Some(dest));
                vel.set_value(T::zero());
                return;
            }

            value.set(Some(next));

            // Re-arm: upgrade our weak slot to the strong closure and queue it.
            if let Some(next) = weak.upgrade().and_then(|s| s.borrow().clone()) {
                schedule_raf(raf_id, move || next());
            }
        });

        *slot.borrow_mut() = Some(step.clone());
        anim_slot.set_value(Some(slot));
        schedule_raf(raf_id, move || step());
    });

    SpringBox { value, reset_to }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gloss(x: f64, y: f64, w: f64, h: f64, r: f64) -> GlossBox {
        GlossBox { x, y, w, h, r }
    }

    #[test]
    fn gloss_all_small_covers_every_field() {
        // Each field above epsilon on its own must break "all small": the
        // check is hand-rolled for GlossBox, and a dropped field would let
        // a still-moving spring tear its rAF loop down early.
        for above in [
            gloss(1.0, 0.0, 0.0, 0.0, 0.0),
            gloss(0.0, 1.0, 0.0, 0.0, 0.0),
            gloss(0.0, 0.0, 1.0, 0.0, 0.0),
            gloss(0.0, 0.0, 0.0, 1.0, 0.0),
            gloss(0.0, 0.0, 0.0, 0.0, 1.0),
        ] {
            assert!(!above.all_small(0.6), "{above:?} read as small");
        }
        assert!(gloss(0.0, 0.0, 0.0, 0.0, 0.0).all_small(0.6));
    }

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
