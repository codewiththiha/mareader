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
    /// The render band, in viewport screens around the viewport. Mounted items
    /// inside the band carry real content ([`crate::VirtualItemState::Active`]);
    /// mounted items outside it are [`crate::VirtualItemState::Blank`]
    /// placeholders at the layout's own sizes. `0` disables the band — the
    /// pages mode, where everything the window mounts renders fully. A stream
    /// pairs a wide mount budget with a band narrower than it, so a fling
    /// slides cheap placeholders past the reader's eyes and only the band
    /// around the viewport ever lays out real content.
    pub render_screens: f64,
    /// Called with the scroll container when it is (re)bound. Lets a consumer
    /// attach its own listeners (the reader's scroll-phase tracker) to the
    /// element the virtualizer already owns, without a second container
    /// reference.
    pub on_bind: Option<Rc<dyn Fn(web_sys::Element)>>,
    /// Called when the container is unbound (a re-bind or an unmount), so a
    /// consumer's listeners come back out with the virtualizer's own — a
    /// listener that survived its element would leak the element with it.
    pub on_unbind: Option<Rc<dyn Fn()>>,
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
            render_screens: 0.0,
            on_bind: None,
            on_unbind: None,
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

    /// A continuous stream: a list whose mount window is wider than its
    /// render band. Items the window mounts outside the band stay
    /// [`crate::VirtualItemState::Blank`] placeholders — layout and scrollbar
    /// honest, content free — until the band reaches them. Pair it with a
    /// mount budget wider than the band (the band is `0.75` viewport screens
    /// each way); a budget narrower than the band would mount nothing the
    /// band does not already cover.
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

    /// The grace can be raised later — a zoom holds items across its
    /// geometry commit — with
    /// [`Virtualizer::set_retention_grace`](crate::Virtualizer::set_retention_grace).
    pub fn retention(mut self, grace_ms: u32, max_retained: usize) -> Self {
        self.retention_grace_ms = grace_ms;
        self.retention_max = max_retained;
        self
    }

    /// Sets [`Self::on_bind`].
    pub fn on_bind(mut self, cb: impl Fn(web_sys::Element) + 'static) -> Self {
        self.on_bind = Some(Rc::new(cb));
        self
    }

    /// Sets [`Self::on_unbind`].
    pub fn on_unbind(mut self, cb: impl Fn() + 'static) -> Self {
        self.on_unbind = Some(Rc::new(cb));
        self
    }
}
