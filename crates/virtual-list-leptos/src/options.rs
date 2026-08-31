//! Adapter options: what the consumer configures once per virtualizer.

use std::rc::Rc;

use leptos::prelude::*;
use virtual_list::{Budget, GridSpec, Viewport};

/// Which scroll axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Axis {
    /// Top-to-bottom scrolling (the default).
    #[default]
    Vertical,
    /// Left-to-right scrolling.
    Horizontal,
}

/// Layout shape. For [`Grid`](Self::Grid), `estimate_size` returns the
/// uniform **row pitch** (cell height + gap below the row).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LayoutShape {
    /// A single column of variably-sized items.
    List,
    /// A uniform multi-column grid, windowed per row. Column count comes from
    /// the spec (fixed, or responsive to the container width).
    Grid(GridSpec),
}

/// Scroll behavior for `scroll_to_*` commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollMode {
    /// Jump immediately.
    Instant,
    /// Browser-animated.
    Smooth,
    /// Smooth when the target is within two viewports, instant beyond — the
    /// glide heuristic: near page-turns animate, far jumps snap.
    #[default]
    Auto,
}

/// Everything [`use_virtualizer`](crate::use_virtualizer) needs. Build with
/// [`VirtualizerOptions::list`] / [`VirtualizerOptions::grid`], then chain
/// setters.
pub struct VirtualizerOptions {
    /// Reactive item count. Changes rebuild the layout and re-anchor the
    /// dominant item.
    pub count: Signal<usize>,
    /// Per-item estimated size. MUST return each item's OWN estimate — never
    /// one global fallback. For grids this returns the uniform row pitch.
    pub estimate_size: Rc<dyn Fn(usize) -> f64>,
    /// List or grid.
    pub shape: LayoutShape,
    /// Gap between list items (grids fold the gap into the row pitch).
    pub gap: f64,
    /// Mount budget: overscan + hard ceiling.
    pub budget: Budget,
    /// Scroll axis.
    pub axis: Axis,
    /// Content padding before the first item.
    pub padding_start: f64,
    /// Content padding after the last item.
    pub padding_end: f64,
    /// Bump to force a layout rebuild when geometry changes without a count
    /// change (a new row pitch, a font swap).
    pub epoch: Option<Signal<u64>>,
    /// Reactive extra indices that must stay mounted.
    pub pinned: Option<Signal<Option<(usize, usize)>>>,
    /// Viewport used before the first ResizeObserver report.
    pub initial_viewport: Viewport,
    /// Initial scroll position (content coordinates).
    pub initial_offset: f64,
    /// Scroll-idle debounce, milliseconds. After this much quiet, a scroll
    /// burst is considered finished.
    pub scroll_end_delay_ms: u32,
    /// Grace period an evicted item stays rendered after a window change,
    /// milliseconds. `0` disables zombie retention (the default: items
    /// unmount the moment they leave the window).
    pub retention_grace_ms: u32,
    /// Hard ceiling on simultaneously retained (zombie) items.
    pub retention_max: usize,
    /// Change-detection epsilon for measurements and viewport writes.
    pub measure_epsilon: f64,
    /// Max re-aims for an in-flight `scroll_to_index`.
    pub max_scroll_retries: u32,
}

impl VirtualizerOptions {
    /// A single-column virtualizer.
    pub fn list(
        count: impl Into<Signal<usize>>,
        estimate_size: impl Fn(usize) -> f64 + 'static,
    ) -> Self {
        Self {
            count: count.into(),
            estimate_size: Rc::new(estimate_size),
            shape: LayoutShape::List,
            gap: 0.0,
            budget: Budget::default(),
            axis: Axis::default(),
            padding_start: 0.0,
            padding_end: 0.0,
            epoch: None,
            pinned: None,
            initial_viewport: Viewport::main_only(0.0),
            initial_offset: 0.0,
            scroll_end_delay_ms: 150,
            retention_grace_ms: 0,
            retention_max: 12,
            measure_epsilon: 0.5,
            max_scroll_retries: 3,
        }
    }

    /// A grid virtualizer. `estimate_size` returns the row pitch.
    pub fn grid(
        count: impl Into<Signal<usize>>,
        estimate_size: impl Fn(usize) -> f64 + 'static,
        spec: GridSpec,
    ) -> Self {
        let mut options = Self::list(count, estimate_size);
        options.shape = LayoutShape::Grid(spec);
        options
    }

    /// Gap between list items.
    pub fn gap(mut self, gap: f64) -> Self {
        self.gap = gap;
        self
    }

    /// Mount budget.
    pub fn budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }

    /// Scroll axis.
    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    /// Content padding `(before, after)`.
    pub fn padding(mut self, start: f64, end: f64) -> Self {
        self.padding_start = start;
        self.padding_end = end;
        self
    }

    /// Layout epoch signal.
    pub fn epoch(mut self, epoch: Signal<u64>) -> Self {
        self.epoch = Some(epoch);
        self
    }

    /// Reactive pinned indices.
    pub fn pinned(mut self, pinned: Signal<Option<(usize, usize)>>) -> Self {
        self.pinned = Some(pinned);
        self
    }

    /// Initial viewport and scroll offset.
    pub fn initial(mut self, viewport: Viewport, offset: f64) -> Self {
        self.initial_viewport = viewport;
        self.initial_offset = offset;
        self
    }

    /// Zombie retention: how long an evicted item stays rendered after the
    /// window moves past it (`grace_ms`), and how many such items may be
    /// retained at once. `grace_ms == 0` disables retention. The grace can
    /// be temporarily raised later (a zoom holds items across its geometry
    /// commit) with [`Virtualizer::set_retention_grace`](crate::Virtualizer::set_retention_grace).
    pub fn retention(mut self, grace_ms: u32, max_retained: usize) -> Self {
        self.retention_grace_ms = grace_ms;
        self.retention_max = max_retained;
        self
    }

    /// Measurement and viewport epsilon.
    pub fn epsilon(mut self, eps: f64) -> Self {
        self.measure_epsilon = eps;
        self
    }

    /// Max scroll-to re-aims.
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.max_scroll_retries = retries;
        self
    }
}
