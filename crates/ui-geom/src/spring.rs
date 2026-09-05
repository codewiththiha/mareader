//! The damped spring every animated box in the reader rides.
//!
//! Stiffness 210 / damping 26 is mildly underdamped (critical ≈ 29 at mass
//! 1): a confident pop with one small settle. Both the gloss card
//! (`ai_core::gloss::geometry::step_spring`) and the floating panels
//! ([`crate::floating::FloatBox`]) step this same integrator, so the feel of the
//! two cannot drift apart — which is the whole reason the physics sits in a
//! crate of its own rather than in whichever feature got it first.

/// Spring stiffness for animated boxes.
const SPRING_STIFFNESS: f64 = 210.0;

/// Spring damping for animated boxes.
const SPRING_DAMPING: f64 = 26.0;

/// One explicit-Euler step of a 1-D spring toward `t` from `c` at velocity
/// `v`. Returns `(position, velocity)`. dt is clamped by the caller so long
/// frames never blow the integrator past its stability bound.
pub fn spring_axis(c: f64, v: f64, t: f64, dt: f64) -> (f64, f64) {
    let force = SPRING_STIFFNESS * (t - c) - SPRING_DAMPING * v;
    let nv = v + force * dt;
    (c + nv * dt, nv)
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
}
