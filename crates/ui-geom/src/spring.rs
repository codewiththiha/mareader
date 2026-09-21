//! The damped spring every animated box in the reader rides, and the shape a
//! value has to have to ride it.
//!
//! Stiffness 210 / damping 26 is mildly underdamped (critical ≈ 29 at mass
//! 1): a confident pop with one small settle. Both the gloss card
//! (`ai_core::gloss::geometry::step_spring`) and the floating panels
//! ([`crate::floating::FloatBox`]) step this same integrator, so the feel of the
//! two cannot drift apart — which is the whole reason the physics sits in a
//! crate of its own rather than in whichever feature got it first.
//!
//! [`SpringValue`] is the other half of that reason. The loop that drives a
//! box is a UI concern and lives in the kit (`ui_kit::motion::spring`), but a
//! trait defined there could only ever be implemented for types the kit can
//! see: Rust's orphan rule forbids `impl ForeignTrait for ForeignType`, so the
//! gloss box's adapter could not sit beside the gloss box. Defining the trait
//! here, in the leaf both riders already depend on, is what lets each domain
//! type bring its own adapter — [`crate::floating::FloatBox`]'s is at the
//! bottom of this file, `ai_core::gloss::spring` holds the gloss box's.

/// Spring stiffness for animated boxes. The pair is the tuning every rider
/// shares — the floating panels and the gloss card both step
/// [`spring_axis`], which is what keeps them from drifting apart; the
/// integrator is the contract, and nothing outside this module names the
/// numbers it runs on.
const SPRING_STIFFNESS: f64 = 210.0;

/// Spring damping for animated boxes; see [`SPRING_STIFFNESS`].
const SPRING_DAMPING: f64 = 26.0;

/// The longest frame the integrator is stepped on, in seconds.
///
/// Callers clamp their frame delta to this before stepping — the app's frame
/// loops read the clock through `crates/ui-kit/src/motion/frame.rs`,
/// whose ceiling for spring riders matches this number. It is a convergence
/// ceiling, not a mathematical stability limit: the convergence tests pin it,
/// and a step past it overshoots and wobbles rather than explodes. The
/// debug assert below turns "callers clamp" from prose into a contract a
/// host test can fail.
pub const MAX_FRAME_S: f64 = 0.032;

/// One explicit-Euler step of a 1-D spring toward `t` from `c` at velocity
/// `v`. Returns `(position, velocity)`. dt is clamped by the caller so long
/// frames never blow the integrator past its stability bound.
pub fn spring_axis(c: f64, v: f64, t: f64, dt: f64) -> (f64, f64) {
    debug_assert!(
        dt.is_finite() && (0.0..=MAX_FRAME_S).contains(&dt),
        "spring_axis dt must be a frame's worth of seconds (0..={MAX_FRAME_S}), got {dt}"
    );
    let force = SPRING_STIFFNESS * (t - c) - SPRING_DAMPING * v;
    let nv = v + force * dt;
    (c + nv * dt, nv)
}

/// A value the spring can drive: five numeric fields with a step, a closeness
/// test and a magnitude test.
///
/// `Send + Sync` mirrors what reactive signals stored in `Signal<T>` require
/// (default storage); plain data types like the boxes qualify trivially.
///
/// Defined here rather than beside the loop that drives it so that a domain
/// type in any crate that already reaches this leaf can implement it for
/// itself — the orphan rule leaves no other crate able to.
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

impl SpringValue for crate::floating::FloatBox {
    fn zero() -> Self {
        Self::default()
    }
    fn close(&self, other: &Self, epsilon: f64) -> bool {
        // The inherent methods of the same name, which is the point: the
        // adapter is four forwards, and the field list is enumerated once.
        crate::floating::FloatBox::close(self, other, epsilon)
    }
    fn step(&self, vel: &Self, target: &Self, dt: f64) -> (Self, Self) {
        crate::floating::FloatBox::step(self, vel, target, dt)
    }
    fn all_small(&self, epsilon: f64) -> bool {
        crate::floating::FloatBox::all_small(self, epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::spring_axis;

    #[test]
    fn the_axis_converges_to_its_target() {
        let mut c = 40.0;
        let mut v = 0.0;
        for _ in 0..200 {
            (c, v) = spring_axis(c, v, 200.0, 1.0 / 60.0);
        }
        assert!((c - 200.0).abs() < 0.5, "did not settle: {c}");
    }

    #[test]
    fn the_axis_stays_bounded_on_a_dropped_frame() {
        let mut c = 0.0;
        let mut v = 0.0;
        for _ in 0..400 {
            (c, v) = spring_axis(c, v, 300.0, 0.032);
            assert!(c.is_finite() && v.is_finite(), "blew up: {c} @ {v}");
        }
        assert!((c - 300.0).abs() < 0.5, "did not settle on long frames: {c}");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "spring_axis dt")]
    fn an_unclamped_frame_fails_loudly_in_debug() {
        // "Callers clamp dt" is a contract a host test can now fail: a caller
        // that hands the integrator a backgrounded tab's first frame back —
        // seconds of clock, no clamp — panics here in debug instead of
        // silently wobbling in release.
        let _ = spring_axis(0.0, 0.0, 100.0, 8.0);
    }
}
