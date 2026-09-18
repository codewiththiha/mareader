//! The motion model: how fast the reader is travelling, which way, and what
//! that means for the pages around them.
//!
//! Everything here is pure arithmetic over numbers the adapter already has —
//! a scroll offset and a clock — which is why the whole predictor is
//! unit-tested on the host with no DOM and no timers. The adapter in
//! [`crate::virtualizer`] owns the clock; this module owns the interpretation.
//!
//! The model answers three questions, in the order they are asked:
//!
//! 1. **How fast?** [`ScrollVelocity`] smooths the raw per-event deltas of a
//!    wheel, a trackpad or a scrollbar drag into one exponentially weighted
//!    pixels-per-second number. Raw deltas are useless for scheduling: a
//!    trackpad emits a burst of small ones and a wheel a handful of large
//!    ones, and either read literally would flip the policy every event.
//! 2. **What kind of movement is that?** [`ScrollPhase`] classifies the
//!    smoothed speed against the VIEWPORT rather than against absolute
//!    pixels, because 1500px/s is a stroll on a 2400px window and a fling on
//!    a 600px one. Classification carries hysteresis, so a speed hovering on
//!    a boundary does not oscillate the policy between two phases.
//! 3. **Where will the reader be?** [`Predictor`] projects the current offset
//!    forward over a short horizon and turns the result into the page index
//!    the reader is about to reach — the answer that lets a scheduler render
//!    the destination instead of the pages being flown past.

/// One scroll offset sample, and what it did to the velocity estimate.
///
/// The derived `Default` is an UNPRIMED model: no position, no clock, and so no
/// speed until the first sample arrives. [`ScrollVelocity::at`] is the
/// constructor a caller that already knows where the reader is uses.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScrollVelocity {
    /// The last offset adopted, in content coordinates.
    offset: f64,
    /// The clock reading that offset arrived at, in milliseconds.
    time_ms: f64,
    /// Smoothed signed speed, pixels per second. Positive travels towards
    /// higher offsets (down a vertical list, right along a horizontal one).
    velocity: f64,
    /// Whether a position AND a clock reading have been recorded. An unprimed
    /// model adopts the first sample it is handed and reports no motion: the
    /// alternative is measuring the distance from a default-constructed zero to
    /// wherever the reader actually is, over one frame, and calling the result
    /// a fling of half a million pixels per second.
    primed: bool,
}

/// Smoothing weight for one new sample. Low enough that a wheel's uneven
/// deltas do not show through, high enough that a real fling is recognised
/// within two or three events rather than after the reader has arrived.
const VELOCITY_ALPHA: f64 = 0.22;

/// Signed speed below which the estimate is treated as still. A resting
/// scroller still reports sub-pixel events on some platforms; without a dead
/// band the direction flag would flicker and every directional policy with it.
const STILL_PX_PER_SEC: f64 = 20.0;

/// Samples further apart than this are not motion: the reader stopped, and the
/// next offset is a fresh start rather than a continuation. Treating a
/// multi-second gap as one movement reports a speed of a few pixels per
/// second for what is actually a stationary reader with a new position —
/// which is the right answer, but only if the estimate is reset rather than
/// averaged into a stale one.
const SAMPLE_GAP_MS: f64 = 220.0;

impl ScrollVelocity {
    /// A model sitting at `offset`, having seen no motion. The adapter seeds
    /// this at bind time so the first real sample measures a movement rather
    /// than a jump from zero.
    pub fn at(offset: f64, now_ms: f64) -> Self {
        Self {
            offset,
            time_ms: now_ms,
            velocity: 0.0,
            primed: true,
        }
    }

    /// Fold one offset sample in and return the smoothed signed speed.
    ///
    /// `now_ms` comes from the caller's monotonic clock. A model built with
    /// [`Self::default`] has no position to measure from, so its first sample
    /// records one and reports no motion; one built with [`Self::at`] is primed
    /// and measures from the sample after that.
    pub fn update(&mut self, offset: f64, now_ms: f64) -> f64 {
        if !self.primed {
            self.primed = true;
            self.offset = offset;
            self.time_ms = now_ms;
            return 0.0;
        }
        let delta_t = now_ms - self.time_ms;
        // A clock that has not moved (two events in one frame) or has gone
        // backwards cannot produce a speed; keep the estimate and the
        // position honest by adopting the offset and reporting what we had.
        if delta_t <= 0.0 {
            self.offset = offset;
            return self.velocity;
        }
        // A long quiet stretch: this is a new start, not a continuation.
        if delta_t > SAMPLE_GAP_MS {
            self.offset = offset;
            self.time_ms = now_ms;
            self.velocity = 0.0;
            return 0.0;
        }

        let raw = (offset - self.offset) / delta_t * 1000.0;
        self.velocity = self.velocity * (1.0 - VELOCITY_ALPHA) + raw * VELOCITY_ALPHA;
        self.offset = offset;
        self.time_ms = now_ms;
        self.velocity
    }

    /// Smoothed signed speed, pixels per second.
    pub fn velocity(&self) -> f64 {
        self.velocity
    }

    /// Magnitude of the smoothed speed — what a phase classification reads.
    pub fn speed(&self) -> f64 {
        self.velocity.abs()
    }

    /// Which way the reader is travelling: `1` towards higher offsets, `-1`
    /// towards lower ones, `0` when the movement is inside the dead band.
    pub fn direction(&self) -> i8 {
        if self.velocity > STILL_PX_PER_SEC {
            1
        } else if self.velocity < -STILL_PX_PER_SEC {
            -1
        } else {
            0
        }
    }

    /// Forget the estimate without moving the recorded position: a jump that
    /// is not motion (a page-turn command, a document switch, a zoom's
    /// geometry commit writing the surface) must not be read as a fling of
    /// several thousand pixels per second.
    pub fn reset(&mut self, offset: f64, now_ms: f64) {
        self.offset = offset;
        self.time_ms = now_ms;
        self.velocity = 0.0;
        self.primed = true;
    }
}

/// What kind of movement the scroller is making.
///
/// The phases are ordered, and every policy that reads one asks "at least
/// this fast?" — which is what [`ScrollPhase::at_least`] is for. They are
/// classified against the VIEWPORT rather than against absolute pixels,
/// because 1500px/s is a stroll on a 2400px window and a fling on a 600px
/// one, and because the numbers a policy actually wants ("about to cross
/// three screens") mean the same thing at any window size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum ScrollPhase {
    /// No movement worth scheduling against.
    #[default]
    Idle,
    /// Reading speed: the reader is looking at what arrives.
    Slow,
    /// Ordinary scrolling.
    Normal,
    /// Covering ground: pages pass faster than they can be read.
    Fast,
    /// A throw. The destination matters and the pages between do not.
    Fling,
}

/// Below this, in screens per second, the scroller is not moving.
const IDLE_SCREEN_PER_SEC: f64 = 0.12;
/// At this the movement is scrolling rather than reading in place.
const SLOW_SCREEN_PER_SEC: f64 = 0.9;
/// At this the pages arriving cannot be read on the way past.
const FAST_SCREEN_PER_SEC: f64 = 2.2;
/// At this the reader is throwing the document rather than scrolling it.
const FLING_SCREEN_PER_SEC: f64 = 4.5;

/// Entry thresholds sit this far ABOVE a boundary…
const ENTRY_MARGIN: f64 = 1.15;
/// …and exit thresholds this far BELOW it, so a speed parked on the line
/// keeps the phase it already has.
const EXIT_MARGIN: f64 = 0.85;

/// The speed, in screens per second, at which `phase` begins.
const fn boundary(phase: ScrollPhase) -> f64 {
    match phase {
        ScrollPhase::Idle => 0.0,
        ScrollPhase::Slow => IDLE_SCREEN_PER_SEC,
        ScrollPhase::Normal => SLOW_SCREEN_PER_SEC,
        ScrollPhase::Fast => FAST_SCREEN_PER_SEC,
        ScrollPhase::Fling => FLING_SCREEN_PER_SEC,
    }
}

/// Classify a speed against boundaries scaled by `margin`.
///
/// One function serves all three classifications: `1.0` is the plain answer,
/// [`ENTRY_MARGIN`] the one that must be beaten to move UP a phase, and
/// [`EXIT_MARGIN`] the one that must be fallen under to move DOWN. The gap
/// between the last two IS the hysteresis — a speed between them keeps the
/// phase it arrived with, so a reader hovering on a boundary does not resize
/// the mount window and re-prioritise every queued render twice per frame.
fn phase_at(screens_per_sec: f64, margin: f64) -> ScrollPhase {
    if screens_per_sec < boundary(ScrollPhase::Slow) * margin {
        ScrollPhase::Idle
    } else if screens_per_sec < boundary(ScrollPhase::Normal) * margin {
        ScrollPhase::Slow
    } else if screens_per_sec < boundary(ScrollPhase::Fast) * margin {
        ScrollPhase::Normal
    } else if screens_per_sec < boundary(ScrollPhase::Fling) * margin {
        ScrollPhase::Fast
    } else {
        ScrollPhase::Fling
    }
}

impl ScrollPhase {
    /// Whether this phase is at least `other` — the ordering question every
    /// policy asks (`if phase.at_least(Fast) { skip the text layer }`).
    pub const fn at_least(self, other: Self) -> bool {
        (self as u8) >= (other as u8)
    }

    /// Classify a speed against the viewport, screens per second.
    pub fn classify(speed_px_per_sec: f64, viewport: f64) -> Self {
        match Self::screens_per_sec(speed_px_per_sec, viewport) {
            Some(screens) => phase_at(screens, 1.0),
            None => Self::Idle,
        }
    }

    /// [`Self::classify`] with hysteresis: the phase the reader is already in
    /// is left only when the speed crosses a threshold set away from the one
    /// that entered it.
    pub fn classify_from(current: Self, speed_px_per_sec: f64, viewport: f64) -> Self {
        let raw = Self::classify(speed_px_per_sec, viewport);
        if raw == current {
            return current;
        }
        let Some(screens) = Self::screens_per_sec(speed_px_per_sec, viewport) else {
            return Self::Idle;
        };
        if raw > current {
            // Rising has to beat the target's entry threshold, and never
            // lands below the phase the reader is already in: a jump from
            // Idle straight into a fling is a fling, not a staircase.
            phase_at(screens, ENTRY_MARGIN).max(current)
        } else {
            // Falling has to drop under the current phase's exit threshold.
            phase_at(screens, EXIT_MARGIN).min(current)
        }
    }

    /// Whether a page arriving now would be read rather than flown past — the
    /// question a render path asks before starting expensive work.
    pub const fn is_moving(self) -> bool {
        !matches!(self, Self::Idle)
    }

    /// Whether the movement is fast enough that full-resolution work for
    /// every page it crosses is waste: no text layers, and no raster for
    /// pages that will be gone before the render lands.
    pub const fn is_sweeping(self) -> bool {
        matches!(self, Self::Fast | Self::Fling)
    }

    /// The speed in screens per second, or `None` when there is no viewport
    /// to measure it against.
    fn screens_per_sec(speed_px_per_sec: f64, viewport: f64) -> Option<f64> {
        if !(viewport > 0.0) || !speed_px_per_sec.is_finite() {
            return None;
        }
        Some(speed_px_per_sec.abs() / viewport)
    }
}

/// Where the reader is heading, and how far a scheduler should look.
///
/// Pure: it takes an offset, a speed and a horizon, and answers with another
/// offset. Turning that into pages belongs to the engine, which owns the
/// layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Predictor {
    /// How far ahead to project, milliseconds.
    horizon_ms: f64,
    /// The longest projection allowed, in viewport screens. The real ceiling:
    /// a prediction past it names a page the reader is not about to arrive
    /// at, and rendering for it is exactly the waste prediction exists to
    /// prevent.
    max_screens: f64,
}

impl Predictor {
    /// A predictor projecting `horizon_ms` ahead, capped at `max_screens`
    /// viewport screens.
    pub const fn new(horizon_ms: f64, max_screens: f64) -> Self {
        Self {
            horizon_ms,
            max_screens,
        }
    }

    /// The offset the reader is expected to reach, clamped into the
    /// scrollable range.
    ///
    /// Directional by construction: a negative speed predicts upwards. Capped
    /// because the useful answer is "which page should be crisp when the
    /// reader arrives", and a reader who keeps flinging for ten seconds does
    /// not arrive at all — the pages that matter are the ones just past where
    /// they are now.
    pub fn offset(&self, scroll_top: f64, velocity: f64, viewport: f64, max_scroll: f64) -> f64 {
        if !velocity.is_finite() || viewport <= 0.0 {
            return scroll_top;
        }
        let cap = self.max_screens.max(0.0) * viewport;
        let projected = velocity * self.horizon_ms.max(0.0) / 1000.0;
        (scroll_top + projected.clamp(-cap, cap)).clamp(0.0, max_scroll.max(0.0))
    }

    /// How many screens ahead of the viewport a scheduler should treat as
    /// "about to be looked at". Grows with speed and stops at the cap: a
    /// stationary reader needs no look-ahead beyond what the budget mounts.
    pub fn screens(&self, speed_px_per_sec: f64, viewport: f64) -> f64 {
        if !(viewport > 0.0) || !speed_px_per_sec.is_finite() {
            return 0.0;
        }
        let screens = speed_px_per_sec.abs() * self.horizon_ms.max(0.0) / 1000.0 / viewport;
        screens.min(self.max_screens.max(0.0))
    }
}

impl Default for Predictor {
    fn default() -> Self {
        // Roughly an eighth of a second of travel, never more than four
        // screens: enough to have a fling's destination warm, short enough
        // that a change of mind wastes nothing already rendered.
        Self::new(120.0, 4.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed a constant speed for `steps` samples, `dt_ms` apart, and return
    /// the settled estimate. Ten steps of an EMA at alpha 0.22 leaves under
    /// a tenth of the initial error, so this is "the speed the reader
    /// sustains".
    fn settle(speed: f64, dt_ms: f64, steps: usize) -> ScrollVelocity {
        let mut v = ScrollVelocity::at(0.0, 0.0);
        let mut offset = 0.0;
        let mut now = 0.0;
        for _ in 0..steps {
            now += dt_ms;
            offset += speed * dt_ms / 1000.0;
            v.update(offset, now);
        }
        v
    }

    #[test]
    fn the_first_sample_records_a_position_and_reports_no_motion() {
        let mut v = ScrollVelocity::default();
        assert_eq!(v.update(4_000.0, 100.0), 0.0);
        assert_eq!(v.direction(), 0);
        assert_eq!(v.velocity(), 0.0);
    }

    #[test]
    fn a_sustained_speed_settles_on_that_speed() {
        let v = settle(2_000.0, 16.0, 12);
        assert!((v.speed() - 2_000.0).abs() < 200.0, "got {}", v.speed());
        assert_eq!(v.direction(), 1);
    }

    #[test]
    fn scrolling_up_is_negative_and_reports_the_up_direction() {
        let v = settle(-1_500.0, 16.0, 12);
        assert!(v.velocity() < 0.0);
        assert_eq!(v.direction(), -1);
        assert!((v.speed() - 1_500.0).abs() < 200.0);
    }

    #[test]
    fn a_jagged_wheel_burst_smooths_instead_of_flipping() {
        // What a wheel actually emits: alternating large and small deltas for
        // one continuous movement. The estimate must not change sign with
        // them, or every directional policy would reverse mid-scroll.
        let mut v = ScrollVelocity::at(0.0, 0.0);
        let mut offset = 0.0;
        let mut now = 0.0;
        let mut signs = 0;
        let mut last_sign = 0;
        for step in 0..24 {
            now += 16.0;
            let delta = if step % 2 == 0 { 120.0 } else { 20.0 };
            offset += delta;
            v.update(offset, now);
            let sign = v.velocity().signum();
            if sign != last_sign {
                signs += 1;
                last_sign = sign;
            }
        }
        assert_eq!(signs, 1, "the estimate changed direction while scrolling one way");
        assert_eq!(v.direction(), 1);
    }

    #[test]
    fn sub_pixel_jitter_never_becomes_a_direction() {
        let mut v = ScrollVelocity::at(1_000.0, 0.0);
        for step in 0..20 {
            v.update(1_000.0 + if step % 2 == 0 { 0.4 } else { -0.4 }, 16.0 * (step + 1) as f64);
        }
        assert_eq!(v.direction(), 0);
        assert!(v.speed() <= STILL_PX_PER_SEC);
    }

    #[test]
    fn a_quiet_stretch_ends_the_movement_rather_than_extending_it() {
        // Ten 16ms samples put the clock at 160ms and the offset at 48px.
        let mut v = settle(300.0, 16.0, 10);
        assert!(v.speed() > 100.0);
        // Half a second of nothing, then the reader nudges the scroller: the
        // estimate must not report the whole gap as one slow movement, or a
        // reader who returns after a pause looks like they are still
        // travelling and the window stays skewed to a direction they left.
        assert_eq!(v.update(48.0 + 100.0, 160.0 + 500.0), 0.0);
        assert_eq!(v.direction(), 0);
        // The nudge itself measures from the new position, not the old one.
        assert!(v.update(148.0 + 40.0, 660.0 + 16.0) > 0.0);
    }

    #[test]
    fn two_samples_in_one_frame_do_not_invent_a_speed() {
        let mut v = ScrollVelocity::at(0.0, 0.0);
        v.update(600.0, 16.0);
        let before = v.velocity();
        // Same clock reading: no duration, so no new information.
        assert_eq!(v.update(900.0, 16.0), before);
    }

    #[test]
    fn reset_drops_a_jump_that_was_not_motion() {
        let mut v = settle(1_200.0, 16.0, 10);
        assert!(v.speed() > 500.0);
        // A page-turn command writes the surface: the offset moved, the
        // reader did not.
        v.reset(9_000.0, 200.0);
        assert_eq!(v.velocity(), 0.0);
        assert_eq!(v.direction(), 0);
        // …and the next real movement measures from the new position.
        v.update(9_150.0, 216.0);
        assert!(v.velocity() > 0.0);
    }

    #[test]
    fn phase_thresholds_read_the_viewport_not_absolute_pixels() {
        // 1500px/s: a stroll on a tall window, a fling on a short one.
        assert_eq!(ScrollPhase::classify(1_500.0, 2_400.0), ScrollPhase::Slow);
        assert_eq!(ScrollPhase::classify(1_500.0, 400.0), ScrollPhase::Fast);
        // An unmeasured viewport classifies nothing.
        assert_eq!(ScrollPhase::classify(9_000.0, 0.0), ScrollPhase::Idle);
    }

    #[test]
    fn phase_ordering_answers_the_at_least_question() {
        assert!(ScrollPhase::Fling.at_least(ScrollPhase::Fast));
        assert!(ScrollPhase::Fast.at_least(ScrollPhase::Fast));
        assert!(!ScrollPhase::Normal.at_least(ScrollPhase::Fast));
        assert!(ScrollPhase::Idle.at_least(ScrollPhase::Idle));
        assert!(ScrollPhase::Fling > ScrollPhase::Idle);
    }

    #[test]
    fn sweeping_is_the_two_phases_that_owe_no_full_render() {
        assert!(!ScrollPhase::Idle.is_sweeping());
        assert!(!ScrollPhase::Slow.is_sweeping());
        assert!(!ScrollPhase::Normal.is_sweeping());
        assert!(ScrollPhase::Fast.is_sweeping());
        assert!(ScrollPhase::Fling.is_sweeping());
        assert!(ScrollPhase::Slow.is_moving());
        assert!(!ScrollPhase::Idle.is_moving());
    }

    #[test]
    fn hysteresis_holds_a_phase_across_its_own_boundary() {
        let viewport = 900.0;
        // The speed at which Normal begins, expressed in pixels for this
        // viewport: the number a reader hovering on a line actually sits on.
        let line = SLOW_SCREEN_PER_SEC * viewport;
        // Arriving from below, the line itself is not yet Normal…
        assert_eq!(
            ScrollPhase::classify_from(ScrollPhase::Slow, line, viewport),
            ScrollPhase::Slow
        );
        // …but past the entry margin above it, it is.
        assert_eq!(
            ScrollPhase::classify_from(ScrollPhase::Slow, line * 1.4, viewport),
            ScrollPhase::Normal
        );
        // And falling back through the same number keeps Normal until the
        // speed drops under the exit margin below it.
        assert_eq!(
            ScrollPhase::classify_from(ScrollPhase::Normal, line, viewport),
            ScrollPhase::Normal
        );
        assert_eq!(
            ScrollPhase::classify_from(ScrollPhase::Normal, line * 0.5, viewport),
            ScrollPhase::Slow
        );
    }

    #[test]
    fn a_jump_straight_into_a_fling_is_a_fling() {
        // Hysteresis must not turn a scrollbar drag into a staircase: the
        // reader who goes from still to throwing in one sample is throwing,
        // and the pages between here and the destination are the ones that
        // must not be rasterised.
        let viewport = 700.0;
        let throwing = FLING_SCREEN_PER_SEC * viewport * 2.0;
        assert_eq!(
            ScrollPhase::classify_from(ScrollPhase::Idle, throwing, viewport),
            ScrollPhase::Fling
        );
        assert_eq!(
            ScrollPhase::classify_from(ScrollPhase::Idle, -throwing, viewport),
            ScrollPhase::Fling
        );
    }

    #[test]
    fn a_speed_parked_on_a_line_does_not_oscillate() {
        // The failure hysteresis exists for: a boundary speed sampled with a
        // little noise must not alternate phases, because a phase change
        // resizes the mount window and re-prioritises every queued render.
        let viewport = 800.0;
        let boundary = FLING_SCREEN_PER_SEC * viewport;
        let mut phase = ScrollPhase::Normal;
        let mut flips = 0;
        for step in 0..40 {
            let speed = if step % 2 == 0 {
                boundary * 1.02
            } else {
                boundary * 0.98
            };
            let next = ScrollPhase::classify_from(phase, speed, viewport);
            if next != phase {
                flips += 1;
                phase = next;
            }
        }
        assert!(flips <= 1, "phase flipped {flips} times on a jittering boundary speed");
    }

    #[test]
    fn prediction_projects_the_direction_of_travel() {
        let p = Predictor::default();
        // Down: the projection lands ahead.
        assert!(p.offset(1_000.0, 2_000.0, 800.0, 100_000.0) > 1_000.0);
        // Up: behind.
        assert!(p.offset(5_000.0, -2_000.0, 800.0, 100_000.0) < 5_000.0);
        // Still: nowhere.
        assert_eq!(p.offset(5_000.0, 0.0, 800.0, 100_000.0), 5_000.0);
    }

    #[test]
    fn prediction_is_clamped_to_the_document() {
        let p = Predictor::default();
        // Past the end: the last scrollable offset, not a page that does not
        // exist.
        assert_eq!(p.offset(9_900.0, 30_000.0, 800.0, 10_000.0), 10_000.0);
        // Before the start: zero, and never negative (the padding band's
        // negative offsets are the caller's business, not a prediction's).
        assert_eq!(p.offset(10.0, -30_000.0, 800.0, 10_000.0), 0.0);
        // An empty document has nowhere to go.
        assert_eq!(p.offset(0.0, 30_000.0, 800.0, 0.0), 0.0);
    }

    #[test]
    fn a_fling_predicts_a_distance_not_an_absurd_one() {
        let p = Predictor::new(120.0, 4.0);
        // 30000px/s over 120ms is 3600px — under the four-screen cap on an
        // 800px viewport only just, so the cap is what bounds it here.
        let projected = p.offset(0.0, 30_000.0, 800.0, 100_000.0);
        assert!(projected <= 4.0 * 800.0 + 1e-9, "got {projected}");
        // And the same speed on a taller viewport projects further, because
        // the cap is in screens rather than pixels.
        let tall = p.offset(0.0, 30_000.0, 2_000.0, 100_000.0);
        assert!(tall > projected);
    }

    #[test]
    fn prediction_ignores_a_corrupt_speed() {
        let p = Predictor::default();
        assert_eq!(p.offset(1_000.0, f64::NAN, 800.0, 9_000.0), 1_000.0);
        assert_eq!(p.offset(1_000.0, f64::INFINITY, 800.0, 9_000.0), 1_000.0);
        assert_eq!(p.offset(1_000.0, 5_000.0, 0.0, 9_000.0), 1_000.0);
    }

    #[test]
    fn look_ahead_screens_grow_with_speed_and_stop_at_the_cap() {
        let p = Predictor::new(120.0, 4.0);
        assert_eq!(p.screens(0.0, 800.0), 0.0);
        let walking = p.screens(800.0, 800.0);
        let flinging = p.screens(8_000.0, 800.0);
        assert!(walking < flinging);
        assert!(flinging <= 4.0 + 1e-9);
        assert_eq!(p.screens(8_000.0, 0.0), 0.0);
    }
}
