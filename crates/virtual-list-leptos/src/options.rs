//! Adapter options: what the consumer configures once per virtualizer.

use std::rc::Rc;

use leptos::prelude::*;
use virtual_list::{Budget, GridSpec, Pipeline, Viewport};

use crate::retention::RetentionPolicy;

/// Which scroll axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Axis {
    /// Top-to-bottom scrolling (the default).
    #[default]
    Vertical,
    /// Left-to-right scrolling.
    Horizontal,
}

/// Layout shape; for a grid the estimate is the row pitch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LayoutShape {
    /// A single column of variably-sized items.
    List,
    /// A uniform multi-column grid, windowed per row.
    Grid(GridSpec),
}

/// Scroll behavior for `scroll_to_*` commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollMode {
    /// Jump immediately.
    Instant,
    /// Browser-animated.
    Smooth,
    /// Smooth within two viewports, instant beyond.
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
    /// Per-item estimated size; never one global fallback.
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
    /// Bump to force a rebuild when geometry moves without a count change.
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
    /// How an item that leaves the window is retired.
    pub retention: RetentionPolicy,
    /// The content pipeline the render band is measured against.
    pub pipeline: Pipeline,
    /// Change-detection epsilon for measurements and viewport writes.
    pub measure_epsilon: f64,
    /// Max re-aims for an in-flight `scroll_to_index`.
    pub max_scroll_retries: u32,
    /// The render band in viewport screens; 0 disables it.
    pub render_screens: f64,
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
            retention: RetentionPolicy::Immediate,
            measure_epsilon: 0.5,
            pipeline: Pipeline::default(),
            max_scroll_retries: 3,
            render_screens: 0.0,
        }
    }

    /// A grid virtualizer; `estimate_size` returns the row pitch.
    pub fn grid(
        count: impl Into<Signal<usize>>,
        estimate_size: impl Fn(usize) -> f64 + 'static,
        spec: GridSpec,
    ) -> Self {
        let mut options = Self::list(count, estimate_size);
        options.shape = LayoutShape::Grid(spec);
        options
    }

    /// A continuous stream: a mount window wider than the render band.
    pub fn stream(
        count: impl Into<Signal<usize>>,
        estimate_size: impl Fn(usize) -> f64 + 'static,
    ) -> Self {
        Self::list(count, estimate_size).render_band(0.75)
    }

    /// Sets [`Self::render_screens`]; `0` disables the band.
    pub fn render_band(mut self, screens: f64) -> Self {
        self.render_screens = screens.max(0.0);
        self
    }

    /// Sets [`Self::gap`].
    pub fn gap(mut self, gap: f64) -> Self {
        self.gap = gap;
        self
    }

    /// Sets [`Self::budget`].
    pub fn budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }

    /// Sets [`Self::axis`].
    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    /// Sets [`Self::padding_start`] and [`Self::padding_end`].
    pub fn padding(mut self, start: f64, end: f64) -> Self {
        self.padding_start = start;
        self.padding_end = end;
        self
    }

    /// Sets [`Self::epoch`].
    pub fn epoch(mut self, epoch: Signal<u64>) -> Self {
        self.epoch = Some(epoch);
        self
    }

    /// Sets [`Self::pinned`].
    pub fn pinned(mut self, pinned: Signal<Option<(usize, usize)>>) -> Self {
        self.pinned = Some(pinned);
        self
    }

    /// Sets [`Self::initial_viewport`] and [`Self::initial_offset`].
    pub fn initial(mut self, viewport: Viewport, offset: f64) -> Self {
        self.initial_viewport = viewport;
        self.initial_offset = offset;
        self
    }

    /// Sets [`Self::retention`], later adjustable at runtime.
    pub fn retention(mut self, policy: RetentionPolicy) -> Self {
        self.retention = policy;
        self
    }
}
