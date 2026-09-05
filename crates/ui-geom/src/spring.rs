//! The damped spring every animated box in the reader rides.
//!
//! Stiffness 210 / damping 26 is mildly underdamped (critical ≈ 29 at mass
//! 1): a confident pop with one small settle. Both the gloss card
//! (`ai_core::gloss::geometry::step_spring`) and the floating panels
//! ([`crate::floating::FloatBox`]) step this same integrator, so the feel of the
//! two cannot drift apart — which is the whole reason the physics sits in a
//! crate of its own rather than in whichever feature got it first.

/// Spring stiffness for animated boxes. Public because the pair is the
/// contract itself: two surfaces (the floating panels and the gloss card)
/// ride this one integrator precisely so they cannot drift out of tune, and
/// a rider that wants to pin the feel in its own tests has to be able to
/// name the tuning rather than re-derive it.
pub const SPRING_STIFFNESS: f64 = 210.0;

/// Spring damping for animated boxes. See [`SPRING_STIFFNESS`] for why the
/// pair is public.
pub const SPRING_DAMPING: f64 = 26.0;

/// The longest frame the integrator is stepped on, in seconds.
///
/// Callers clamp their frame delta to this before stepping — the app's frame
/// loops read the clock through `src/components/primitives/motion/frame.rs`,
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
