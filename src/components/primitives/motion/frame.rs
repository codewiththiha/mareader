//! The frame delta every animation-frame loop needs.
//!
//! Four loops in this app run once per frame: the three that read the clock
//! through here — the continuous auto-scroll ticker, the held-arrow repeat
//! behind a navigation shortcut, and the spring that drives the floating
//! surfaces — plus the zoom tween, which interpolates against its own start
//! stamp rather than a delta. Each of the three needs the same reading of the
//! clock: how many seconds passed since the previous frame, with a ceiling on
//! that answer. They were each hand-rolled, with the "no previous frame yet"
//! case spelled differently in every one (a `NAN` sentinel compared at the use
//! site, a `mut` rebinding, a pair of stamps written at arm time), which is how
//! a loop ends up taking one bogus step the first time it runs.
//!
//! The LOOP around that reading is one answer as well:
//! `app_chrome::hooks::use_raf::FrameLoop` owns the re-arming, the stop flag and
//! the owner cleanup for the auto-scroll ticker, the spring and the zoom tween.
//! The held-arrow repeat is the exception and keeps its own cells — it is driven
//! by window-level key events, so there is no reactive owner to be cleaned up
//! by, and it reads no signals while it runs.
//!
//! The ceiling is the part that matters at runtime. A tab that was backgrounded
//! stops receiving frames but keeps its clock, so the first frame back can
//! report seconds of elapsed time; a loop that trusted it would scroll the
//! reader past a page of text in one jump, or hand the spring a step so large
//! it overshoots and wobbles. Clamping turns a stalled frame into a merely
//! fast one.

/// The longest frame a *scrolling* loop trusts, in seconds: 50ms, or 20fps.
///
/// Anything slower than that is not a frame rate, it is an interruption — the
/// tab was hidden, the main thread was blocked by a raster, the machine slept.
/// Scrolling the true gap in those cases moves the reader past content they
/// never saw; scrolling one clamped frame keeps the motion legible and costs
/// nothing but a slightly shorter jump.
pub const MAX_SCROLL_FRAME_S: f64 = 0.05;

/// Seconds between `prev_ms` and `now_ms`, clamped to `max_s`.
///
/// `prev_ms` being `NAN` — the sentinel a loop writes when it arms — means
/// there is no previous frame, and the answer is `0.0`: the first frame of a
/// loop moves nothing, and the second one starts the motion at a real rate.
/// A gap that comes out negative (a clock that went backwards) clamps to `0.0`
/// for the same reason: no consumer here can do anything sane with time
/// running in reverse, and a negative step would scroll the reader back or
/// integrate a spring backwards.
///
/// Both stamps are milliseconds, the unit `js_sys::Date::now()` returns; the
/// result is seconds, the unit every rate in the app is expressed in
/// (pixels per second, spring stiffness per second).
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

    /// The armed-loop case: no previous stamp, so no time has passed and the
    /// first frame moves nothing.
    #[test]
    fn the_first_frame_passes_no_time() {
        assert!(close(frame_delta(f64::NAN, 1_000.0, MAX_SCROLL_FRAME_S), 0.0));
    }

    /// An ordinary 60fps frame reports its own gap, in seconds.
    #[test]
    fn an_ordinary_frame_is_its_own_gap() {
        assert!(close(frame_delta(1_000.0, 1_016.0, MAX_SCROLL_FRAME_S), 0.016));
    }

    /// The regression the clamp exists for: a backgrounded tab's first frame
    /// back reports eight seconds and must not scroll eight seconds' worth.
    #[test]
    fn a_stalled_tab_does_not_jump_the_reader() {
        assert!(close(frame_delta(1_000.0, 9_000.0, MAX_SCROLL_FRAME_S), MAX_SCROLL_FRAME_S));
    }

    /// A clock that went backwards is treated as no time at all rather than as
    /// a negative step, which would scroll the reader up or integrate a spring
    /// in reverse.
    #[test]
    fn a_backwards_clock_passes_no_time() {
        assert!(close(frame_delta(2_000.0, 1_000.0, MAX_SCROLL_FRAME_S), 0.0));
    }

    /// The bound belongs to the consumer: the spring clamps tighter than a
    /// scroll because its integrator, not the reader's eye, sets the limit.
    #[test]
    fn the_bound_is_the_caller_s() {
        assert!(close(frame_delta(0.0, 100.0, 0.032), 0.032));
        assert!(close(frame_delta(0.0, 100.0, MAX_SCROLL_FRAME_S), 0.05));
    }
}
