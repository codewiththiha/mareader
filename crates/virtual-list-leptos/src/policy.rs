//! The adaptive policy: the one place that decides what a scroll position, a
//! speed and a viewport mean for the pages around them.
//!
//! The virtualizer already had the primitives this sits on top of — a mount
//! window, a render band, zombie retention, a settled flag — and each of them
//! was configured with a number that never changed. That is the shape this
//! module replaces: one policy resolves the reader's MOTION into every
//! window and every delay at once, so the pieces stop disagreeing with each
//! other about how fast the reader is going.
//!
//! The rule the whole policy exists to enforce:
//!
//! > Mounting a page is not a request to rasterise it.
//!
//! A mounted item is an item whose geometry the layout is honest about. What
//! it is owed visually is a separate answer, and it depends on where the
//! reader is heading: [`RenderQuality::Full`] for the pages under their eyes
//! and the one they are about to reach, [`RenderQuality::Preview`] for the
//! pages a movement has made plausible, and [`RenderQuality::Placeholder`]
//! for the rest of the window — geometry, and nothing that allocates.
//!
//! Pure like everything else in this crate's core: no DOM, no timers, no
//! clock of its own. The caller supplies the phase, the direction and the
//! viewport; this module supplies the windows.

use virtual_list::{Slack, Window};

use crate::motion::{Predictor, ScrollPhase};

/// How far ahead to look, and how far that look is allowed to reach.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PredictionPolicy {
    /// The projection horizon, milliseconds of travel.
    pub horizon_ms: f64,
    /// The longest projection, in viewport screens. A fling's raw projection
    /// can run past the end of a short document; the cap is what keeps the
    /// prediction a page the reader will actually arrive at.
    pub max_screens: f64,
}

impl Default for PredictionPolicy {
    fn default() -> Self {
        Self {
            horizon_ms: 120.0,
            max_screens: 4.0,
        }
    }
}

impl PredictionPolicy {
    /// The predictor this policy describes.
    pub fn predictor(&self) -> Predictor {
        Predictor::new(self.horizon_ms, self.max_screens)
    }
}

/// What each visual tier is worth, in viewport screens, and what it costs.
///
/// Every screen count here is resolved ASYMMETRICALLY: the side the reader is
/// travelling towards gets the full amount, the side they are leaving gets
/// [`RenderPolicy::trail_ratio`] of it. Reading is directional, and a page
/// behind the reader is only coming back if they change their mind — which is
/// what zombie retention is for, not what the render band is for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderPolicy {
    /// Half-width of the FULL tier: pages rendered at the committed scale
    /// with their text layer. The tier the reader is looking at, so it stays
    /// tight — every screen added here is a full-page raster per screen.
    pub full_screens: f64,
    /// Half-width of the PREVIEW tier: pages held at a low-resolution raster
    /// and no text layer, upgraded to full when the reader settles on them.
    pub preview_screens: f64,
    /// Half-width of the MOUNT slack, in screens. The window itself still
    /// answers to [`virtual_list::Budget::max_items`]: this is how far the
    /// policy would like to reach before the ceiling trims it back.
    pub mount_screens: f64,
    /// What the trailing side gets, as a fraction of the leading one. `1.0`
    /// is the symmetric behaviour a motion-blind policy has.
    pub trail_ratio: f64,
    /// Extra screens of mount slack while the reader is sweeping, so the
    /// geometry of a fling's destination is already laid out when the
    /// movement stops. Costs DOM, not rasters: the tier inside it is
    /// [`RenderQuality::Placeholder`].
    pub sweep_mount_screens: f64,
    /// How long a newly mounted page waits before its render is queued, by
    /// phase. The point is not to be slow — it is to stop rasterising pages
    /// that a movement has already made irrelevant, which is the single
    /// largest source of wasted full-page surfaces during a fling.
    pub slow_delay_ms: u32,
    /// See [`Self::slow_delay_ms`]: the delay at [`ScrollPhase::Normal`].
    pub normal_delay_ms: u32,
    /// See [`Self::slow_delay_ms`]: the delay at [`ScrollPhase::Fast`].
    pub fast_delay_ms: u32,
    /// See [`Self::slow_delay_ms`]: the delay at [`ScrollPhase::Fling`].
    pub fling_delay_ms: u32,
    /// Raster lanes while the reader is settled. Deliberately NOT raised for
    /// throughput's sake: the settled moment is when the visible pages should
    /// land, and two lanes land them without three full-page rasters fighting
    /// for the same worker.
    pub settled_workers: usize,
    /// Raster lanes while sweeping. Fewer than settled on purpose — during a
    /// fling the goal is the shortest time to the first useful page, not the
    /// most pages started, and every lane that is busy is a lane the
    /// destination cannot have.
    pub sweeping_workers: usize,
    /// The output-scale multiplier a PREVIEW raster is rendered at: the same
    /// CSS geometry, a fraction of the pixels, no text layer. Below about a
    /// quarter the page stops reading as the page it stands in for; above
    /// about a half it stops being cheap, which is the only reason the tier
    /// exists.
    pub preview_scale: f64,
}

impl Default for RenderPolicy {
    fn default() -> Self {
        Self {
            full_screens: 0.75,
            preview_screens: 2.0,
            mount_screens: 2.5,
            trail_ratio: 0.4,
            sweep_mount_screens: 1.5,
            slow_delay_ms: 0,
            normal_delay_ms: 8,
            fast_delay_ms: 35,
            fling_delay_ms: 90,
            settled_workers: 2,
            sweeping_workers: 1,
            preview_scale: 0.45,
        }
    }
}

impl RenderPolicy {
    /// How long a page mounted under `phase` waits before its render is
    /// queued.
    pub fn delay_ms(&self, phase: ScrollPhase) -> u32 {
        match phase {
            ScrollPhase::Idle | ScrollPhase::Slow => self.slow_delay_ms,
            ScrollPhase::Normal => self.normal_delay_ms,
            ScrollPhase::Fast => self.fast_delay_ms,
            ScrollPhase::Fling => self.fling_delay_ms,
        }
    }

    /// How many raster lanes the scheduler should run under `phase`.
    pub fn workers(&self, phase: ScrollPhase) -> usize {
        if phase.is_sweeping() {
            self.sweeping_workers
        } else {
            self.settled_workers
        }
    }

    /// The screens a tier is worth on each side of the viewport: the leading
    /// side in full, the trailing side at [`Self::trail_ratio`].
    fn split(&self, screens: f64, phase: ScrollPhase) -> (f64, f64) {
        let screens = screens.max(0.0);
        let ratio = self.trail_ratio.clamp(0.0, 1.0);
        // A settled reader has no leading side: the tier is symmetric, which
        // is what makes an idle document look the same in every direction.
        if !phase.is_moving() {
            return (screens, screens);
        }
        (screens, screens * ratio)
    }
}

/// How many bytes of raster the reader is allowed to hold, and how the
/// tiers share them.
///
/// A page count is the wrong unit for a PDF: one poster page can cost ten
/// times what its neighbour does, so a document of 100 pages is not
/// necessarily cheaper than one of 20. The budget is therefore in bytes, and
/// the engine — which owns the canvases and knows their real dimensions —
/// enforces it. The numbers travel with the policy because they are policy:
/// what the reader is willing to spend is a product decision, not an
/// engine one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryPolicy {
    /// Total raster budget, bytes. Includes the full tier, the preview tier
    /// and the thumbnail cache; the engine reclaims from the cheapest tier
    /// upwards when it is exceeded.
    pub max_bytes: usize,
    /// The preview tier's own ceiling, bytes. Bounded separately because it
    /// is the tier that grows with a fling: without its own cap, a long
    /// throw would spend the whole budget on pages the reader is not going to
    /// stop on and leave nothing for the page they do.
    pub preview_bytes: usize,
    /// Hard ceiling on simultaneously held preview surfaces, whatever they
    /// cost. The byte budget's backstop for a document of huge pages.
    pub max_preview_pages: usize,
}

impl Default for MemoryPolicy {
    fn default() -> Self {
        Self {
            max_bytes: 96 * 1024 * 1024,
            preview_bytes: 24 * 1024 * 1024,
            max_preview_pages: 8,
        }
    }
}

/// How long an evicted page keeps its DOM, by phase.
///
/// Retention is reversal protection: readers fling down and drag back up, and
/// the pages they just left are the ones they are about to ask for again. The
/// faster the movement, the further a reversal reaches, so the grace grows
/// with the phase — bounded by [`Self::max_retained`], because a bridge that
/// is not bounded is not virtualisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// Grace at rest and at reading speed, milliseconds.
    pub settled_grace_ms: u32,
    /// Grace while covering ground, milliseconds.
    pub fast_grace_ms: u32,
    /// Grace during a throw, milliseconds.
    pub fling_grace_ms: u32,
    /// Ceiling on simultaneously retained items.
    pub max_retained: usize,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            settled_grace_ms: 120,
            fast_grace_ms: 220,
            fling_grace_ms: 300,
            max_retained: 12,
        }
    }
}

impl RetentionPolicy {
    /// The grace a page evicted under `phase` should get.
    pub fn grace_ms(&self, phase: ScrollPhase) -> u32 {
        match phase {
            ScrollPhase::Fling => self.fling_grace_ms,
            ScrollPhase::Fast => self.fast_grace_ms,
            _ => self.settled_grace_ms,
        }
    }
}

/// The whole adaptive policy: prediction, tiers, memory and retention.
///
/// Built with [`AdaptivePolicy::reader`] and adjusted with the `with_*`
/// setters, which take `self` and return `Self` so a caller can start from
/// the shipped numbers and move one of them:
///
/// ```ignore
/// AdaptivePolicy::reader().with_full_screens(1.0)
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdaptivePolicy {
    /// How far ahead to look.
    pub prediction: PredictionPolicy,
    /// What each tier is worth and what it costs.
    pub rendering: RenderPolicy,
    /// How much raster the reader may hold.
    pub memory: MemoryPolicy,
    /// How long an evicted page keeps its DOM.
    pub retention: RetentionPolicy,
}

impl Default for AdaptivePolicy {
    fn default() -> Self {
        Self::reader()
    }
}

impl AdaptivePolicy {
    /// The policy a document reader wants: a tight full tier, a preview ring
    /// two screens wide, a mount window that leans into the direction of
    /// travel, and a raster budget sized for a desktop webview.
    pub const fn reader() -> Self {
        Self {
            prediction: PredictionPolicy {
                horizon_ms: 120.0,
                max_screens: 4.0,
            },
            rendering: RenderPolicy {
                full_screens: 0.75,
                preview_screens: 2.0,
                mount_screens: 2.5,
                trail_ratio: 0.4,
                sweep_mount_screens: 1.5,
                slow_delay_ms: 0,
                normal_delay_ms: 8,
                fast_delay_ms: 35,
                fling_delay_ms: 90,
                settled_workers: 2,
                sweeping_workers: 1,
                preview_scale: 0.45,
            },
            memory: MemoryPolicy {
                max_bytes: 96 * 1024 * 1024,
                preview_bytes: 24 * 1024 * 1024,
                max_preview_pages: 8,
            },
            retention: RetentionPolicy {
                settled_grace_ms: 120,
                fast_grace_ms: 220,
                fling_grace_ms: 300,
                max_retained: 12,
            },
        }
    }

    /// Sets [`RenderPolicy::full_screens`].
    pub fn with_full_screens(mut self, screens: f64) -> Self {
        self.rendering.full_screens = screens;
        self
    }

    /// Sets [`RenderPolicy::preview_screens`].
    pub fn with_preview_screens(mut self, screens: f64) -> Self {
        self.rendering.preview_screens = screens;
        self
    }

    /// Sets [`RenderPolicy::mount_screens`].
    pub fn with_mount_screens(mut self, screens: f64) -> Self {
        self.rendering.mount_screens = screens;
        self
    }

    /// Sets [`PredictionPolicy::max_screens`].
    pub fn with_prediction_screens(mut self, screens: f64) -> Self {
        self.prediction.max_screens = screens;
        self
    }

    /// Sets [`MemoryPolicy::max_bytes`].
    pub fn with_memory_budget(mut self, bytes: usize) -> Self {
        self.memory.max_bytes = bytes;
        self
    }

    /// The mount slack for one frame: how far past the viewport the window
    /// reaches on each side, in pixels.
    ///
    /// `direction` is the sign of the smoothed velocity (`1` towards higher
    /// offsets, `-1` towards lower, `0` at rest) and decides which side is
    /// the leading one. A sweep widens the leading side by
    /// [`RenderPolicy::sweep_mount_screens`], because the geometry of a
    /// fling's destination is cheap to hold and expensive to be missing.
    pub fn mount_slack(&self, phase: ScrollPhase, direction: i8, viewport: f64) -> Slack {
        let render = &self.rendering;
        let mut lead = render.mount_screens.max(0.0);
        if phase.is_sweeping() {
            lead += render.sweep_mount_screens.max(0.0);
        }
        let trail = lead * render.trail_ratio.clamp(0.0, 1.0);
        let (before, after) = if direction < 0 {
            (lead * viewport, trail * viewport)
        } else {
            (trail * viewport, lead * viewport)
        };
        Slack::split(before, after)
    }

    /// The full-quality tier for one frame: the items overlapping the
    /// viewport padded by [`RenderPolicy::full_screens`], asymmetrically.
    pub fn full_slack(&self, phase: ScrollPhase, direction: i8, viewport: f64) -> Slack {
        self.tier_slack(self.rendering.full_screens, phase, direction, viewport)
    }

    /// The preview tier for one frame: [`RenderPolicy::preview_screens`],
    /// asymmetrically. Always at least as wide as the full tier, so the two
    /// can never cross and leave a gap no tier covers.
    pub fn preview_slack(&self, phase: ScrollPhase, direction: i8, viewport: f64) -> Slack {
        let screens = self
            .rendering
            .preview_screens
            .max(self.rendering.full_screens);
        self.tier_slack(screens, phase, direction, viewport)
    }

    fn tier_slack(
        &self,
        screens: f64,
        phase: ScrollPhase,
        direction: i8,
        viewport: f64,
    ) -> Slack {
        let (lead, trail) = self.rendering.split(screens, phase);
        let (before, after) = if direction < 0 {
            (lead, trail)
        } else {
            (trail, lead)
        };
        Slack::split(before * viewport, after * viewport)
    }
}

/// What one item is owed, visually.
///
/// This is the tier, not the lifecycle: a retained zombie still holds its own
/// last bitmap and is described by [`crate::VirtualItemState::Zombie`], which
/// the adapter layers on top of a tier decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderQuality {
    /// Rasterised at the committed scale, with its text and link layers.
    Full,
    /// A low-resolution raster and no text layer: enough to read the page's
    /// shape and colour, cheap enough to hold several of them.
    Preview,
    /// Geometry only — the placeholder tier, which must not allocate a
    /// canvas, a bitmap or a text layer per page.
    #[default]
    Placeholder,
}

impl RenderQuality {
    /// Whether this tier owes a raster at all.
    pub fn paints(&self) -> bool {
        matches!(self, Self::Full | Self::Preview)
    }
}

/// The frame's whole answer: where the reader is, where they are going, and
/// what each window around them is owed.
///
/// Cheap to copy and compare, which is what lets the adapter publish it as a
/// signal without waking the view on a movement that changed nothing it can
/// see: the windows are item ranges, and a scroll that stays inside one item
/// produces an equal plan.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RenderPlan {
    /// The movement's classification.
    pub phase: ScrollPhase,
    /// Smoothed signed speed, pixels per second.
    pub velocity: f64,
    /// Sign of the speed: `1` towards higher offsets, `-1` towards lower,
    /// `0` at rest.
    pub direction: i8,
    /// Where the viewport is, in content coordinates.
    pub scroll_top: f64,
    /// Where the reader is projected to be, clamped to the document.
    pub predicted_offset: f64,
    /// The item the projection lands on — the destination a scheduler should
    /// have ready, and the one page that outranks its neighbours even when
    /// the full tier does not reach it.
    pub predicted_index: usize,
    /// Items at least partly on screen.
    pub visible: Option<Window>,
    /// The full-quality tier.
    pub full: Option<Window>,
    /// The preview tier (a superset of [`Self::full`] whenever both exist).
    pub preview: Option<Window>,
    /// The mount window: geometry honest, content optional.
    pub mount: Option<Window>,
    /// How long a page mounted this frame waits before its render is queued.
    pub delay_ms: u32,
    /// How many raster lanes the scheduler should run.
    pub workers: usize,
    /// How long an item evicted this frame keeps its DOM.
    pub grace_ms: u32,
}

impl RenderPlan {
    /// What `index` is owed this frame.
    ///
    /// The visible range is answered first and unconditionally: a page the
    /// reader is looking at is never a placeholder, whatever the movement
    /// says, and never waits for a prediction to include it.
    pub fn quality(&self, index: usize) -> RenderQuality {
        if self.visible.map(|w| w.contains(index)).unwrap_or(false) {
            return RenderQuality::Full;
        }
        if self.full.map(|w| w.contains(index)).unwrap_or(false) {
            return RenderQuality::Full;
        }
        if self.preview.map(|w| w.contains(index)).unwrap_or(false) {
            return RenderQuality::Preview;
        }
        RenderQuality::Placeholder
    }

    /// The pages that outrank their neighbours this frame, in priority order:
    /// the predicted destination first, then the visible range from its
    /// leading edge. A scheduler that can only afford one render knows which
    /// one to spend it on.
    pub fn priorities(&self) -> impl Iterator<Item = usize> {
        let predicted = self.predicted_index;
        let visible = self.visible.into_iter().flat_map(|w| w.iter());
        core::iter::once(predicted)
            .chain(visible)
            .chain(self.full.into_iter().flat_map(|w| w.iter()))
    }

    /// Whether this frame's movement is fast enough that expensive work for
    /// pages outside the visible range is waste.
    pub fn is_sweeping(&self) -> bool {
        self.phase.is_sweeping()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VH: f64 = 800.0;

    fn policy() -> AdaptivePolicy {
        AdaptivePolicy::reader()
    }

    #[test]
    fn a_settled_reader_gets_symmetric_tiers() {
        let p = policy();
        let full = p.full_slack(ScrollPhase::Idle, 0, VH);
        assert_eq!(full.before, full.after);
        assert!((full.after - 0.75 * VH).abs() < 1e-9);
        // And the preview ring is wider than the full tier on both sides.
        let preview = p.preview_slack(ScrollPhase::Idle, 0, VH);
        assert!(preview.before > full.before && preview.after > full.after);
    }

    #[test]
    fn travelling_down_puts_the_lead_below() {
        let p = policy();
        let slack = p.full_slack(ScrollPhase::Normal, 1, VH);
        assert!(slack.after > slack.before, "lead must be on the far side");
        assert!((slack.after - 0.75 * VH).abs() < 1e-9);
        assert!((slack.before - 0.75 * VH * 0.4).abs() < 1e-9);
    }

    #[test]
    fn travelling_up_puts_the_lead_above() {
        let p = policy();
        let down = p.full_slack(ScrollPhase::Normal, 1, VH);
        let up = p.full_slack(ScrollPhase::Normal, -1, VH);
        assert!(up.before > up.after);
        // The same two numbers, mirrored: direction moves the tier, it does
        // not resize it.
        assert!((up.before - down.after).abs() < 1e-9);
        assert!((up.after - down.before).abs() < 1e-9);
    }

    #[test]
    fn a_sweep_widens_the_mount_window_ahead_only() {
        let p = policy();
        let settled = p.mount_slack(ScrollPhase::Idle, 1, VH);
        let fling = p.mount_slack(ScrollPhase::Fling, 1, VH);
        assert!(fling.after > settled.after);
        assert!(fling.before > settled.before);
        // The lead grows by more than the trail: the window leans into the
        // movement rather than inflating.
        let lead_gain = fling.after - settled.after;
        let trail_gain = fling.before - settled.before;
        assert!(lead_gain > trail_gain * 2.0);
    }

    #[test]
    fn an_upwards_sweep_leans_the_other_way() {
        let p = policy();
        let up = p.mount_slack(ScrollPhase::Fast, -1, VH);
        assert!(up.before > up.after);
    }

    #[test]
    fn the_tiers_never_cross() {
        // Whatever the phase, the preview ring contains the full tier: a gap
        // between them would be a band of mounted pages owed nothing at all
        // while pages further away hold a raster.
        let p = policy();
        for phase in [
            ScrollPhase::Idle,
            ScrollPhase::Slow,
            ScrollPhase::Normal,
            ScrollPhase::Fast,
            ScrollPhase::Fling,
        ] {
            for direction in [-1i8, 0, 1] {
                let full = p.full_slack(phase, direction, VH);
                let preview = p.preview_slack(phase, direction, VH);
                assert!(preview.before >= full.before, "{phase:?} {direction}");
                assert!(preview.after >= full.after, "{phase:?} {direction}");
                let mount = p.mount_slack(phase, direction, VH);
                assert!(mount.before >= preview.before, "{phase:?} {direction}");
                assert!(mount.after >= preview.after, "{phase:?} {direction}");
            }
        }
    }

    #[test]
    fn negative_and_huge_screen_counts_clamp_rather_than_invert() {
        let p = policy()
            .with_full_screens(-1.0)
            .with_preview_screens(-0.5)
            .with_mount_screens(-2.0);
        let slack = p.mount_slack(ScrollPhase::Fling, 1, VH);
        assert_eq!(slack.before, 0.0);
        assert_eq!(slack.after, 0.0);
        assert_eq!(slack.total(), 0.0);
        let wide = policy().with_preview_screens(1e9).preview_slack(ScrollPhase::Idle, 0, VH);
        assert!(wide.after.is_finite() && wide.after > 0.0);
    }

    #[test]
    fn delays_and_lanes_follow_the_phase() {
        let p = policy();
        assert_eq!(p.rendering.delay_ms(ScrollPhase::Idle), 0);
        assert_eq!(p.rendering.delay_ms(ScrollPhase::Slow), 0);
        assert!(p.rendering.delay_ms(ScrollPhase::Normal) > 0);
        assert!(p.rendering.delay_ms(ScrollPhase::Fast) > p.rendering.delay_ms(ScrollPhase::Normal));
        assert!(p.rendering.delay_ms(ScrollPhase::Fling) > p.rendering.delay_ms(ScrollPhase::Fast));
        // Fewer lanes while sweeping: the destination wants the worker, not
        // the pages being flown past.
        assert!(p.rendering.workers(ScrollPhase::Fling) < p.rendering.workers(ScrollPhase::Idle));
        assert_eq!(p.rendering.workers(ScrollPhase::Fast), 1);
        assert_eq!(p.rendering.workers(ScrollPhase::Slow), 2);
    }

    #[test]
    fn retention_grace_grows_with_the_movement() {
        let p = policy();
        let settled = p.retention.grace_ms(ScrollPhase::Idle);
        assert_eq!(settled, p.retention.grace_ms(ScrollPhase::Normal));
        assert!(p.retention.grace_ms(ScrollPhase::Fast) > settled);
        assert!(p.retention.grace_ms(ScrollPhase::Fling) > p.retention.grace_ms(ScrollPhase::Fast));
        assert!(p.retention.max_retained > 0);
    }

    #[test]
    fn the_memory_budget_gives_the_preview_tier_its_own_ceiling() {
        let p = policy();
        assert!(p.memory.preview_bytes < p.memory.max_bytes);
        assert!(p.memory.max_preview_pages > 0);
        let custom = p.with_memory_budget(48 * 1024 * 1024);
        assert_eq!(custom.memory.max_bytes, 48 * 1024 * 1024);
        // The preview ceiling is a separate number and does not move with it.
        assert_eq!(custom.memory.preview_bytes, p.memory.preview_bytes);
    }

    #[test]
    fn a_preview_is_materially_cheaper_than_the_raster_it_stands_in_for() {
        let scale = policy().rendering.preview_scale;
        assert!(scale > 0.2, "a preview below a quarter stops reading as the page");
        assert!(scale < 0.75, "a preview above three quarters is not cheap");
        // Pixels go as the square of the scale: the tier's whole argument.
        assert!(scale * scale < 0.4);
    }

    #[test]
    fn the_setters_move_one_number_each() {
        let base = policy();
        assert_eq!(base.with_full_screens(1.5).rendering.full_screens, 1.5);
        assert_eq!(base.with_full_screens(1.5).rendering.preview_screens, base.rendering.preview_screens);
        assert_eq!(base.with_preview_screens(3.0).rendering.preview_screens, 3.0);
        assert_eq!(base.with_mount_screens(2.0).rendering.mount_screens, 2.0);
        assert_eq!(base.with_prediction_screens(6.0).prediction.max_screens, 6.0);
        assert_eq!(base.with_prediction_screens(6.0).prediction.horizon_ms, base.prediction.horizon_ms);
    }

    #[test]
    fn the_predictor_is_the_policy_prediction() {
        let p = policy().with_prediction_screens(2.0);
        let predictor = p.prediction.predictor();
        // Two screens of an 800px viewport is the cap, whatever the speed.
        assert!(predictor.offset(0.0, 40_000.0, VH, 1e9) <= 2.0 * VH + 1e-9);
        assert!(predictor.screens(40_000.0, VH) <= 2.0 + 1e-9);
    }

    fn window(first: usize, last: usize) -> Window {
        Window { first, last }
    }

    #[test]
    fn a_visible_page_is_full_quality_whatever_the_movement_says() {
        // The invariant the whole tier model rests on: the page under the
        // reader's eyes is never a placeholder, and never waits for a
        // prediction to happen to include it.
        let plan = RenderPlan {
            phase: ScrollPhase::Fling,
            direction: 1,
            visible: Some(window(9, 10)),
            full: Some(window(14, 16)),
            preview: Some(window(12, 20)),
            mount: Some(window(6, 24)),
            predicted_index: 17,
            ..RenderPlan::default()
        };
        assert_eq!(plan.quality(9), RenderQuality::Full);
        assert_eq!(plan.quality(10), RenderQuality::Full);
        assert_eq!(plan.quality(15), RenderQuality::Full);
        assert_eq!(plan.quality(13), RenderQuality::Preview);
        assert_eq!(plan.quality(7), RenderQuality::Placeholder);
        assert!(plan.is_sweeping());
    }

    #[test]
    fn quality_answers_placeholder_outside_the_window() {
        let plan = RenderPlan {
            mount: Some(window(4, 8)),
            full: Some(window(5, 7)),
            ..RenderPlan::default()
        };
        assert_eq!(plan.quality(0), RenderQuality::Placeholder);
        assert_eq!(plan.quality(100), RenderQuality::Placeholder);
        // An empty plan (no layout yet) owes nothing and promises nothing.
        let empty = RenderPlan::default();
        assert_eq!(empty.quality(0), RenderQuality::Placeholder);
        assert!(!empty.is_sweeping());
    }

    #[test]
    fn priorities_put_the_destination_first() {
        let plan = RenderPlan {
            visible: Some(window(10, 11)),
            full: Some(window(10, 13)),
            predicted_index: 13,
            ..RenderPlan::default()
        };
        let order: Vec<usize> = plan.priorities().collect();
        assert_eq!(order[0], 13);
        // The visible pages follow immediately, in reading order.
        assert_eq!(order[1], 10);
        assert_eq!(order[2], 11);
        // …and the rest of the full tier after them.
        assert!(order.contains(&12));
    }

    #[test]
    fn placeholder_tier_paints_nothing() {
        assert!(!RenderQuality::Placeholder.paints());
        assert!(RenderQuality::Preview.paints());
        assert!(RenderQuality::Full.paints());
        assert_eq!(RenderQuality::default(), RenderQuality::Placeholder);
    }
}
