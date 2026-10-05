//! Scroll motion: what the reader is doing to the scrollbar, and what the
//! content pipeline therefore owes them.
//!
//! A placeholder is only a win when filling the item genuinely would not have
//! made it in time. That single sentence needs two numbers a virtualizer
//! historically does not have — how fast this scroller is moving, and how fast
//! this machine can make content — and it is why a distance band, applied to
//! every scroll at every speed, both shows blanks during ordinary reading and
//! buys nothing when it does.
//!
//! [`Motion`] measures the first number: signed velocity in pixels per second,
//! estimated over the adapter's frame samples with a time-constant blend, so a
//! 30 Hz renderer and a 120 Hz one read the same speed for the same scroll.
//! [`Pipeline`] carries the second, measured by the caller: its median fill cost
//! and how many items it fills at once. [`Motion::band`] joins them, and
//! engagement requires *both* conditions — the reader is genuinely flicking, and
//! pages are arriving faster than the pipeline can make them:
//!
//! - a fast machine reading short pages never shows a placeholder, because the
//!   content is ready before the reader arrives;
//! - a fling through a large PDF that outruns the raster lanes shows placeholders
//!   for the items it is flying past, and spends its budget on the
//!   [`FillPriority`] classes that matter, in order.
//!
//! The band's leading pad is `speed × fill_ms` — the distance the reader covers
//! while one fill is in flight, which is exactly how far ahead of them the warm
//! region has to reach — clamped to a floor and a ceiling in viewport screens.
//! There is no magic overscan constant to tune: stop scrolling and the band
//! closes to the window, and a slower pipeline widens it by itself.
//!
//! The [`Direction`] latch and the engagement latch are both hysteretic for the
//! same reason: a boundary that flips on one sample is a boundary the reader
//! sees. A direction only turns when a sample's own sign clears the flip floor,
//! so the rubber-band recoil at the end of a flick cannot move the lead side to
//! the wrong end for a frame; engagement only lets go well below where it took
//! hold, so a scroll that slows across the threshold does not strobe the
//! placeholder mode.
//!
//! Pure arithmetic: no clocks, no DOM, no framework, no `std`. The adapter owns
//! the frame chain that calls [`Motion::update`] and the scroll-end event that
//! calls [`Motion::settle`]. The rules here are tested against the sample
//! streams those produce — see `tests/motion_band.rs`.

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

/// How soon the reader will look at a mounted item — the order a fill queue
/// works in, which during a scroll is never the document order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FillPriority {
    /// Overlapping the viewport. Nothing outranks it.
    #[default]
    Visible,
    /// Outside the viewport, inside the band, on the side being approached:
    /// fill it while the reader is still traveling toward it.
    Ahead,
    /// Inside the band, behind the reader: cheap to keep, last of the band.
    Behind,
    /// Mounted as a placeholder. No content work at all until it moves up.
    Warm,
}

impl FillPriority {
    /// Sort key, lower first — so a caller orders a queue with one comparison
    /// instead of matching on the enum at every pop.
    pub const fn rank(self) -> u8 {
        match self {
            Self::Visible => 0,
            Self::Ahead => 1,
            Self::Behind => 2,
            Self::Warm => 3,
        }
    }
}

/// Tuning for [`Motion`]. The speed gates are px/s because that is the unit a
/// scroller reports; the band's limits are viewport screens because a
/// pixel is not a unit a reader scrolls in, and the same flick crosses far more
/// of a short window than a tall one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionConfig {
    /// Smoothing time constant, milliseconds. The blend in [`Motion::update`]
    /// weights a sample by `1 - e^(-dt/tau)`, so `tau` is how many
    /// milliseconds of movement it takes the estimate to reach ~63 % of a
    /// constant-speed scroll.
    pub tau_ms: f64,
    /// Speed at which a scroll counts as a seek, px/s — the *floor*, and the
    /// estimate must also outrun the pipeline's capacity before placeholders
    /// engage.
    pub enter_floor_px_s: f64,
    /// The fraction of the entry gate an engaged scroll must fall below to let
    /// go. Below 1.0 by construction, because the gap between the two gates is
    /// what stops a slowing scroll from strobing the placeholder mode; clamped
    /// into `0.05..=0.99` where it is read, so a bad value cannot make the
    /// latch open and close on the same sample.
    pub hysteresis: f64,
    /// The fraction of `enter_floor_px_s` a sample's own movement must clear to
    /// turn the [`Direction`] around.
    pub flip_ratio: f64,
    /// Smallest lead while engaged, in viewport screens.
    pub min_lead_screens: f64,
    /// Largest lead, in viewport screens. Warming past this buys RAM and
    /// nothing else: no pipeline fills that far ahead in time, so those items
    /// are placeholders either way.
    pub max_lead_screens: f64,
    /// How much of a screen stays warm behind the reader while engaged.
    pub trail_screens: f64,
}

impl Default for MotionConfig {
    fn default() -> Self {
        Self {
            tau_ms: 32.0,
            // ~2 screens/s on a 700 px window: a deliberate flick, not wheel
            // stepping. A reader with a smaller window engages later in pixels,
            // which is the point of expressing it in screens.
            enter_floor_px_s: 1_400.0,
            hysteresis: 0.4,
            flip_ratio: 0.35,
            min_lead_screens: 0.5,
            max_lead_screens: 2.0,
            trail_screens: 0.25,
        }
    }
}

/// What the caller's content pipeline can do, measured rather than assumed: the
/// fill cost is its own recent median, the lane count is what it actually runs.
/// This is the input that makes engagement a fact about the machine instead of
/// a constant copied from a demo.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pipeline {
    /// Median time to make one item's content real, milliseconds. `0` means the
    /// caller has not measured it yet, which reads as "no capacity" and leaves
    /// the decision to the speed floor.
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

    /// The gate that opens the placeholder mode: the reader must beat the
    /// pipeline's throughput *and* the config's speed floor. Beating only one
    /// of them is not a reason to show an empty page — a huge fling a fast
    /// machine keeps up with should still be real content, and a slow pipeline
    /// nudged at reading speed should not be blanked either.
    pub fn enter_px_s(&self, config: &MotionConfig) -> f64 {
        config.enter_floor_px_s.max(self.capacity_px_s())
    }

    /// The gate that closes it, strictly below the entry gate. Derived from it
    /// rather than fixed, so a reader on a slow pipeline holds the band while
    /// demand stays high and lets go as soon as the content can catch up.
    pub fn exit_px_s(&self, config: &MotionConfig) -> f64 {
        self.enter_px_s(config) * config.hysteresis.clamp(0.05, 0.99)
    }
}

/// A band of content coordinates: `start` inclusive, `end` exclusive, exactly
/// the half-open shape [`crate::Layout::overlapping`] takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandRange {
    /// First coordinate inside the band.
    pub start: f64,
    /// One past the last.
    pub end: f64,
}

/// The band policy for one frame: the slice of the document that carries real
/// content, and whether the rest of the mount window is a placeholder.
///
/// `placeholder` is the whole answer: `false` means every mounted item renders,
/// so a caller must not consult `active` for culling (it still reports the
/// padded viewport, so a diagnostic can show what the band would have been).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandWindow {
    /// The content band, in content coordinates. The caller intersects it with
    /// its mount window to get the indices that render.
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
///
/// [`Motion::update`] is the only input to the estimate and takes nothing but
/// position and time; [`Motion::band`] is the policy evaluator and owns the
/// engagement latch, because engagement is only meaningful against a pipeline.
/// [`Motion::engaged`] therefore reports the latch as of the last `band` call —
/// `false` before the first one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    config: MotionConfig,
    offset: f64,
    at_ms: f64,
    velocity_px_s: f64,
    direction: Direction,
    engaged: bool,
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
        }
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
    ///
    /// Call it once per animation frame from the adapter's frame chain, and
    /// again from the scroll listener: the displacement over the interval is
    /// what counts, so a burst of scroll events inside one frame reads as the
    /// one movement it is. A non-positive interval (a duplicate timestamp, a
    /// clock that went backwards across a document swap) records the position
    /// and leaves the estimate alone rather than dividing by it.
    pub fn update(&mut self, offset: f64, now_ms: f64) {
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

        // The sample's own sign turns the direction, but only past the flip
        // floor, and only if the estimate is still moving: a recoil is a stop,
        // not a turn. The floor comes from the config's speed floor rather than
        // `Pipeline::enter_px_s` because `update` deliberately knows nothing
        // about the pipeline — it is a measurement, not a policy.
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

    /// The scroll ended: `scrollend`, or the adapter's debounce firing. The
    /// browser has said so in words, so the estimate goes to rest at once
    /// instead of smoothing toward it over the frames that will not come.
    pub fn settle(&mut self) {
        self.velocity_px_s = 0.0;
        self.direction = Direction::Still;
        self.engaged = false;
    }

    /// The band this frame deserves.
    ///
    /// `offset` and `viewport` are the scroller's; `overscan_px` is the padding
    /// the mount policy already applies, which the band never goes under — a
    /// band narrower than what is already mounted would blank items whose DOM
    /// is paid for; `pipeline` is the caller's measured throughput.
    ///
    /// Engagement needs both conditions behind [`Pipeline::enter_px_s`]: a
    /// genuinely quick scroll, and one arriving faster than the pipeline can
    /// fill. [`Pipeline::exit_px_s`] is the same latch, lower, so a fling dying
    /// away does not strobe the placeholder mode; [`Motion::settle`] ends the
    /// state outright.
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
        // The lead goes where the reader is going; at Still it has to cover
        // both ends, because a stalled fling may resume either way.
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

    /// The index the viewport is expected to reach by the time one fill
    /// finishes, so a prefetch is aimed at a place rather than at a direction.
    ///
    /// The partial item is rounded *toward* the reader (`trunc`), which is the
    /// conservative choice: a prefetch that arrives early is free, one that
    /// arrives late is a blank. Never below zero; the caller clamps to its own
    /// item count.
    pub fn landing_index(&self, index: usize, pitch: f64, fill_ms: f64) -> usize {
        let pitch = pitch.max(1.0);
        let moved = self.velocity_px_s * (fill_ms.max(0.0) / 1_000.0);
        let landed = index as f64 + (moved / pitch).trunc();
        if landed <= 0.0 {
            0
        } else {
            landed as usize
        }
    }

    /// The urgency of one mounted index. `visible` is the window overlapping
    /// the viewport; `band` is the window that must carry content, or `None`
    /// when the whole mount window does — the ordinary-speed case, where every
    /// item is [`FillPriority::Visible`] because nothing may be a placeholder.
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
            // Engaged with no direction: the reader may go either way, so
            // nothing outranks anything else inside the band.
            Direction::Still => FillPriority::Behind,
        }
    }
}

/// `1 - e^(-x)` for `x >= 0`, evaluated without `libm` so the kernel stays
/// `no_std`: `e^x = 2^n * e^r` with `r` reduced to `[-ln2/2, ln2/2]` and a
/// factorial series there, which converges to better than one part in 10^9 in
/// nine terms. `x >= 20` is 1.0 to double precision anyway.
fn one_minus_exp_neg(x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x > 20.0 {
        return 1.0;
    }
    const LOG2_E: f64 = 1.442_695_040_888_963_4;
    const LN_2: f64 = 0.693_147_180_559_945_3;
    let n = (x * LOG2_E + 0.5) as i32;
    let r = f64::from(n) * LN_2 - x;
    // e^r = sum r^k / k!, and the `k = 0` term is exactly the `1` that
    // `1 - e^-x` takes back off, so only k >= 1 is accumulated.
    let mut term = 1.0;
    let mut sum = 0.0;
    let mut k = 1;
    while k <= 9 {
        term *= r / k as f64;
        sum += term;
        k += 1;
    }
    // e^-x = 2^-n * e^r, so 1 - e^-x = 1 - 2^-n - 2^-n * sum. Scaling by a
    // power of two is an exponent edit, not a multiplication.
    let scale = f64::from_bits((1_023 - n as u64) << 52);
    (1.0 - scale) - scale * sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exp_helper_matches_the_definition() {
        // Reference values computed once, so the test needs no `std` float
        // intrinsics in a `no_std` build. `x = n * ln 2` reduces to `r = 0`,
        // where the result is exact by construction: 1 - 2^-n.
        assert_eq!(one_minus_exp_neg(0.0), 0.0);
        assert_eq!(one_minus_exp_neg(25.0), 1.0);
        assert_eq!(one_minus_exp_neg(0.693_147_180_559_945_3), 0.5);
        assert_eq!(one_minus_exp_neg(1.386_294_361_119_890_6), 0.75);
        assert_eq!(one_minus_exp_neg(2.079_441_541_679_835_7), 0.875);
        // And the in-between samples land on the definition to 1e-9.
        for (x, wanted) in [
            (0.02_f64, 0.019_801_326_702_611_44_f64),
            (1.0, 0.632_120_558_828_557_6),
            (5.0, 0.993_262_053_000_914_5),
            (12.0, 0.999_993_919_789_048_3),
        ] {
            assert!(
                (one_minus_exp_neg(x) - wanted).abs() < 1e-9,
                "x={x}: {} vs {wanted}",
                one_minus_exp_neg(x)
            );
        }
    }

    #[test]
    fn a_band_range_is_half_open_and_never_negative() {
        let band = BandRange {
            start: 100.0,
            end: 250.0,
        };
        assert_eq!(band.len(), 150.0);
        assert!(band.contains(100.0));
        assert!(!band.contains(250.0));
        let inverted = BandRange {
            start: 300.0,
            end: 100.0,
        };
        assert_eq!(inverted.len(), 0.0);
    }
}

// only the changed file was rewritten
