//! Scroll motion: the measured speed, the pipeline's capacity, and the
//! band that follows.

use core::f64::consts::{LN_2, LOG2_E};

use crate::Window;

/// Which way the scroller is moving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    /// Nothing worth naming: at rest, or movement that has not resolved.
    #[default]
    Still,
    /// The offset is increasing — down a vertical list, forward through a
    /// document.
    Forward,
    /// The offset is decreasing.
    Backward,
}

/// How soon the reader will look at a mounted item, as a rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FillPriority {
    /// Overlapping the viewport. Nothing outranks it.
    #[default]
    Visible,
    /// Outside the viewport, inside the band, on the side approached.
    Ahead,
    /// Inside the band, behind the reader: cheap to keep, last of the band.
    Behind,
    /// Mounted as a placeholder. No content work at all until it moves up.
    Warm,
}

impl FillPriority {
    /// Sort key, lower first.
    pub const fn rank(self) -> u8 {
        match self {
            Self::Visible => 0,
            Self::Ahead => 1,
            Self::Behind => 2,
            Self::Warm => 3,
        }
    }
}

/// Tuning for [`Motion`]: px/s gates, screen-sized band limits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionConfig {
    /// Smoothing time constant in ms: the blend's `1 - e^(-dt/tau)` weight.
    pub tau_ms: f64,
    /// Speed at which a scroll counts as a seek, px/s.
    pub enter_floor_px_s: f64,
    /// The fraction of the entry gate an engaged scroll falls below to
    /// let go.
    pub hysteresis: f64,
    /// The fraction of `enter_floor_px_s` a sample's own movement must clear to
    /// turn the [`Direction`] around.
    pub flip_ratio: f64,
    /// Smallest lead while engaged, in viewport screens.
    pub min_lead_screens: f64,
    /// Largest lead, in viewport screens; past it buys only RAM.
    pub max_lead_screens: f64,
    /// How much of a screen stays warm behind the reader while engaged.
    pub trail_screens: f64,
}

impl Default for MotionConfig {
    fn default() -> Self {
        Self {
            tau_ms: 32.0,
            // ~2 screens/s on a 700 px window: a deliberate flick.
            enter_floor_px_s: 1_400.0,
            hysteresis: 0.4,
            flip_ratio: 0.35,
            min_lead_screens: 0.5,
            max_lead_screens: 2.0,
            trail_screens: 0.25,
        }
    }
}

/// What the caller's content pipeline can do, measured not assumed.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pipeline {
    /// Time to make one item's content real, in ms; 0 means unmeasured.
    pub fill_ms: f64,
    /// The layout's average item extent, pixels. `0` is the same unknown.
    pub pitch: f64,
    /// How many items the pipeline fills at once.
    pub lanes: usize,
}

impl Pipeline {
    /// Items per second this pipeline can sustain.
    pub fn items_per_s(&self) -> f64 {
        if self.lanes == 0 || self.fill_ms <= 0.0 {
            return 0.0;
        }
        (self.lanes as f64) * (1_000.0 / self.fill_ms.max(1.0))
    }

    /// The same, in pixels per second: items times the extent each one covers.
    pub fn capacity_px_s(&self) -> f64 {
        self.items_per_s() * self.pitch.max(0.0)
    }

    /// The gate that opens the placeholder mode.
    pub fn enter_px_s(&self, config: &MotionConfig) -> f64 {
        config.enter_floor_px_s.max(self.capacity_px_s())
    }

    /// The gate that closes it, strictly below the entry gate.
    pub fn exit_px_s(&self, config: &MotionConfig) -> f64 {
        self.enter_px_s(config) * config.hysteresis.clamp(0.05, 0.99)
    }
}

/// A band of content coordinates: `start` inclusive, `end` exclusive, exactly
/// the half-open shape [`crate::Layout::overlapping`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BandRange {
    /// First coordinate inside the band.
    pub start: f64,
    /// One past the last.
    pub end: f64,
}

/// The band policy for one frame: the content slice and the
/// placeholder flag.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BandWindow {
    /// The content band, in content coordinates.
    pub active: BandRange,
    /// Whether items outside `active` render as placeholders.
    pub placeholder: bool,
    /// Lead past the viewport's far edge, in the direction of travel.
    pub lead_px: f64,
    /// Padding against the direction of travel.
    pub trail_px: f64,
    /// The estimate this band was computed from, px/s.
    pub speed_px_s: f64,
    /// And its direction.
    pub direction: Direction,
}

/// The estimated motion of one scroller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    config: MotionConfig,
    offset: f64,
    at_ms: f64,
    velocity_px_s: f64,
    direction: Direction,
    engaged: bool,
    /// Whether `offset`/`at_ms` hold a real sample yet.
    primed: bool,
}

impl Motion {
    /// A fresh estimator, at rest.
    pub const fn new(config: MotionConfig) -> Self {
        Self {
            config,
            offset: 0.0,
            at_ms: 0.0,
            velocity_px_s: 0.0,
            direction: Direction::Still,
            engaged: false,
            primed: false,
        }
    }

    /// Whether any scroll sample has reached the estimator yet.
    pub const fn primed(&self) -> bool {
        self.primed
    }

    /// Rebase the estimator on a position it did not measure itself — a
    /// container that bound late, or a programmatic jump. The next sample
    /// then measures a real interval instead of the reader's whole session.
    pub fn seed(&mut self, offset: f64, now_ms: f64) {
        self.offset = offset;
        self.at_ms = now_ms;
        self.velocity_px_s = 0.0;
        self.direction = Direction::Still;
        self.engaged = false;
        self.primed = true;
    }

    /// The last sampled scroll position.
    pub const fn offset(&self) -> f64 {
        self.offset
    }

    /// The signed velocity estimate, pixels per second. Positive is
    /// [`Direction::Forward`].
    pub const fn velocity_px_s(&self) -> f64 {
        self.velocity_px_s
    }

    /// Its magnitude.
    pub fn speed_px_s(&self) -> f64 {
        self.velocity_px_s.abs()
    }

    /// The latched direction of travel.
    pub const fn direction(&self) -> Direction {
        self.direction
    }

    /// Whether the last [`Motion::band`] treated this scroll as a seek.
    pub const fn engaged(&self) -> bool {
        self.engaged
    }

    /// Fold one scroll sample into the estimate.
    pub fn update(&mut self, offset: f64, now_ms: f64) {
        if !self.primed {
            // A first sample has no interval to measure: dividing by the
            // clock's origin would report how long the app has been open as
            // the time this scroll took.
            self.seed(offset, now_ms);
            return;
        }
        let dt = now_ms - self.at_ms;
        let delta = offset - self.offset;
        self.offset = offset;
        if dt <= 0.0 {
            return;
        }
        self.at_ms = now_ms;

        let instantaneous = delta * (1_000.0 / dt);
        let tau = self.config.tau_ms.max(1.0);
        let alpha = one_minus_exp_neg(dt / tau);
        self.velocity_px_s += (instantaneous - self.velocity_px_s) * alpha;

        // The sample's sign turns the direction, past the flip floor only.
        let flip = self.config.enter_floor_px_s.max(1.0) * self.config.flip_ratio;
        self.direction = if self.speed_px_s() < flip {
            Direction::Still
        } else {
            match (self.direction, instantaneous) {
                (Direction::Forward, i) if i > -flip => Direction::Forward,
                (Direction::Backward, i) if i < flip => Direction::Backward,
                (_, i) if i > 0.0 => Direction::Forward,
                (_, i) if i < 0.0 => Direction::Backward,
                _ => Direction::Still,
            }
        };
    }

    /// The scroll ended: the estimate goes to rest at once.
    pub fn settle(&mut self) {
        self.velocity_px_s = 0.0;
        self.direction = Direction::Still;
        self.engaged = false;
    }

    /// The band this frame deserves.
    pub fn band(
        &mut self,
        offset: f64,
        viewport: f64,
        overscan_px: f64,
        pipeline: &Pipeline,
    ) -> BandWindow {
        let screens = viewport.max(0.0);
        let overscan = overscan_px.max(0.0);
        let speed = self.speed_px_s();
        let enter = pipeline.enter_px_s(&self.config);
        let exit = pipeline.exit_px_s(&self.config);
        self.engaged = if self.engaged {
            speed > exit
        } else {
            speed >= enter
        };

        if !self.engaged {
            return BandWindow {
                active: BandRange {
                    start: offset - overscan,
                    end: offset + viewport + overscan,
                },
                placeholder: false,
                lead_px: 0.0,
                trail_px: overscan,
                speed_px_s: speed,
                direction: self.direction,
            };
        }

        let lead = (speed * pipeline.fill_ms.max(0.0) / 1_000.0)
            .max(self.config.min_lead_screens * screens)
            .min(self.config.max_lead_screens * screens)
            .max(overscan);
        let trail = (self.config.trail_screens * screens)
            .max(overscan)
            .min(lead);
        // The lead goes where the reader is going; Still covers both ends.
        let (before, after) = match self.direction {
            Direction::Forward => (trail, lead),
            Direction::Backward => (lead, trail),
            Direction::Still => (lead, lead),
        };
        BandWindow {
            active: BandRange {
                start: offset - before,
                end: offset + viewport + after,
            },
            placeholder: true,
            lead_px: lead,
            trail_px: trail,
            speed_px_s: speed,
            direction: self.direction,
        }
    }

    /// The index the viewport reaches by the time one fill finishes.
    pub fn landing_index(&self, index: usize, pitch: f64, fill_ms: f64) -> usize {
        let pitch = pitch.max(1.0);
        let moved = self.velocity_px_s * (fill_ms.max(0.0) / 1_000.0);
        let landed = index as f64 + (moved / pitch).trunc();
        if landed <= 0.0 { 0 } else { landed as usize }
    }

    /// The urgency of one mounted index.
    pub fn priority(&self, index: usize, visible: Window, band: Option<Window>) -> FillPriority {
        let Some(band) = band else {
            return FillPriority::Visible;
        };
        if !band.contains(index) {
            return FillPriority::Warm;
        }
        if visible.contains(index) {
            return FillPriority::Visible;
        }
        match self.direction {
            Direction::Forward => {
                if index > visible.last {
                    FillPriority::Ahead
                } else {
                    FillPriority::Behind
                }
            }
            Direction::Backward => {
                if index < visible.first {
                    FillPriority::Ahead
                } else {
                    FillPriority::Behind
                }
            }
            // Engaged with no direction: nothing outranks anything else.
            Direction::Still => FillPriority::Behind,
        }
    }
}

/// `1 - e^(-x)` for `x >= 0`, without `libm` so the kernel stays
/// `no_std`.
fn one_minus_exp_neg(x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x > 20.0 {
        return 1.0;
    }
    let n = (x * LOG2_E + 0.5) as i32;
    let r = f64::from(n) * LN_2 - x;
    // e^r = sum r^k / k!; the k = 0 term is taken back
    let mut term = 1.0;
    let mut sum = 0.0;
    let mut k = 1;
    while k <= 9 {
        term *= r / k as f64;
        sum += term;
        k += 1;
    }
    // e^-x = 2^-n * e^r: scaling by a power of two edits the exponent.
    let scale = f64::from_bits((1_023 - n as u64) << 52);
    (1.0 - scale) - scale * sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exp_helper_matches_the_definition() {
        // Reference values, so the test needs no float intrinsics.
        assert_eq!(one_minus_exp_neg(0.0), 0.0);
        assert_eq!(one_minus_exp_neg(25.0), 1.0);
        assert_eq!(one_minus_exp_neg(LN_2), 0.5);
        assert_eq!(one_minus_exp_neg(2.0 * LN_2), 0.75);
        assert_eq!(one_minus_exp_neg(3.0 * LN_2), 0.875);
        // And the in-between samples against the definition itself.
        for x in [0.05, 0.4, 1.0, 3.0, 5.0, 7.0] {
            let mut term = 1.0;
            let mut sum = 1.0;
            for k in 1..=60 {
                term *= -x / k as f64;
                sum += term;
            }
            let wanted = 1.0 - sum;
            assert!(
                (one_minus_exp_neg(x) - wanted).abs() < 1e-9,
                "x={x}: {} vs {wanted}",
                one_minus_exp_neg(x)
            );
        }
    }

    #[test]
    fn the_first_sample_is_the_origin_not_a_speed() {
        let mut motion = Motion::new(MotionConfig::default());
        assert!(!motion.primed());
        // The app has been open five minutes and the reader flicks.
        motion.update(900.0, 300_000.0);
        assert!(motion.primed());
        assert_eq!(motion.velocity_px_s(), 0.0);
        assert_eq!(motion.direction(), Direction::Still);
        assert!(!motion.engaged());
    }

    #[test]
    fn detection_does_not_depend_on_application_uptime() {
        let mut early = Motion::new(MotionConfig::default());
        let mut late = Motion::new(MotionConfig::default());
        for (motion, base) in [(&mut early, 0.0), (&mut late, 600_000.0)] {
            motion.update(0.0, base);
            motion.update(900.0, base + 16.0);
            motion.update(1_800.0, base + 32.0);
        }
        assert!(late.speed_px_s() > 30_000.0, "{}", late.speed_px_s());
        assert_eq!(late.direction(), Direction::Forward);
        assert!((late.speed_px_s() - early.speed_px_s()).abs() < 1e-6);
    }

    #[test]
    fn seeding_rebases_the_estimate() {
        let mut motion = Motion::new(MotionConfig::default());
        let idle = Pipeline::default();
        motion.update(0.0, 0.0);
        motion.update(4_000.0, 16.0);
        motion.band(4_000.0, 800.0, 0.0, &idle);
        assert!(motion.engaged(), "a real fling earns the band");
        // A programmatic jump is not reader momentum.
        motion.seed(90_000.0, 120_000.0);
        assert_eq!(motion.offset(), 90_000.0);
        assert_eq!(motion.velocity_px_s(), 0.0);
        assert_eq!(motion.direction(), Direction::Still);
        assert!(!motion.engaged(), "the band goes back to the viewport");
    }
}
