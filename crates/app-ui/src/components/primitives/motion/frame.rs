//! The frame delta every animation loop needs, with a ceiling.

/// The longest frame a scrolling loop trusts: 50ms.
pub const MAX_SCROLL_FRAME_S: f64 = 0.05;

/// `NAN` means no previous frame, so the answer is `0.0`; so is a
/// backwards clock.
pub fn frame_delta(prev_ms: f64, now_ms: f64, max_s: f64) -> f64 {
    if prev_ms.is_nan() {
        return 0.0;
    }
    ((now_ms - prev_ms) / 1000.0).clamp(0.0, max_s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// The armed-loop case: no previous stamp, so nothing moves.
    #[test]
    fn the_first_frame_passes_no_time() {
        assert!(close(
            frame_delta(f64::NAN, 1_000.0, MAX_SCROLL_FRAME_S),
            0.0
        ));
    }

    #[test]
    fn an_ordinary_frame_is_its_own_gap() {
        assert!(close(
            frame_delta(1_000.0, 1_016.0, MAX_SCROLL_FRAME_S),
            0.016
        ));
    }

    /// The regression the clamp exists for: a backgrounded tab's first frame.
    #[test]
    fn a_stalled_tab_does_not_jump_the_reader() {
        assert!(close(
            frame_delta(1_000.0, 9_000.0, MAX_SCROLL_FRAME_S),
            MAX_SCROLL_FRAME_S
        ));
    }

    /// A backwards clock is no time at all, not a negative step.
    #[test]
    fn a_backwards_clock_passes_no_time() {
        assert!(close(
            frame_delta(2_000.0, 1_000.0, MAX_SCROLL_FRAME_S),
            0.0
        ));
    }

    /// The bound belongs to the consumer: the spring clamps tighter.
    #[test]
    fn the_bound_is_the_caller_s() {
        assert!(close(frame_delta(0.0, 100.0, 0.032), 0.032));
        assert!(close(frame_delta(0.0, 100.0, MAX_SCROLL_FRAME_S), 0.05));
    }
}
