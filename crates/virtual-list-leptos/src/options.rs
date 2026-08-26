//! Adapter options: what the consumer configures once per virtualizer.

use std::rc::Rc;

use leptos::prelude::*;
use virtual_list::{Budget, GridSpec, Viewport};

use crate::render::Positioning;

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
    /// Sticky item indices (list layouts only).
    pub sticky: Vec<usize>,
    /// Render contract.
    pub positioning: Positioning,
    /// Viewport used before the first ResizeObserver report.
    pub initial_viewport: Viewport,
    /// Initial scroll position (content coordinates).
    pub initial_offset: f64,
    /// Scroll-idle debounce (`is_scrolling` reset), milliseconds.
    pub scroll_end_delay_ms: u32,
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
            sticky: Vec::new(),
            positioning: Positioning::default(),
            initial_viewport: Viewport::main_only(0.0),
            initial_offset: 0.0,
            scroll_end_delay_ms: 150,
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

    /// Sticky indices.
    pub fn sticky(mut self, sticky: Vec<usize>) -> Self {
        self.sticky = sticky;
        self
    }

    /// Render contract.
    pub fn positioning(mut self, positioning: Positioning) -> Self {
        self.positioning = positioning;
        self
    }

    /// Initial viewport and scroll offset.
    pub fn initial(mut self, viewport: Viewport, offset: f64) -> Self {
        self.initial_viewport = viewport;
        self.initial_offset = offset;
        self
    }

    /// Scroll-idle debounce, milliseconds.
    pub fn scroll_end_delay(mut self, ms: u32) -> Self {
        self.scroll_end_delay_ms = ms;
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
