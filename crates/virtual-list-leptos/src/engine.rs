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
    Align, AnchorPolicy, Budget, GridLayout, Layout, LayoutKind, ListLayout, Viewport, Window,
    correct, pin_at, rescale_anchor,
};

use crate::options::{LayoutShape, ScrollMode};
use crate::render::{VirtualItem, VirtualRow};
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

/// The engine.
pub struct VirtualizerCore {
    layout: LayoutKind,
    budget: Budget,
    shape: LayoutShape,
    gap: f64,
    padding_start: f64,
    padding_end: f64,
    eps: f64,
    max_retries: u32,

    hint: usize,
    scroll_top: f64,
    viewport: Viewport,
    range: Option<Window>,
    pinned: Option<(usize, usize)>,
    pending: Option<PendingScroll>,
    queue: Vec<(usize, f64)>,
    suspended: bool,
}

impl VirtualizerCore {
    /// Build an engine around an initial layout.
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
            hint: 0,
            scroll_top: config.initial_offset,
            viewport: config.viewport,
            range: None,
            pinned: None,
            pending: None,
            queue: Vec::new(),
            suspended: false,
        };
        this.scroll_top = this.scroll_top.clamp(this.min_scroll(), this.max_scroll());
        this.range = this.rewindow().range;
        this
    }

    /// The container scrolled. `content_top` is in content coordinates.
    ///
    /// Sub-epsilon deltas (a scroll event that moved less than `eps` — the
    /// browser fires scroll events for fractional-pixel wheel deltas) are
    /// ignored wholesale: they cannot change the window, and adopting them
    /// would wake every `scroll_top` consumer (dominant-page tracking,
    /// navigation sync) for a movement the display cannot even show.
    pub fn on_scroll(&mut self, content_top: f64) -> Step {
        if (content_top - self.scroll_top).abs() <= self.eps {
            return Step {
                range: self.range,
                scroll_write: None,
                layout_changed: false,
            };
        }
        self.scroll_top = content_top;
        self.rewindow()
    }

    /// The container resized.
    pub fn on_viewport(&mut self, vp: Viewport) -> Step {
        let current = self.viewport;
        let main_changed = (vp.main - current.main).abs() > self.eps;
        let cross_changed = (vp.cross - current.cross).abs() > self.eps;
        if !main_changed && !cross_changed {
            return Step {
                range: self.range,
                scroll_write: None,
                layout_changed: false,
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

    /// Extra indices that must stay mounted.
    pub fn set_pinned(&mut self, pinned: Option<(usize, usize)>) -> Step {
        self.pinned = pinned;
        self.rewindow()
    }

    /// The item count changed.
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

    /// Queue a measured size.
    pub fn queue_size(&mut self, index: usize, size: f64) {
        self.queue.push((index, size.max(0.0)));
    }

    /// Stop flushing.
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

    /// Scroll to an item with an alignment.
    ///
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

    /// Current mount window.
    pub fn range(&self) -> Option<Window> {
        self.range
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
    pub fn min_scroll(&self) -> f64 {
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
    pub fn items(&self) -> Vec<VirtualItem> {
        let Some(window) = self.range else {
            return Vec::new();
        };
        (window.first..=window.last).map(|index| self.item_at(index)).collect()
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
            let base =
                self.layout
                    .window_hinted(self.scroll_top, self.viewport, self.budget, &mut hint);
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
        Step {
            range,
            scroll_write: None,
            layout_changed: false,
        }
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

/// Build the right layout kind from the shape.
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
            core.on_scroll(0.0).range,
            Some(Window { first: 0, last: 1 })
        );
        assert_eq!(
            core.on_scroll(1_000.0).range,
            Some(Window {
                first: 10,
                last: 11
            })
        );
    }

    #[test]
    fn measurement_above_the_viewport_shifts_scroll() {
        let mut core = list_core(100, 100.0, 200.0);
        let _ = core.on_scroll(5_000.0);
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
        let _ = core.on_scroll(5_000.0);
        core.queue_size(90, 200.0);
        let flush = core.flush().expect("flush");
        assert_eq!(flush.step.scroll_write, None);
    }

    #[test]
    fn subpixel_measurements_are_filtered() {
        let mut core = list_core(10, 100.0, 200.0);
        let _ = core.on_scroll(0.0);
        core.queue_size(5, 100.3);
        let flush = core.flush().expect("flush");
        assert_eq!(flush.applied, 0);
        assert!(!flush.step.layout_changed);
    }

    #[test]
    fn last_measurement_wins_per_index() {
        let mut core = list_core(10, 100.0, 200.0);
        let _ = core.on_scroll(0.0);
        core.queue_size(5, 300.0);
        core.queue_size(5, 400.0);
        let flush = core.flush().expect("flush");
        assert_eq!(flush.applied, 1);
        assert_eq!(core.layout().size(5), 400.0);
    }

    #[test]
    fn count_shrink_clamps_and_reanchors() {
        let mut core = list_core(100, 100.0, 200.0);
        let _ = core.on_scroll(9_000.0);
        let estimate = |_index: usize| 100.0;
        let step = core.set_count(20, &estimate);
        assert!(step.layout_changed);
        assert_eq!(core.scroll_top(), 1_800.0);
        assert_eq!(step.scroll_write, Some(1_800.0));
    }

    #[test]
    fn pinned_indices_extend_the_window() {
        let mut core = list_core(100, 100.0, 200.0);
        let _ = core.on_scroll(0.0);
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

        let _ = core.on_scroll(5_000.0);
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
        let _ = core.on_scroll(5_000.0); // old document, deep scroll

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
        let _ = core.on_scroll(1_000.0);
        let _ = core.scroll_to_index(50, Align::Start, ScrollMode::Instant, &surface);
        for index in 20..30 {
            core.queue_size(index, 150.0);
        }
        let flush = core.flush().expect("flush");
        assert_eq!(flush.step.scroll_write, Some(5_500.0));
        let _ = core.on_scroll(5_500.0);
        core.queue_size(60, 150.0);
        let flush = core.flush().expect("flush");
        assert_eq!(flush.step.scroll_write, None);
        assert!(core.pending().is_none());
    }

    #[test]
    fn pending_scroll_exhausts_retries() {
        let surface = TestSurface::default();
        let mut core = list_core(100, 100.0, 200.0);
        let _ = core.on_scroll(1_000.0);
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
        let _ = core.on_scroll(0.0);
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
        let _ = core.on_scroll(1_800.0);
        for index in 0..6 {
            core.queue_size(index, 10.0);
        }
        let flush = core.flush().expect("flush");
        assert_eq!(flush.step.scroll_write, Some(1_260.0));
    }

    #[test]
    fn viewport_jitter_within_epsilon_is_ignored() {
        let mut core = list_core(100, 100.0, 200.0);
        let _ = core.on_scroll(0.0);
        let step = core.on_viewport(Viewport::new(200.3, 0.0));
        assert!(!step.layout_changed);
        assert_eq!(step.range, core.range());
    }

    #[test]
    fn scroll_jitter_within_epsilon_is_ignored() {
        let mut core = list_core(100, 100.0, 200.0);
        let _ = core.on_scroll(1_000.0);
        let before = core.scroll_top();
        // Half an epsilon: no window recompute, no position adoption.
        let step = core.on_scroll(1_000.2);
        assert!(!step.layout_changed);
        assert_eq!(step.range, core.range());
        assert_eq!(core.scroll_top(), before);
        // Past epsilon: adopted normally.
        let _ = core.on_scroll(1_001.0);
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
        let small_window = small.on_scroll(0.0).range.expect("window");
        let tall_window = tall.on_scroll(0.0).range.expect("window");
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
        let _ = core.on_scroll(0.0);
        let rows = core.rows();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].items, 0..2);
        assert_eq!(rows[2].items, 4..5);
        assert_eq!(rows[1].start, 100.0);
    }

    #[test]
    fn rescale_keeps_the_reader_on_their_item() {
        let mut core = list_core(50, 100.0, 200.0);
        let _ = core.on_scroll(2_400.0);
        let step = core.rescale(2.0, &|_index| 200.0);
        assert!(step.layout_changed);
        assert_eq!(step.scroll_write, Some(4_900.0));
        assert_eq!(core.dominant(), 24);
    }

    #[test]
    fn rescale_keeps_the_viewport_center_stable() {
        // The zoom contract from the reader's side: whatever content point
        // sits at the viewport CENTER before a rescale must still sit at the
        // center afterwards. A top-anchored rescale would hold the top pixel
        // fixed instead and let the focal point walk — which reads as the
        // page sliding under the reader while it scales. A sidebar slide
        // rescales per frame, so any per-frame drift compounds into the
        // visible mid-slide misalignment; this pins the invariant that
        // prevents it.
        let vh = 500.0;
        let scroll = 10_000.0;
        let mut core = list_core(200, 100.0, vh);
        let _ = core.on_scroll(scroll);

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
        let _ = core.on_scroll(1_200.0);
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
}
