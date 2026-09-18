//! The virtualizer engine: a pure state machine over a [`virtual_list::Layout`].
//!
//! Every input is a transition (`on_scroll`, `flush`, ...), returning a
//! [`Step`] that describes what the framework adapter must apply (range write,
//! corrected scroll, layout-version bump). No signals, no DOM, and no timers
//! live here — which is why the entire refresh engine is unit-tested on the
//! host against a `TestSurface` test double.
//!
//! Coordinates are **content coordinates**: `0` is the top of the first item;
//! negative offsets address the scrollable `padding_start` band that sits
//! before it.

use virtual_list::{
    Align, AnchorPolicy, Budget, GridLayout, Layout, LayoutKind, ListLayout, Slack, Viewport,
    Window, correct, pin_at, rescale_anchor,
};

use crate::motion::{ScrollPhase, ScrollVelocity};
use crate::options::{LayoutShape, ScrollMode};
use crate::policy::{AdaptivePolicy, RenderPlan, RenderQuality};
use crate::render::{VirtualItem, VirtualItemState, VirtualRow};
use crate::surface::ScrollSurface;

/// Engine configuration that does not change per frame.
#[derive(Debug, Clone)]
pub struct CoreConfig {
    /// Mount budget (overscan + ceiling).
    pub budget: Budget,
    /// List or grid.
    pub shape: LayoutShape,
    /// List gap (grids fold the gap into the row pitch).
    pub gap: f64,
    /// Content padding before the first item.
    pub padding_start: f64,
    /// Content padding after the last item.
    pub padding_end: f64,
    /// Initial viewport (`main` = scroll-axis extent, `cross` = across).
    pub viewport: Viewport,
    /// Initial scroll position (content coordinates).
    pub initial_offset: f64,
    /// Change-detection epsilon.
    pub eps: f64,
    /// How many times an in-flight `scroll_to_index` may re-aim.
    pub max_retries: u32,
    /// The render band, in viewport screens around the viewport: mounted
    /// items outside it answer [`VirtualItemState::Blank`]. `0` disables the
    /// band (pages mode: everything mounted renders fully). Ignored when
    /// [`Self::adaptive`] is set — an adaptive policy owns every tier.
    pub render_screens: f64,
    /// The motion-aware policy. `None` (the default) is the motion-blind
    /// behaviour: a symmetric band from [`Self::render_screens`], overscan
    /// from [`CoreConfig::budget`], and every mounted item full quality.
    /// `Some` hands the mount slack, the two tiers and the predicted
    /// destination to [`AdaptivePolicy`], which resolves all three from the
    /// scroll velocity — the budget then supplies only the mount ceiling.
    pub adaptive: Option<AdaptivePolicy>,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            budget: Budget::default(),
            shape: LayoutShape::List,
            gap: 0.0,
            padding_start: 0.0,
            padding_end: 0.0,
            viewport: Viewport::main_only(0.0),
            initial_offset: 0.0,
            eps: 0.5,
            max_retries: 3,
            render_screens: 0.0,
            adaptive: None,
        }
    }
}

/// What a transition changed; the adapter applies it to signals and the DOM.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Step {
    /// The current mount window.
    pub range: Option<Window>,
    /// A corrected scroll position to apply, in content coordinates.
    pub scroll_write: Option<f64>,
    /// The layout's geometry changed — bump the layout version.
    pub layout_changed: bool,
    /// The frame's tier plan: where the reader is, where they are going, and
    /// what each window around them is owed. Always present, including for a
    /// transition that changed nothing, so an adapter never has to decide
    /// whether the plan it holds is still current.
    pub plan: RenderPlan,
}

/// The outcome of a measurement flush.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flush {
    /// How many measurements actually changed a size.
    pub applied: usize,
    /// What the adapter should apply.
    pub step: Step,
}

/// An in-flight `scroll_to_index`, re-aimed when measurements move the target.
#[derive(Debug, Clone, Copy)]
struct PendingScroll {
    index: usize,
    align: Align,
    last_target: f64,
    attempts: u32,
}

/// The framework-free core: layout, windowing and scroll state in one
/// value, driven by the adapter in [`crate::Virtualizer`].
pub struct VirtualizerCore {
    layout: LayoutKind,
    budget: Budget,
    shape: LayoutShape,
    gap: f64,
    padding_start: f64,
    padding_end: f64,
    eps: f64,
    max_retries: u32,
    render_screens: f64,

    hint: usize,
    scroll_top: f64,
    viewport: Viewport,
    range: Option<Window>,
    pinned: Option<(usize, usize)>,
    pending: Option<PendingScroll>,
    queue: Vec<(usize, f64)>,
    suspended: bool,

    /// The motion-aware policy, when the caller configured one.
    adaptive: Option<AdaptivePolicy>,
    /// The smoothed scroll velocity. Only fed by [`Self::on_scroll`]: a
    /// programmatic write is not motion and must not be read as one.
    velocity: ScrollVelocity,
    /// The classified movement, carried across samples so classification can
    /// apply hysteresis.
    phase: ScrollPhase,
    /// The clock reading of the last motion sample, kept so a reset has a
    /// "now" to restart from without the core owning a clock.
    clock_ms: f64,
    /// Whether scroll samples are the reader's or ours. A commanded scroll
    /// (a page turn, a search reveal, a zoom's geometry commit) is echoed
    /// back by the browser as a burst of scroll events that look exactly
    /// like a fling; while muted, samples still move the window but never
    /// reach the estimator. [`Self::settle`] — the adapter's scroll-end
    /// timer, which the echo burst re-arms like any other — lifts it.
    motion_muted: bool,
    /// The frame's plan. Recomputed by [`Self::rewindow`], read by every
    /// tier question the core answers.
    plan: RenderPlan,
}

impl VirtualizerCore {
    /// Build a core around its initial layout.
    pub fn new(layout: LayoutKind, config: CoreConfig) -> Self {
        let mut this = Self {
            layout,
            budget: config.budget,
            shape: config.shape,
            gap: config.gap,
            padding_start: config.padding_start,
            padding_end: config.padding_end,
            eps: config.eps,
            max_retries: config.max_retries,
            render_screens: config.render_screens,
            hint: 0,
            scroll_top: config.initial_offset,
            viewport: config.viewport,
            range: None,
            pinned: None,
            pending: None,
            queue: Vec::new(),
            suspended: false,
            adaptive: config.adaptive,
            velocity: ScrollVelocity::at(config.initial_offset, 0.0),
            phase: ScrollPhase::Idle,
            clock_ms: 0.0,
            motion_muted: false,
            plan: RenderPlan::default(),
        };
        this.scroll_top = this.scroll_top.clamp(this.min_scroll(), this.max_scroll());
        this.velocity = ScrollVelocity::at(this.scroll_top, 0.0);
        this.range = this.rewindow().range;
        this
    }

    /// The container scrolled. `content_top` is in content coordinates and
    /// `now_ms` is the caller's monotonic clock — the core owns no clock of
    /// its own, which is what keeps the motion model testable on the host.
    ///
    /// Sub-epsilon deltas (a scroll event that moved less than `eps` — the
    /// browser fires scroll events for fractional-pixel wheel deltas) are
    /// ignored wholesale: they cannot change the window, they are not motion
    /// worth classifying, and adopting them would wake every `scroll_top`
    /// consumer (dominant-page tracking, navigation sync) for a movement the
    /// display cannot even show.
    pub fn on_scroll(&mut self, content_top: f64, now_ms: f64) -> Step {
        if (content_top - self.scroll_top).abs() <= self.eps {
            return Step {
                range: self.range,
                scroll_write: None,
                layout_changed: false,
                plan: self.plan,
            };
        }
        self.clock_ms = now_ms;
        self.scroll_top = content_top;
        if self.adaptive.is_some() && !self.motion_muted {
            let speed = self.velocity.update(content_top, now_ms);
            self.phase = ScrollPhase::classify_from(self.phase, speed, self.viewport.main);
        }
        self.rewindow()
    }

    /// The scroller has been quiet for the adapter's scroll-end window: the
    /// movement is over.
    ///
    /// This is a transition, not a cleanup. The motion model has to be told,
    /// because the last scroll event of a fling still reads as a fling — and
    /// an idle reader is owed the opposite of what a fling is: symmetric
    /// tiers, no render delay, both raster lanes, and the settle promotion
    /// that upgrades the preview ring under their eyes to full quality.
    pub fn settle(&mut self, now_ms: f64) -> Step {
        self.clock_ms = now_ms;
        self.motion_muted = false;
        self.velocity.reset(self.scroll_top, now_ms);
        self.phase = ScrollPhase::Idle;
        self.rewindow()
    }

    /// Forget the motion estimate, and stop believing the samples that
    /// follow until the scroller goes quiet.
    ///
    /// For a jump that is not motion: a programmatic scroll (a page turn, a
    /// search reveal, the mount-time anchor), a geometry rebuild, or a zoom's
    /// rescale all write `scroll_top` by an amount no reader travelled, and
    /// the browser echoes that write back through [`Self::on_scroll`] over
    /// the next few frames. Fed to the estimator, a smooth page turn reads as
    /// a fling of several thousand pixels per second — which would narrow the
    /// tiers and delay the render of exactly the page the reader asked for.
    /// [`Self::settle`] lifts the mute.
    pub fn reset_motion(&mut self) {
        self.velocity.reset(self.scroll_top, self.clock_ms);
        self.phase = ScrollPhase::Idle;
        self.motion_muted = true;
    }

    /// The visible extent changed; re-window against it.
    pub fn on_viewport(&mut self, vp: Viewport) -> Step {
        let current = self.viewport;
        let main_changed = (vp.main - current.main).abs() > self.eps;
        let cross_changed = (vp.cross - current.cross).abs() > self.eps;
        if !main_changed && !cross_changed {
            return Step {
                range: self.range,
                scroll_write: None,
                layout_changed: false,
                plan: self.plan,
            };
        }

        let rebuilt = match self.shape {
            LayoutShape::Grid(spec) if cross_changed => {
                let count = self.layout.item_count();
                let pitch = match &self.layout {
                    LayoutKind::Grid(grid) => grid.row_pitch(),
                    LayoutKind::List(_) => {
                        if count > 0 {
                            self.layout.item_size_hint()
                        } else {
                            0.0
                        }
                    }
                };
                self.layout = LayoutKind::Grid(GridLayout::resolve(spec, count, pitch, vp.cross));
                self.hint = 0;
                true
            }
            _ => false,
        };

        self.viewport = vp;
        let mut scroll_write = None;
        let max_scroll = self.max_scroll();
        if self.scroll_top > max_scroll {
            self.scroll_top = max_scroll;
            scroll_write = Some(max_scroll);
        }

        let mut step = self.rewindow();
        step.layout_changed = rebuilt;
        // The viewport's own scroll correction always wins over a pending
        // scroll-to's landing write: the frame that moves the viewport IS the
        // ground truth the pending target is being re-aimed against.
        if scroll_write.is_some() {
            if rebuilt {
                self.refresh_pending_target();
            }
            step.scroll_write = scroll_write;
        } else if rebuilt {
            step.scroll_write = self.settle_pending();
        }
        step
    }

    /// Replace the extra indices that must stay mounted.
    pub fn set_pinned(&mut self, pinned: Option<(usize, usize)>) -> Step {
        self.pinned = pinned;
        self.rewindow()
    }

    /// The item count changed; `sizes` estimates the items the layout has
    /// not measured.
    pub fn set_count(&mut self, count: usize, sizes: &dyn Fn(usize) -> f64) -> Step {
        if count == self.layout.item_count() {
            return self.rewindow();
        }

        let anchor = if self.layout.is_empty() {
            None
        } else {
            let item = self.layout.dominant(self.scroll_top, self.viewport.main);
            Some((item, self.scroll_top - self.layout.offset(item)))
        };

        let shape = self.shape;
        let gap = self.gap;
        let cross = self.viewport.cross;
        self.layout = build_layout(&shape, count, sizes, cross, gap);
        self.hint = 0;
        self.pending = None;
        self.queue.clear();
        self.reset_motion();

        let max_scroll = self.max_scroll();
        self.scroll_top = match anchor {
            Some((item, px)) if count > 0 => {
                let item = item.min(count - 1);
                (self.layout.offset(item) + px).clamp(self.min_scroll(), max_scroll)
            }
            _ => self.scroll_top.clamp(self.min_scroll(), max_scroll),
        };

        let mut step = self.rewindow();
        step.layout_changed = true;
        step.scroll_write = Some(self.scroll_top);
        step
    }

    /// Rebuild the layout at the current count from fresh sizes, preserving
    /// the reader's anchor even when geometry changes without a count change.
    pub fn rebuild(&mut self, sizes: &dyn Fn(usize) -> f64) -> Step {
        let count = self.layout.item_count();
        let anchor = if self.layout.is_empty() {
            None
        } else {
            let item = self.layout.dominant(self.scroll_top, self.viewport.main);
            Some((item, self.scroll_top - self.layout.offset(item)))
        };
        let (shape, gap, cross) = (self.shape, self.gap, self.viewport.cross);
        self.layout = build_layout(&shape, count, sizes, cross, gap);
        self.hint = 0;
        self.queue.clear();
        self.reset_motion();
        self.scroll_top = match anchor {
            Some((item, px)) if count > 0 => {
                let item = item.min(count - 1);
                (self.layout.offset(item) + px).clamp(self.min_scroll(), self.max_scroll())
            }
            _ => self.scroll_top.clamp(self.min_scroll(), self.max_scroll()),
        };

        let mut step = self.rewindow();
        step.layout_changed = true;
        step.scroll_write = Some(self.scroll_top);
        self.refresh_pending_target();
        step
    }

    /// Queue one measured size for the next [`Self::flush`].
    pub fn queue_size(&mut self, index: usize, size: f64) {
        self.queue.push((index, size.max(0.0)));
    }

    /// Hold queued measurements until [`Self::resume`].
    pub fn suspend(&mut self) {
        self.suspended = true;
    }

    /// Resume flushing; flushes immediately if anything queued while suspended.
    pub fn resume(&mut self) -> Option<Flush> {
        self.suspended = false;
        self.flush()
    }

    /// Apply every queued measurement as one transaction.
    pub fn flush(&mut self) -> Option<Flush> {
        if self.suspended || self.queue.is_empty() {
            return None;
        }

        let mut queued = core::mem::take(&mut self.queue);
        queued.sort_unstable_by_key(|(index, _)| *index);

        let mut merged: Vec<(usize, f64)> = Vec::with_capacity(queued.len());
        for (index, size) in queued {
            if let Some(last) = merged.last_mut()
                && last.0 == index
            {
                last.1 = size;
                continue;
            }
            merged.push((index, size));
        }

        let anchor = self.dominant();
        let mut applied = 0usize;
        let mut new_top = self.scroll_top;
        for (index, size) in merged {
            if index >= self.layout.item_count() {
                continue;
            }
            if (self.layout.size(index) - size).abs() <= self.eps {
                continue;
            }
            let delta = self.layout.set_size(index, size);
            if delta != 0.0 {
                applied += 1;
                new_top = correct(new_top, AnchorPolicy::Item(anchor), index, delta);
            }
        }

        if applied == 0 {
            return Some(Flush {
                applied: 0,
                step: Step {
                    range: self.range,
                    scroll_write: None,
                    layout_changed: false,
                    plan: self.plan,
                },
            });
        }

        let max_scroll = self.max_scroll();
        new_top = new_top.clamp(self.min_scroll(), max_scroll);

        let mut scroll_write = None;
        if (new_top - self.scroll_top).abs() > self.eps {
            self.scroll_top = new_top;
            scroll_write = Some(new_top);
        }

        let mut step = self.rewindow();
        step.layout_changed = true;
        if let Some(target) = scroll_write {
            step.scroll_write = Some(target);
            self.refresh_pending_target();
        } else {
            step.scroll_write = self.settle_pending();
        }

        Some(Flush { applied, step })
    }

    /// Multiply every size by `factor`, keeping the content point under the
    /// viewport center fixed.
    pub fn rescale(&mut self, factor: f64, sizes: &dyn Fn(usize) -> f64) -> Step {
        if self.layout.is_empty() || factor <= 0.0 || factor.is_nan() {
            return self.rewindow();
        }

        let (item, px) = pin_at(&self.layout, self.scroll_top, self.viewport.main, 0.5);
        let new_top = rescale_anchor(&self.layout, self.scroll_top, item, px, factor);
        let count = self.layout.item_count();
        let shape = self.shape;
        let gap = self.gap;
        let cross = self.viewport.cross;
        self.layout = build_layout(&shape, count, sizes, cross, gap);
        self.hint = 0;
        self.pending = None;
        self.reset_motion();
        if let Some(top) = new_top {
            self.scroll_top = top.clamp(self.min_scroll(), self.max_scroll());
        }

        let mut step = self.rewindow();
        step.layout_changed = true;
        step.scroll_write = Some(self.scroll_top);
        step
    }

    /// Scroll to an absolute content offset (clamped).
    ///
    /// The surface is written once, here. An **instant** write is adopted
    /// into the core state immediately, so a geometry rebuild in the same
    /// tick (a document switch) anchors at the NEW position rather than the
    /// stale pre-jump one — the returned [`Step`] lets the adapter apply the
    /// new window without a second DOM write. A smooth write waits for the
    /// browser echo (`on_scroll`) and returns `None`.
    pub fn scroll_to_offset(
        &mut self,
        content_top: f64,
        mode: ScrollMode,
        surface: &impl ScrollSurface,
    ) -> Option<Step> {
        // An explicit offset always supersedes an in-flight programmatic
        // scroll; otherwise a pending re-aim could fight the new position.
        self.pending = None;
        // A commanded scroll is not motion: the browser echoes the write back
        // through `on_scroll` a frame later, and an estimate fed that echo
        // would read a page turn as a fling.
        self.reset_motion();
        let target = content_top.clamp(self.min_scroll(), self.max_scroll());
        let smooth = self.resolve_smooth(target, mode);
        surface.set_scroll(target, smooth);
        if smooth {
            None
        } else {
            self.scroll_top = target;
            Some(self.rewindow())
        }
    }

    /// Same instant-adoption contract as [`Self::scroll_to_offset`]. The
    /// pending-scroll bookkeeping is armed for BOTH behaviors so a
    /// measurement that moves the target can re-aim an in-flight scroll;
    /// only instant writes adopt locally.
    pub fn scroll_to_index(
        &mut self,
        index: usize,
        align: Align,
        mode: ScrollMode,
        surface: &impl ScrollSurface,
    ) -> Option<Step> {
        if self.layout.is_empty() {
            return None;
        }
        self.reset_motion();
        let index = index.min(self.layout.item_count() - 1);
        let Some(target) = self.target_offset(index, align) else {
            self.pending = None;
            return None;
        };
        let smooth = self.resolve_smooth(target, mode);
        self.pending = Some(PendingScroll {
            index,
            align,
            last_target: target,
            attempts: 0,
        });
        surface.set_scroll(target, smooth);
        if smooth {
            None
        } else {
            self.scroll_top = target;
            Some(self.rewindow())
        }
    }

    /// The current mount window; `None` before the first layout.
    pub fn range(&self) -> Option<Window> {
        self.range
    }

    /// The render band: the tighter window inside the mount window that
    /// carries real content at full quality. With no band configured it IS the
    /// mount window (pages mode); with one, it is the items overlapping the
    /// viewport padded by `render_screens` viewport screens each way,
    /// intersected with the mount window — the band never mounts, it only
    /// decides which of the mounted items render. Under an adaptive policy it
    /// is the policy's full tier, which leans in the direction of travel
    /// instead of being symmetric. Every partly-visible item is inside it by
    /// construction, so nothing the reader is looking at is ever a
    /// placeholder.
    pub fn render_range(&self) -> Option<Window> {
        self.plan.full
    }

    /// The preview tier: the band plus the ring around it that is worth a
    /// cheap raster but not a full one. Equal to [`Self::render_range`] when
    /// no adaptive policy is configured, so a motion-blind surface has no
    /// preview tier at all.
    pub fn preview_range(&self) -> Option<Window> {
        self.plan.preview
    }

    /// This frame's whole tier plan — the windows, the motion behind them and
    /// the predicted destination. The plan is the API a renderer should read:
    /// [`Self::render_range`] and [`Self::item_state`] are projections of it,
    /// kept for callers that only ask one question.
    pub fn render_plan(&self) -> RenderPlan {
        self.plan
    }

    /// The render state of a mounted index: [`VirtualItemState::Active`]
    /// inside the full tier, [`VirtualItemState::Preview`] inside the preview
    /// ring, [`VirtualItemState::Blank`] for the rest of the window. The
    /// adapter overrides it for retained zombies.
    pub fn item_state(&self, index: usize) -> VirtualItemState {
        match self.plan.quality(index) {
            RenderQuality::Full => VirtualItemState::Active,
            RenderQuality::Preview => VirtualItemState::Preview,
            RenderQuality::Placeholder => VirtualItemState::Blank,
        }
    }

    /// Current scroll position.
    pub fn scroll_top(&self) -> f64 {
        self.scroll_top
    }

    /// Current viewport.
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// The item the reader is looking at.
    pub fn dominant(&self) -> usize {
        if self.layout.is_empty() {
            0
        } else {
            self.layout.dominant(self.scroll_top, self.viewport.main)
        }
    }

    /// Full spacer extent, paddings included.
    pub fn total_size(&self) -> f64 {
        self.padding_start + self.layout.total() + self.padding_end
    }

    /// Lowest scrollable content offset: the start of the
    /// `padding_start` band.
    fn min_scroll(&self) -> f64 {
        -self.padding_start
    }

    /// Largest scrollable content offset.
    pub fn max_scroll(&self) -> f64 {
        (self.layout.total() + self.padding_end - self.viewport.main).max(0.0)
    }

    /// Item offset including `padding_start`.
    pub fn offset_of(&self, index: usize) -> f64 {
        self.padding_start + self.layout.offset(index)
    }

    /// Index of the item whose span contains `pos` (leading-edge semantics),
    /// `O(log n)` over the layout's prefix sums. Positions past the end resolve
    /// to the last item. The inverse of [`Self::offset_of`]: subtracting
    /// `padding_start` keeps the two in the same coordinate frame.
    pub fn index_at(&self, pos: f64) -> usize {
        if self.layout.is_empty() {
            0
        } else {
            self.layout.index_at(pos - self.padding_start)
        }
    }

    /// Resolved column count for grids.
    pub fn columns(&self) -> Option<usize> {
        match &self.layout {
            LayoutKind::Grid(grid) => Some(grid.columns()),
            LayoutKind::List(_) => None,
        }
    }

    /// Number of items.
    pub fn item_count(&self) -> usize {
        self.layout.item_count()
    }

    /// Whether measurements are suspended.
    pub fn suspended(&self) -> bool {
        self.suspended
    }

    /// The in-flight pending scroll, if any.
    pub fn pending(&self) -> Option<(usize, Align)> {
        self.pending.map(|pending| (pending.index, pending.align))
    }

    /// Borrow the layout.
    pub fn layout(&self) -> &LayoutKind {
        &self.layout
    }

    /// The mounted items, DOM-ready (`start` includes `padding_start`).
    ///
    /// An item's state says what the renderer owes it: [`VirtualItemState::Active`]
    /// inside the full tier, [`VirtualItemState::Preview`] inside the preview
    /// ring, [`VirtualItemState::Blank`] for the rest of the window. Zombie
    /// retention is the adapter's layer on top (it knows the grace clock this
    /// pure core does not).
    pub fn items(&self) -> Vec<VirtualItem> {
        let Some(window) = self.range else {
            return Vec::new();
        };
        (window.first..=window.last)
            .map(|index| {
                let mut item = self.item_at(index);
                item.state = self.item_state(index);
                item
            })
            .collect()
    }

    /// One item's render contract, window-independent: valid for any index
    /// in the layout (the layout models every item; only MOUNTING is
    /// windowed). The adapter uses this to keep freshly evicted items
    /// rendered at their laid-out position for a short grace period.
    pub fn item_at(&self, index: usize) -> VirtualItem {
        VirtualItem {
            index,
            start: self.layout.offset(index) + self.padding_start,
            size: self.layout.size(index),
            cross_start: self.layout.cross_offset(index),
            cross_size: self.layout.cross_size(index),
            row: match &self.layout {
                LayoutKind::Grid(grid) => grid.row_of(index),
                LayoutKind::List(_) => index,
            },
            state: crate::render::VirtualItemState::Active,
        }
    }

    /// The mounted rows.
    pub fn rows(&self) -> Vec<VirtualRow> {
        let Some(window) = self.range else {
            return Vec::new();
        };
        match &self.layout {
            LayoutKind::Grid(grid) => {
                let row_first = grid.row_of(window.first);
                let row_last = grid.row_of(window.last);
                (row_first..=row_last)
                    .map(|row| VirtualRow {
                        row,
                        start: grid.row_offset(row) + self.padding_start,
                        items: grid.row_items(row),
                    })
                    .collect()
            }
            LayoutKind::List(_) => (window.first..=window.last)
                .map(|index| VirtualRow {
                    row: index,
                    start: self.layout.offset(index) + self.padding_start,
                    items: index..index + 1,
                })
                .collect(),
        }
    }

    /// Recompute the window from the current state using the hinted search.
    fn rewindow(&mut self) -> Step {
        let range = if self.layout.is_empty() {
            None
        } else {
            let mut hint = self.hint;
            // With a policy the mount slack is the motion's answer, not the
            // budget's overscan: the budget keeps only its ceiling, which is
            // the one number that must NOT depend on how fast the reader is
            // going (a fling may look further ahead, but it may not mount the
            // whole document to do it).
            let base = match self.adaptive {
                Some(policy) => {
                    let slack = policy.mount_slack(
                        self.phase,
                        self.velocity.direction(),
                        self.viewport.main,
                    );
                    self.layout.window_slack_hinted(
                        self.scroll_top,
                        self.viewport,
                        slack,
                        self.budget.max_items,
                        &mut hint,
                    )
                }
                None => {
                    self.layout
                        .window_hinted(self.scroll_top, self.viewport, self.budget, &mut hint)
                }
            };
            self.hint = hint;
            match (base, self.pinned) {
                (Some(window), Some((first, last))) => {
                    let last_index = self.layout.item_count() - 1;
                    Some(window.union(Window {
                        first: first.min(last_index),
                        last: last.min(last_index),
                    }))
                }
                (None, Some((first, last))) if first <= last => {
                    let last_index = self.layout.item_count() - 1;
                    Some(Window {
                        first: first.min(last_index),
                        last: last.min(last_index),
                    })
                }
                (other, _) => other,
            }
        };
        self.range = range;
        let plan = self.build_plan(range);
        self.plan = plan;
        Step {
            range,
            scroll_write: None,
            layout_changed: false,
            plan,
        }
    }

    /// Resolve this frame's tiers from the motion and the mount window.
    ///
    /// Every window is intersected with the mount window: a tier can only
    /// describe items that exist, and an item outside the window owes nothing
    /// at all (the adapter decides whether its DOM is being bridged as a
    /// zombie).
    fn build_plan(&self, mount: Option<Window>) -> RenderPlan {
        let viewport = self.viewport.main;
        let visible = if self.layout.is_empty() {
            None
        } else {
            self.layout.visible(self.scroll_top, viewport)
        };
        let Some(policy) = self.adaptive else {
            // Motion-blind: one band from `render_screens`, or no band at all.
            let full = if self.render_screens > 0.0 {
                self.banded(Slack::symmetric(self.render_screens * viewport), mount)
            } else {
                mount
            };
            return RenderPlan {
                phase: ScrollPhase::Idle,
                velocity: 0.0,
                direction: 0,
                scroll_top: self.scroll_top,
                predicted_offset: self.scroll_top,
                predicted_index: self.dominant(),
                visible,
                full,
                preview: full,
                mount,
                delay_ms: 0,
                workers: 0,
                grace_ms: 0,
            };
        };

        let direction = self.velocity.direction();
        let velocity = self.velocity.velocity();
        let predicted_offset = policy
            .prediction
            .predictor()
            .offset(self.scroll_top, velocity, viewport, self.max_scroll());
        let full = self.banded(policy.full_slack(self.phase, direction, viewport), mount);
        let preview = self.banded(policy.preview_slack(self.phase, direction, viewport), mount);
        RenderPlan {
            phase: self.phase,
            velocity,
            direction,
            scroll_top: self.scroll_top,
            predicted_offset,
            predicted_index: self.index_at(predicted_offset),
            visible,
            full,
            preview,
            mount,
            delay_ms: policy.rendering.delay_ms(self.phase),
            workers: policy.rendering.workers(self.phase),
            grace_ms: policy.retention.grace_ms(self.phase),
        }
    }

    /// The items overlapping the viewport padded by `slack`, intersected with
    /// the mount window. `None` when there is nothing to intersect with.
    fn banded(&self, slack: Slack, mount: Option<Window>) -> Option<Window> {
        let mount = mount?;
        let band = self.layout.overlapping(
            self.scroll_top - slack.before,
            self.viewport.main + slack.total(),
        )?;
        let first = band.first.max(mount.first);
        let last = band.last.min(mount.last);
        (first <= last).then_some(Window { first, last })
    }

    /// Resolve [`ScrollMode::Auto`].
    fn resolve_smooth(&self, target: f64, mode: ScrollMode) -> bool {
        match mode {
            ScrollMode::Instant => false,
            ScrollMode::Smooth => true,
            ScrollMode::Auto => (target - self.scroll_top).abs() <= 2.0 * self.viewport.main,
        }
    }

    /// The scroll target that puts `index` at `align`.
    fn target_offset(&self, index: usize, align: Align) -> Option<f64> {
        if self.layout.is_empty() {
            return None;
        }
        let index = index.min(self.layout.item_count() - 1);
        let start = self.layout.offset(index);
        let size = self.layout.size(index);
        let viewport = self.viewport.main;
        let raw = match align {
            Align::Start => start,
            Align::Center => start - (viewport - size) / 2.0,
            Align::End => start - viewport + size,
            Align::Auto => {
                if start >= self.scroll_top && start + size <= self.scroll_top + viewport {
                    return None;
                }
                if start < self.scroll_top {
                    start
                } else {
                    start - viewport + size
                }
            }
        };
        Some(raw.clamp(self.min_scroll(), self.max_scroll()))
    }

    /// Re-aim the pending scroll after the layout moved.
    fn settle_pending(&mut self) -> Option<f64> {
        let pending = self.pending?;
        let Some(target) = self.target_offset(pending.index, pending.align) else {
            self.pending = None;
            return None;
        };
        if (target - pending.last_target).abs() <= self.eps {
            self.pending = None;
            return None;
        }
        let slot = self.pending.as_mut()?;
        if slot.attempts >= self.max_retries {
            self.pending = None;
            return None;
        }
        slot.attempts += 1;
        slot.last_target = target;
        Some(target)
    }

    /// Sync the pending scroll's bookkeeping with a target we already applied.
    fn refresh_pending_target(&mut self) {
        let Some(pending) = self.pending else {
            return;
        };
        let Some(target) = self.target_offset(pending.index, pending.align) else {
            self.pending = None;
            return;
        };
        if let Some(slot) = self.pending.as_mut() {
            slot.last_target = target;
        }
    }
}

pub(crate) fn build_layout(
    shape: &LayoutShape,
    count: usize,
    sizes: &dyn Fn(usize) -> f64,
    cross_extent: f64,
    gap: f64,
) -> LayoutKind {
    match shape {
        LayoutShape::List => {
            let layout = ListLayout::estimated(count, sizes, gap);
            LayoutKind::List(layout)
        }
        LayoutShape::Grid(spec) => {
            let pitch = if count > 0 { sizes(0).max(0.0) } else { 0.0 };
            LayoutKind::Grid(GridLayout::resolve(*spec, count, pitch, cross_extent))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::TestSurface;
    use virtual_list::GridSpec;

    /// The clock the motion-blind tests hand to `on_scroll`. One constant
    /// reading for every sample: the estimator sees no elapsed time and so
    /// reports no motion, which is exactly what a test about windowing wants.
    const NOW: f64 = 0.0;

    fn list_core(count: usize, size: f64, vh: f64) -> VirtualizerCore {
        VirtualizerCore::new(
            LayoutKind::List(ListLayout::uniform(count, size, 0.0)),
            CoreConfig {
                budget: Budget::screenfuls(0.0, 1_000),
                viewport: Viewport::main_only(vh),
                ..CoreConfig::default()
            },
        )
    }

    fn grid_core(
        items: usize,
        pitch: f64,
        spec: GridSpec,
        viewport: Viewport,
        budget: Budget,
    ) -> VirtualizerCore {
        let layout = LayoutKind::Grid(GridLayout::resolve(spec, items, pitch, viewport.cross));
        VirtualizerCore::new(
            layout,
            CoreConfig {
                shape: LayoutShape::Grid(spec),
                viewport,
                budget,
                ..CoreConfig::default()
            },
        )
    }

    #[test]
    fn scroll_moves_the_window() {
        let mut core = list_core(100, 100.0, 200.0);
        assert_eq!(
            core.on_scroll(0.0, NOW).range,
            Some(Window { first: 0, last: 1 })
        );
        assert_eq!(
            core.on_scroll(1_000.0, NOW).range,
            Some(Window {
                first: 10,
                last: 11
            })
        );
    }

    #[test]
    fn measurement_above_the_viewport_shifts_scroll() {
        let mut core = list_core(100, 100.0, 200.0);
        let _ = core.on_scroll(5_000.0, NOW);
        core.queue_size(10, 200.0);
        let flush = core.flush().expect("flush");
        assert_eq!(flush.applied, 1);
        assert!(flush.step.layout_changed);
        assert_eq!(flush.step.scroll_write, Some(5_100.0));
        assert_eq!(core.dominant(), 50);
    }

    #[test]
    fn measurement_below_the_viewport_keeps_scroll() {
        let mut core = list_core(100, 100.0, 200.0);
        let _ = core.on_scroll(5_000.0, NOW);
        core.queue_size(90, 200.0);
        let flush = core.flush().expect("flush");
        assert_eq!(flush.step.scroll_write, None);
    }

    #[test]
    fn subpixel_measurements_are_filtered() {
        let mut core = list_core(10, 100.0, 200.0);
        let _ = core.on_scroll(0.0, NOW);
        core.queue_size(5, 100.3);
        let flush = core.flush().expect("flush");
        assert_eq!(flush.applied, 0);
        assert!(!flush.step.layout_changed);
    }

    #[test]
    fn last_measurement_wins_per_index() {
        let mut core = list_core(10, 100.0, 200.0);
        let _ = core.on_scroll(0.0, NOW);
        core.queue_size(5, 300.0);
        core.queue_size(5, 400.0);
        let flush = core.flush().expect("flush");
        assert_eq!(flush.applied, 1);
        assert_eq!(core.layout().size(5), 400.0);
    }

    #[test]
    fn count_shrink_clamps_and_reanchors() {
        let mut core = list_core(100, 100.0, 200.0);
        let _ = core.on_scroll(9_000.0, NOW);
        let estimate = |_index: usize| 100.0;
        let step = core.set_count(20, &estimate);
        assert!(step.layout_changed);
        assert_eq!(core.scroll_top(), 1_800.0);
        assert_eq!(step.scroll_write, Some(1_800.0));
    }

    #[test]
    fn pinned_indices_extend_the_window() {
        let mut core = list_core(100, 100.0, 200.0);
        let _ = core.on_scroll(0.0, NOW);
        let step = core.set_pinned(Some((50, 51)));
        let window = step.range.expect("window");
        assert!(window.contains(0) && window.contains(1));
        assert!(window.contains(50) && window.contains(51));
        let step = core.set_pinned(None);
        assert_eq!(step.range, Some(Window { first: 0, last: 1 }));
    }

    #[test]
    fn scroll_to_index_writes_and_echoes() {
        let surface = TestSurface::default();
        let mut core = list_core(100, 100.0, 200.0);
        // Instant: adopted into the core state immediately, no echo needed.
        assert!(core.scroll_to_index(50, Align::Start, ScrollMode::Instant, &surface).is_some());
        assert_eq!(core.scroll_top(), 5_000.0);
        assert_eq!(surface.writes(), vec![(5_000.0, false)]);

        let _ = core.on_scroll(5_000.0, NOW);
        // Auto within two viewports: smooth, so nothing adopts locally yet.
        assert!(core.scroll_to_index(52, Align::Start, ScrollMode::Auto, &surface).is_none());
        // Auto beyond two viewports: instant, adopted locally.
        assert!(core.scroll_to_index(0, Align::Start, ScrollMode::Auto, &surface).is_some());
        assert_eq!(surface.writes()[1], (5_200.0, true));
        assert_eq!(surface.writes()[2], (0.0, false));
        assert_eq!(core.scroll_top(), 0.0);
    }

    #[test]
    fn instant_scroll_is_adopted_before_a_geometry_rebuild() {
        // The document-switch race: the app writes scroll_top = 0 (Instant)
        // and the adapter's count-rebuild re-anchors in the same tick,
        // before the DOM echo lands. The rebuild must anchor at the NEW
        // position, not the stale pre-jump one.
        let surface = TestSurface::default();
        let mut core = list_core(100, 100.0, 200.0);
        let _ = core.on_scroll(5_000.0, NOW); // old document, deep scroll

        // Scroll to the top of the NEW document: instant, adopted now.
        assert!(core.scroll_to_offset(0.0, ScrollMode::Instant, &surface).is_some());
        assert_eq!(core.scroll_top(), 0.0);

        // The count rebuild that follows anchors at the adopted 0.
        let estimate = |_index: usize| 100.0;
        let step = core.set_count(20, &estimate);
        assert!(step.layout_changed);
        assert_eq!(core.scroll_top(), 0.0);
        assert_eq!(step.scroll_write, Some(0.0));
    }

    #[test]
    fn pending_scroll_retargets_when_offscreen_sizes_move() {
        let surface = TestSurface::default();
        let mut core = list_core(100, 100.0, 200.0);
        let _ = core.on_scroll(1_000.0, NOW);
        let _ = core.scroll_to_index(50, Align::Start, ScrollMode::Instant, &surface);
        for index in 20..30 {
            core.queue_size(index, 150.0);
        }
        let flush = core.flush().expect("flush");
        assert_eq!(flush.step.scroll_write, Some(5_500.0));
        let _ = core.on_scroll(5_500.0, NOW);
        core.queue_size(60, 150.0);
        let flush = core.flush().expect("flush");
        assert_eq!(flush.step.scroll_write, None);
        assert!(core.pending().is_none());
    }

    #[test]
    fn pending_scroll_exhausts_retries() {
        let surface = TestSurface::default();
        let mut core = list_core(100, 100.0, 200.0);
        let _ = core.on_scroll(1_000.0, NOW);
        // Smooth scroll-to: NOT adopted locally — the browser echoes it, and
        // until it does the core still works from the old position. That is
        // the window the bounded re-aim protects: measurements keep moving
        // the target before the echo lands.
        assert!(core.scroll_to_index(50, Align::Start, ScrollMode::Smooth, &surface).is_none());

        let mut retries = 0;
        for round in 0..5 {
            core.queue_size(20, 100.0 + (round + 1) as f64);
            if let Some(flush) = core.flush()
                && flush.step.scroll_write.is_some()
            {
                retries += 1;
            }
        }

        assert_eq!(retries, 3);
        assert!(core.pending().is_none());
    }

    #[test]
    fn suspend_buffers_measurements_until_resume() {
        let mut core = list_core(100, 100.0, 200.0);
        let _ = core.on_scroll(0.0, NOW);
        core.suspend();
        core.queue_size(5, 300.0);
        assert!(core.flush().is_none());
        core.queue_size(6, 300.0);
        let flush = core.resume().expect("resume flushes the backlog");
        assert_eq!(flush.applied, 2);
        assert!(flush.step.layout_changed);
    }

    #[test]
    fn flush_clamps_scroll_when_content_shrinks() {
        let mut core = list_core(20, 100.0, 200.0);
        let _ = core.on_scroll(1_800.0, NOW);
        for index in 0..6 {
            core.queue_size(index, 10.0);
        }
        let flush = core.flush().expect("flush");
        assert_eq!(flush.step.scroll_write, Some(1_260.0));
    }

    #[test]
    fn viewport_jitter_within_epsilon_is_ignored() {
        let mut core = list_core(100, 100.0, 200.0);
        let _ = core.on_scroll(0.0, NOW);
        let step = core.on_viewport(Viewport::new(200.3, 0.0));
        assert!(!step.layout_changed);
        assert_eq!(step.range, core.range());
    }

    #[test]
    fn scroll_jitter_within_epsilon_is_ignored() {
        let mut core = list_core(100, 100.0, 200.0);
        let _ = core.on_scroll(1_000.0, NOW);
        let before = core.scroll_top();
        // Half an epsilon: no window recompute, no position adoption.
        let step = core.on_scroll(1_000.2, NOW);
        assert!(!step.layout_changed);
        assert_eq!(step.range, core.range());
        assert_eq!(core.scroll_top(), before);
        // Past epsilon: adopted normally.
        let _ = core.on_scroll(1_001.0, NOW);
        assert!((core.scroll_top() - 1_001.0).abs() < 1e-9);
    }

    #[test]
    fn responsive_grid_re_resolves_columns_on_width_change() {
        let spec = GridSpec::responsive(120.0, 12.0);
        let mut core = grid_core(
            100,
            150.0,
            spec,
            Viewport::new(720.0, 264.0),
            Budget::items(2, 100),
        );
        assert_eq!(core.columns(), Some(2));
        assert_eq!(core.layout().total(), 50.0 * 150.0);
        let step = core.on_viewport(Viewport::new(720.0, 552.0));
        assert!(step.layout_changed);
        assert_eq!(core.columns(), Some(4));
        assert_eq!(core.layout().total(), 25.0 * 150.0);
    }

    #[test]
    fn grid_mounts_more_rows_in_a_taller_viewport() {
        let spec = GridSpec::fixed(2, 8.0);
        let budget = Budget::items(1, 1_000);
        let mut small = grid_core(200, 120.0, spec, Viewport::new(720.0, 264.0), budget);
        let mut tall = grid_core(200, 120.0, spec, Viewport::new(1_440.0, 264.0), budget);
        let small_window = small.on_scroll(0.0, NOW).range.expect("window");
        let tall_window = tall.on_scroll(0.0, NOW).range.expect("window");
        assert_eq!(small_window.len(), 14);
        assert_eq!(tall_window.len(), 26);
    }

    #[test]
    fn rows_are_the_grid_render_unit() {
        let mut core = grid_core(
            5,
            100.0,
            GridSpec::fixed(2, 8.0),
            Viewport::new(500.0, 264.0),
            Budget::items(0, 100),
        );
        let _ = core.on_scroll(0.0, NOW);
        let rows = core.rows();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].items, 0..2);
        assert_eq!(rows[2].items, 4..5);
        assert_eq!(rows[1].start, 100.0);
    }

    #[test]
    fn rescale_keeps_the_reader_on_their_item() {
        let mut core = list_core(50, 100.0, 200.0);
        let _ = core.on_scroll(2_400.0, NOW);
        let step = core.rescale(2.0, &|_index| 200.0);
        assert!(step.layout_changed);
        assert_eq!(step.scroll_write, Some(4_900.0));
        assert_eq!(core.dominant(), 24);
    }

    #[test]
    fn rescale_keeps_the_viewport_center_stable() {
        // The zoom contract from the reader's side: whatever content point
        // sits at the viewport CENTER before a rescale must still sit at the
        // center after. A top-anchored rescale lets the focal point walk —
        // the page slides under the reader while it scales — and a sidebar
        // slide rescales per frame, so per-frame drift compounds into the
        // visible mid-slide misalignment. This pins the invariant.
        let vh = 500.0;
        let scroll = 10_000.0;
        let mut core = list_core(200, 100.0, vh);
        let _ = core.on_scroll(scroll, NOW);

        // The content point at the viewport center, by hand for this
        // uniform gapless list: item 102, 50px into it.
        let center = scroll + vh / 2.0;
        let item = (center / 100.0) as usize;
        let px = center - item as f64 * 100.0;
        assert_eq!((item, px), (102, 50.0));

        let before = core.dominant();
        let step = core.rescale(0.8, &|_index| 80.0);
        assert!(step.layout_changed);

        // The same item still dominates, and the anchored content point is
        // still at the viewport center.
        assert_eq!(core.dominant(), before);
        let anchored = core.offset_of(item) + px * 0.8;
        assert!(
            (anchored - core.scroll_top() - vh / 2.0).abs() < 1e-9,
            "viewport center drifted: anchored {} vs scroll {} + {}",
            anchored,
            core.scroll_top(),
            vh / 2.0
        );
    }

    #[test]
    fn rebuild_re_pitches_the_grid_without_moving_the_reader() {
        let mut core = grid_core(
            100,
            120.0,
            GridSpec::fixed(2, 8.0),
            Viewport::new(600.0, 252.0),
            Budget::items(1, 100),
        );
        let _ = core.on_scroll(1_200.0, NOW);
        let dominant = core.dominant();
        let step = core.rebuild(&|_index| 200.0);
        assert!(step.layout_changed);
        assert_eq!(core.dominant(), dominant);
    }

    #[test]
    fn scroll_targets_can_enter_the_start_padding_band() {
        let mut core = VirtualizerCore::new(
            LayoutKind::List(ListLayout::uniform(10, 100.0, 0.0)),
            CoreConfig {
                padding_start: 12.0,
                viewport: Viewport::main_only(300.0),
                ..CoreConfig::default()
            },
        );
        let surface = TestSurface::default();
        assert!(core.scroll_to_offset(-12.0, ScrollMode::Instant, &surface).is_some());
        assert_eq!(core.scroll_top(), -12.0);
        assert_eq!(surface.writes(), vec![(-12.0, false)]);
        assert!(core.scroll_to_index(0, Align::Center, ScrollMode::Instant, &surface).is_some());
        assert!(surface.writes()[1].0 < 0.0);
        assert!(core.scroll_to_offset(-500.0, ScrollMode::Instant, &surface).is_some());
        assert_eq!(surface.writes()[2].0, -12.0);
        assert_eq!(core.scroll_top(), -12.0);
    }

    fn stream_core(render_screens: f64) -> VirtualizerCore {
        VirtualizerCore::new(
            LayoutKind::List(ListLayout::uniform(200, 100.0, 0.0)),
            CoreConfig {
                // A mount budget WIDER than the band: two screens of overscan
                // each way, so the band has something to blank.
                budget: Budget::screenfuls(2.0, 1_000),
                viewport: Viewport::main_only(200.0),
                render_screens,
                ..CoreConfig::default()
            },
        )
    }

    #[test]
    fn a_render_band_blanks_the_mount_fringes_but_never_the_viewport() {
        let mut core = stream_core(0.75);
        let _ = core.on_scroll(2_000.0, NOW);
        // Mount: visible rows 20-21 plus two screens of overscan -> 16..=25.
        // Band: viewport padded three quarters of a screen -> rows 18..=23.
        let items = core.items();
        assert_eq!(
            items.iter().map(|item| item.index).collect::<Vec<_>>(),
            (16..=25).collect::<Vec<_>>()
        );
        for item in &items {
            let want = if (18..=23).contains(&item.index) {
                VirtualItemState::Active
            } else {
                VirtualItemState::Blank
            };
            assert_eq!(item.state, want, "item {}", item.index);
            // A placeholder keeps the layout's own size: the scrollbar and
            // the anchors must not see the band at all.
            assert_eq!(item.size, 100.0);
        }
        // The rows under the reader's eyes are never blanks.
        assert_eq!(core.item_state(20), VirtualItemState::Active);
        assert_eq!(core.item_state(21), VirtualItemState::Active);
    }

    #[test]
    fn without_a_band_everything_mounted_is_active() {
        let mut core = stream_core(0.0);
        let _ = core.on_scroll(2_000.0, NOW);
        let items = core.items();
        assert!(!items.is_empty());
        assert!(items.iter().all(|item| item.state == VirtualItemState::Active));
        assert_eq!(core.render_range(), core.range());
    }

    #[test]
    fn the_band_never_changes_what_the_window_mounts_or_totals() {
        let mut banded = stream_core(0.75);
        let mut plain = stream_core(0.0);
        let mut top = 0.0;
        while top < 19_000.0 {
            let _ = banded.on_scroll(top, NOW);
            let _ = plain.on_scroll(top, NOW);
            assert_eq!(banded.range(), plain.range(), "mount window at {top}");
            assert_eq!(banded.total_size(), plain.total_size(), "extent at {top}");
            top += 317.0;
        }
    }
}

#[cfg(test)]
mod motion_tests {
    //! The adaptive half of the engine: what a movement does to the windows.
    //!
    //! The geometry here is deliberately blunt — 200px items in an 800px
    //! viewport, so one screen is four items and a policy written in screens
    //! can be asserted in whole items. The motion is fed frame by frame at
    //! 16ms, which is what the adapter's rAF coalescing actually delivers, and
    //! the mount ceiling is wide enough that the tiers, not the trim, are what
    //! these tests measure.

    use super::*;
    use crate::surface::TestSurface;

    const VH: f64 = 800.0;
    const ITEM: f64 = 200.0;
    /// Ten screens of items: past what any tier reaches, so a window asserted
    /// below is the policy's answer rather than the ceiling's.
    const ROOM: usize = 40;

    /// A clock that hands out one 16ms frame per call.
    struct Clock(f64);

    impl Clock {
        fn frame(&mut self) -> f64 {
            self.0 += 16.0;
            self.0
        }
    }

    /// A core already sitting at `offset`, with its motion model primed from
    /// that position — exactly as the adapter primes it from the resume offset.
    /// A test that jumped there with a scroll sample instead would be measuring
    /// the jump: one frame from zero to eight thousand pixels reads as a fling
    /// of half a million pixels per second.
    fn adaptive_core_at(items: usize, offset: f64) -> VirtualizerCore {
        VirtualizerCore::new(
            LayoutKind::List(ListLayout::uniform(items, ITEM, 0.0)),
            CoreConfig {
                budget: Budget::screenfuls(0.0, ROOM),
                viewport: Viewport::main_only(VH),
                initial_offset: offset,
                adaptive: Some(AdaptivePolicy::reader()),
                ..CoreConfig::default()
            },
        )
    }

    /// Scroll at `speed` px/s for `frames` frames, starting where the core is.
    /// Clamped to the document, because the browser clamps: a test that
    /// scrolled past the end would be measuring a position no reader can hold.
    fn scroll(core: &mut VirtualizerCore, clock: &mut Clock, speed: f64, frames: usize) {
        let mut top = core.scroll_top();
        for _ in 0..frames {
            top = (top + speed * 0.016).clamp(0.0, core.max_scroll());
            let _ = core.on_scroll(top, clock.frame());
        }
    }

    #[test]
    fn a_fast_scroll_predicts_the_item_the_reader_is_landing_on() {
        let mut core = adaptive_core_at(200, 8_000.0);
        let mut clock = Clock(0.0);
        // Ten screens a second: a throw.
        scroll(&mut core, &mut clock, 8_000.0, 20);

        let plan = core.render_plan();
        assert_eq!(plan.phase, ScrollPhase::Fling);
        assert_eq!(plan.direction, 1);
        // The projection is a distance ahead of the viewport, so the item it
        // names is ahead of the one under the reader's eyes.
        assert!(
            plan.predicted_index > core.dominant(),
            "predicted {} from dominant {}",
            plan.predicted_index,
            core.dominant()
        );
        assert!(plan.predicted_offset > plan.scroll_top);
        // …and it is a real item, resolved through the layout rather than
        // counted off the dominant one.
        assert_eq!(plan.predicted_index, core.index_at(plan.predicted_offset));
        assert!(plan.is_sweeping());
        assert_eq!(plan.delay_ms, 90);
        assert_eq!(plan.workers, 1);
    }

    #[test]
    fn reversing_switches_the_prediction_and_the_lean() {
        let mut core = adaptive_core_at(200, 20_000.0);
        let mut clock = Clock(0.0);
        scroll(&mut core, &mut clock, -6_000.0, 20);

        let plan = core.render_plan();
        assert_eq!(plan.direction, -1);
        assert!(plan.predicted_index < core.dominant());
        let full = plan.full.expect("a full tier");
        let visible = plan.visible.expect("a visible range");
        // Travelling up, the lead is above: more items between the top of the
        // full tier and the top of the viewport than below its bottom.
        assert!(
            visible.first - full.first > full.last - visible.last,
            "full {full:?} around visible {visible:?}"
        );
    }

    #[test]
    fn the_full_tier_leans_in_the_direction_of_travel() {
        let mut core = adaptive_core_at(200, 8_000.0);
        let mut clock = Clock(0.0);
        scroll(&mut core, &mut clock, 4_000.0, 20);

        let plan = core.render_plan();
        assert_eq!(plan.direction, 1);
        let full = plan.full.expect("a full tier");
        let visible = plan.visible.expect("a visible range");
        // 0.75 screens ahead is three items; the trailing side gets the
        // policy's 0.4 of that, which is one item of the 240px.
        assert_eq!(full.last - visible.last, 3);
        assert_eq!(visible.first - full.first, 1);
    }

    #[test]
    fn a_settled_reader_gets_a_symmetric_tier() {
        // No sample at all: a reader who opened the book here and has not moved.
        let core = adaptive_core_at(200, 8_000.0);
        let plan = core.render_plan();
        assert_eq!(plan.phase, ScrollPhase::Idle);
        let full = plan.full.expect("a full tier");
        let visible = plan.visible.expect("a visible range");
        assert_eq!(visible.first - full.first, full.last - visible.last);
        assert_eq!(full.last - visible.last, 3);
    }

    #[test]
    fn the_tiers_nest_and_the_visible_items_are_always_full() {
        let mut core = adaptive_core_at(200, 8_000.0);
        let mut clock = Clock(0.0);
        for speed in [1_500.0, 4_000.0, 9_000.0, 20_000.0] {
            scroll(&mut core, &mut clock, speed, 8);
            let plan = core.render_plan();
            let (Some(mount), Some(preview), Some(full), Some(visible)) =
                (plan.mount, plan.preview, plan.full, plan.visible)
            else {
                panic!("every window exists mid-document");
            };
            assert!(
                mount.first <= full.first && mount.last >= full.last,
                "{mount:?} over {full:?}"
            );
            assert!(
                full.first >= preview.first && full.last <= preview.last,
                "{full:?} inside {preview:?}"
            );
            assert!(preview.first <= visible.first && preview.last >= visible.last);
            // Whatever the movement, the items on screen are full quality, and
            // the item states are the plan's own answer — one truth, read two
            // ways, because the view layer consumes both.
            for index in visible.iter() {
                assert_eq!(plan.quality(index), RenderQuality::Full, "item {index}");
                assert_eq!(core.item_state(index), VirtualItemState::Active);
            }
            for item in core.items() {
                assert_eq!(item.state, core.item_state(item.index));
                match item.state {
                    VirtualItemState::Active => {
                        assert_eq!(plan.quality(item.index), RenderQuality::Full)
                    }
                    VirtualItemState::Preview => {
                        assert_eq!(plan.quality(item.index), RenderQuality::Preview)
                    }
                    VirtualItemState::Blank => {
                        assert_eq!(plan.quality(item.index), RenderQuality::Placeholder)
                    }
                    VirtualItemState::Zombie => panic!("the core retains nothing"),
                }
            }
        }
    }

    #[test]
    fn a_fling_mounts_placeholders_between_the_reader_and_the_destination() {
        let mut core = adaptive_core_at(200, 4_000.0);
        let mut clock = Clock(0.0);
        scroll(&mut core, &mut clock, 12_000.0, 20);

        let plan = core.render_plan();
        let mount = plan.mount.expect("a mount window");
        let full = plan.full.expect("a full tier");
        // The window reaches further ahead than the tier does, so there are
        // mounted items owed nothing but their geometry — the boxes a fling
        // slides past instead of rasters it throws away.
        assert!(mount.last > full.last, "{mount:?} over {full:?}");
        let placeholders = (full.last + 1..=mount.last)
            .filter(|index| core.item_state(*index) == VirtualItemState::Blank)
            .count();
        assert!(placeholders > 0, "nothing past the tier was a placeholder");
        // And the destination the plan named is inside the window: the point of
        // predicting it is that it can be ready before the reader arrives.
        assert!(mount.contains(plan.predicted_index));
    }

    #[test]
    fn the_settle_promotes_the_ring_behind_the_reader() {
        let mut core = adaptive_core_at(200, 8_000.0);
        let mut clock = Clock(0.0);
        scroll(&mut core, &mut clock, 6_000.0, 20);
        let moving = core.render_plan();
        assert!(moving.phase.at_least(ScrollPhase::Fast));
        // A page above the viewport that a downward scroll left in the ring.
        let above = moving.full.expect("a full tier").first - 1;
        assert_eq!(moving.quality(above), RenderQuality::Preview);

        let _ = core.settle(clock.frame());
        let settled = core.render_plan();
        assert_eq!(settled.phase, ScrollPhase::Idle);
        assert_eq!(settled.direction, 0);
        // Settling is the promotion: the trailing side gets the full 0.75
        // screens instead of 0.4 of it, so the page just read is crisp rather
        // than soft if the reader scrolls back up into it.
        assert_eq!(settled.quality(above), RenderQuality::Full);
        assert!(settled.full.expect("a tier").first < moving.full.expect("a tier").first);
    }

    #[test]
    fn a_commanded_scroll_is_not_read_as_a_fling() {
        let mut core = adaptive_core_at(200, 2_000.0);
        let mut clock = Clock(0.0);
        scroll(&mut core, &mut clock, 6_000.0, 16);
        assert!(core.render_plan().phase.at_least(ScrollPhase::Fast));

        // A page turn: the surface is written and the browser animates to it,
        // echoing a burst of scroll events that cover the whole distance.
        let surface = TestSurface::default();
        assert!(core
            .scroll_to_index(120, Align::Start, ScrollMode::Smooth, &surface)
            .is_none());
        let target = surface.writes().last().expect("a write").0;
        let _ = core.on_scroll(target, clock.frame());

        let plan = core.render_plan();
        assert_eq!(plan.phase, ScrollPhase::Idle, "a page turn looked like a fling");
        assert_eq!(plan.velocity, 0.0);
        assert_eq!(plan.delay_ms, 0);
        assert_eq!(plan.workers, 2);
        // The prediction is the reader's own position, not a projection of a
        // jump they did not make.
        assert_eq!(plan.predicted_index, core.dominant());
        assert_eq!(plan.predicted_offset, core.scroll_top());

        // The scroll-end window lifts the mute, and real motion measures again.
        let _ = core.settle(clock.frame());
        scroll(&mut core, &mut clock, 6_000.0, 16);
        assert!(core.render_plan().phase.at_least(ScrollPhase::Fast));
    }

    #[test]
    fn prediction_is_clamped_to_the_document() {
        // Near the end, throwing downwards: the projection cannot run off the
        // last page, or the scheduler would be handed an item that does not
        // exist and a window around it that cannot mount.
        let mut core = adaptive_core_at(60, 8_000.0);
        let mut clock = Clock(0.0);
        scroll(&mut core, &mut clock, 20_000.0, 20);
        assert!((core.scroll_top() - core.max_scroll()).abs() < 1e-9, "the end");
        let plan = core.render_plan();
        assert!(plan.predicted_offset <= core.max_scroll() + 1e-9);
        assert!(plan.predicted_index < core.item_count());
        assert_eq!(plan.predicted_index, core.index_at(plan.predicted_offset));

        // And the same at the top, throwing upwards.
        let mut core = adaptive_core_at(60, 4_000.0);
        let mut clock = Clock(0.0);
        scroll(&mut core, &mut clock, -20_000.0, 20);
        assert!(core.scroll_top() <= 1e-9, "the start");
        let plan = core.render_plan();
        assert!(plan.predicted_offset >= 0.0);
        assert_eq!(plan.predicted_index, core.index_at(plan.predicted_offset));
    }

    #[test]
    fn prediction_is_a_distance_so_a_fold_out_is_not_skipped() {
        // Ten 800px pages, an 8000px fold-out, ten more. A reader crossing the
        // boundary at reading speed is predicted INTO the plate; counting two
        // pages ahead would have named the page after it — 8000px further on
        // than anything the reader is about to reach.
        let mut sizes = vec![800.0; 10];
        sizes.push(8_000.0);
        sizes.extend(core::iter::repeat_n(800.0, 10));
        let mut core = VirtualizerCore::new(
            LayoutKind::List(ListLayout::new(sizes, 0.0)),
            CoreConfig {
                budget: Budget::screenfuls(0.0, ROOM),
                viewport: Viewport::main_only(VH),
                initial_offset: 7_700.0,
                adaptive: Some(AdaptivePolicy::reader()),
                ..CoreConfig::default()
            },
        );
        let mut clock = Clock(0.0);
        let mut top = 7_700.0;
        // One screen a second: a reading speed, not a scroll.
        for _ in 0..20 {
            top += 12.8;
            let _ = core.on_scroll(top, clock.frame());
        }
        let plan = core.render_plan();
        assert!((plan.velocity - 800.0).abs() < 150.0, "velocity {}", plan.velocity);
        let here = core.index_at(plan.scroll_top);
        assert_eq!(here, 9, "the reader is still on the last small page");
        assert_eq!(plan.predicted_index, 10, "the projection lands inside the plate");
        assert_ne!(plan.predicted_index, here + 2);
        assert_eq!(plan.predicted_index, core.index_at(plan.predicted_offset));
    }

    #[test]
    fn a_motion_blind_core_never_reports_motion() {
        // The regression guard for the whole feature: a surface that did not
        // ask for a policy (the thumbnail grid, the text stream) behaves
        // exactly as it did before the motion model existed, however fast it is
        // scrolled, and however often.
        // Two items to a screen, so the budget's three-item ceiling is a
        // ceiling the visible range fits under and the trim actually bites.
        let mut core = VirtualizerCore::new(
            LayoutKind::List(ListLayout::uniform(200, ITEM * 2.0, 0.0)),
            CoreConfig {
                budget: Budget::screenfuls(0.5, 3),
                viewport: Viewport::main_only(VH),
                initial_offset: 4_000.0,
                ..CoreConfig::default()
            },
        );
        let mut clock = Clock(0.0);
        scroll(&mut core, &mut clock, 20_000.0, 30);
        let plan = core.render_plan();
        assert_eq!(plan.phase, ScrollPhase::Idle);
        assert_eq!(plan.velocity, 0.0);
        assert_eq!(plan.delay_ms, 0);
        assert_eq!(plan.grace_ms, 0);
        assert_eq!(plan.full, plan.mount);
        assert_eq!(plan.preview, plan.mount);
        assert!(core
            .items()
            .iter()
            .all(|item| item.state == VirtualItemState::Active));
        // The window is still the budget's, symmetric and capped at three.
        assert!(plan.mount.expect("a window").len() <= 3);
        assert_eq!(core.render_range(), core.range());
        assert_eq!(core.preview_range(), core.range());
    }

    #[test]
    fn the_tiers_never_move_the_geometry() {
        // What a tier says about an item is what it is OWED, never where it
        // sits or how big it is: the scrollbar, the anchors and the offsets all
        // read the layout, and a placeholder that reported a different size
        // would move every page below it.
        let mut core = adaptive_core_at(200, 0.0);
        let mut clock = Clock(0.0);
        let mut top = 0.0;
        while top < 30_000.0 {
            top += 317.0;
            let _ = core.on_scroll(top, clock.frame());
            for item in core.items() {
                assert_eq!(item.size, ITEM, "item {}", item.index);
                assert_eq!(item.start, core.offset_of(item.index), "item {}", item.index);
            }
            assert_eq!(core.render_plan().scroll_top, core.scroll_top());
        }
    }

    #[test]
    fn the_mount_ceiling_still_binds_under_a_policy() {
        // The policy decides how far the window reaches; the budget decides how
        // much of that is allowed to exist. A fling may look four screens
        // ahead, but it may not mount the document to do it.
        // Six items, against four screens' worth of look-ahead the fling below
        // would otherwise mount: the ceiling is the binding constraint, and by
        // more than the visible range so the assertion is about the ceiling.
        let mut core = VirtualizerCore::new(
            LayoutKind::List(ListLayout::uniform(200, ITEM, 0.0)),
            CoreConfig {
                budget: Budget::screenfuls(0.0, 6),
                viewport: Viewport::main_only(VH),
                initial_offset: 8_000.0,
                adaptive: Some(AdaptivePolicy::reader()),
                ..CoreConfig::default()
            },
        );
        let mut clock = Clock(0.0);
        scroll(&mut core, &mut clock, 12_000.0, 20);
        let plan = core.render_plan();
        let mount = plan.mount.expect("a window");
        assert!(mount.len() <= 6, "{mount:?}");
        // …and the reader's own items are never what the ceiling trimmed.
        let visible = plan.visible.expect("a visible range");
        assert!(mount.first <= visible.first && mount.last >= visible.last);
    }
}
