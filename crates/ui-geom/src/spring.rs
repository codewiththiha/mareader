//! The damped spring every animated box in the reader rides.

/// Spring stiffness for animated boxes; the pair every rider shares.
const SPRING_STIFFNESS: f64 = 210.0;

/// Spring damping for animated boxes; see [`SPRING_STIFFNESS`].
const SPRING_DAMPING: f64 = 26.0;

/// The longest frame the integrator is stepped on, in seconds.
pub const MAX_FRAME_S: f64 = 0.032;

/// One explicit-Euler step of a 1-D spring toward `t` from `c` at velocity `v`.
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
        assert!(
            (c - 300.0).abs() < 0.5,
            "did not settle on long frames: {c}"
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "spring_axis dt")]
    fn an_unclamped_frame_fails_loudly_in_debug() {
        // "Callers clamp dt" is a contract a host test can fail.
        let _ = spring_axis(0.0, 0.0, 100.0, 8.0);
    }
}
