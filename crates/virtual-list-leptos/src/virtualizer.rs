//! The reactive adapter: [`use_virtualizer`] wires the pure
//! [`crate::core::VirtualizerCore`] to Leptos signals, the scroll container,
//! and two `ResizeObserver`s — applying the engine's [`crate::core::Step`]s
//! with write-if-changed guards so nothing downstream re-renders unless the
//! mounted window actually changed.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::{Event, ResizeObserver, ResizeObserverEntry};

use virtual_list::{Align, Viewport, Window};

use crate::core::{CoreConfig, Step, VirtualizerCore, build_layout};
use crate::measure::observe_item;
use crate::observe::{raf, viewport_of};
use crate::options::{ScrollMode, VirtualizerOptions};
use crate::render::{Positioning, VirtualItem, VirtualRow};
use crate::surface::{DomSurface, ScrollSurface};

type ObserverCallback = Closure<dyn FnMut(js_sys::Array, ResizeObserver)>;
type ObserverBinding = (ResizeObserver, ObserverCallback);
type ListenerCallback = Closure<dyn FnMut(Event)>;
type ListenerBinding = (web_sys::Element, &'static str, ListenerCallback);
type RangeCallback = Rc<dyn Fn(Option<Window>)>;
type IdleCallback = Rc<dyn Fn()>;

/// Create a virtualizer. Must be called inside a reactive owner.
pub fn use_virtualizer(options: VirtualizerOptions) -> Virtualizer {
    let count0 = options.count.get_untracked();
    let config = CoreConfig {
        budget: options.budget,
        shape: options.shape,
        gap: options.gap,
        sticky: options.sticky.clone(),
        padding_start: options.padding_start,
        padding_end: options.padding_end,
        viewport: options.initial_viewport,
        initial_offset: options.initial_offset,
        eps: options.measure_epsilon,
        max_retries: options.max_scroll_retries,
    };
    let layout = build_layout(
        &options.shape,
        count0,
        &*options.estimate_size,
        options.initial_viewport.cross,
        options.gap,
        &options.sticky,
    );
    let core = VirtualizerCore::new(layout, config);
    let initial_range = core.range();
    let initial_scroll = core.scroll_top();
    let initial_epoch = options
        .epoch
        .map(|signal| signal.get_untracked())
        .unwrap_or(0);

    let inner = Rc::new(VirtualizerInner {
        surface: DomSurface::new(options.axis, options.padding_start),
        scroll_top: RwSignal::new(initial_scroll),
        viewport: RwSignal::new(options.initial_viewport),
        range: RwSignal::new(initial_range),
        layout_version: RwSignal::new(0),
        is_scrolling: RwSignal::new(false),
        last_epoch: Cell::new(initial_epoch),
        options,
        core: RefCell::new(core),
        pending_scroll: Rc::new(Cell::new(None)),
        scroll_armed: Rc::new(Cell::new(false)),
        flush_armed: Rc::new(Cell::new(false)),
        scroll_feedback: Cell::new(true),
        container_ro: RefCell::new(None),
        item_ro: RefCell::new(None),
        listeners: RefCell::new(Vec::new()),
        scroll_end_timer: RefCell::new(None),
        range_cbs: RefCell::new(Vec::new()),
        idle_cbs: RefCell::new(Vec::new()),
        items_signal: OnceCell::new(),
        rows_signal: OnceCell::new(),
        total_signal: OnceCell::new(),
        padding_signal: OnceCell::new(),
        range_signal: OnceCell::new(),
        dominant_signal: OnceCell::new(),
        scrolling_signal: OnceCell::new(),
    });

    {
        let inner = inner.clone();
        Effect::new(move |_| {
            let count = inner.options.count.get();
            let epoch = inner.options.epoch.map(|signal| signal.get()).unwrap_or(0);
            let estimate = inner.options.estimate_size.clone();

            let step = {
                let mut core = inner.core.borrow_mut();
                if core.item_count() != count {
                    Some(core.set_count(count, &*estimate))
                } else if epoch != inner.last_epoch.get() {
                    Some(core.rebuild(&*estimate))
                } else {
                    None
                }
            };

            if let Some(step) = step {
                inner.apply(step);
            }
            inner.last_epoch.set(epoch);
        });
    }

    if let Some(signal) = inner.options.pinned {
        let inner = inner.clone();
        Effect::new(move |_| {
            let step = inner.core.borrow_mut().set_pinned(signal.get());
            inner.apply(step);
        });
    }

    {
        let inner = inner.clone();
        Effect::new(move |_| {
            let range = inner.range.get();
            let callbacks: Vec<_> = inner.range_cbs.borrow().iter().cloned().collect();
            for callback in callbacks {
                callback(range);
            }
        });
    }

    {
        let inner_handle = StoredValue::new_local(inner.clone());
        on_cleanup(move || inner_handle.with_value(|inner| inner.dispose()));
    }

    let virtualizer = Virtualizer { inner };

    // MATERIALIZE THE DERIVED SIGNALS HERE, IN THIS OWNER.
    //
    // The reader's effects (navigation_sync's scroll→page sync, the pinned
    // window in ReaderPage) call `v.dominant()`, and other components call
    // `items()`/`rows()`/`total_size()`/... lazily through these accessors.
    // `Signal::derive_local` registers the memo with the CURRENT reactive
    // owner, and a Leptos effect runs its callback inside a per-run temporary
    // owner that is disposed the moment the run ends. A signal first created
    // inside such a run would therefore be DISPOSED before the next effect
    // run could read it — every subsequent `dominant().get()` panics with
    // "already been disposed", which kills the scroll→page sync, the zoom
    // window pinning and the thumbnail page tracking all at once. Creating
    // them eagerly here (use_virtualizer runs in the component's stable
    // owner) makes their lifetime the component's, not a single effect run's.
    // Memos are lazy, so this is node registration only — no computation.
    let _ = virtualizer.items();
    let _ = virtualizer.rows();
    let _ = virtualizer.total_size();
    let _ = virtualizer.padding();
    let _ = virtualizer.range();
    let _ = virtualizer.dominant();
    let _ = virtualizer.is_scrolling();

    virtualizer
}

/// Shared adapter state.
pub(crate) struct VirtualizerInner {
    pub options: VirtualizerOptions,
    pub core: RefCell<VirtualizerCore>,
    pub surface: DomSurface,

    pub scroll_top: RwSignal<f64>,
    pub viewport: RwSignal<Viewport>,
    pub range: RwSignal<Option<Window>>,
    pub layout_version: RwSignal<u64>,
    pub is_scrolling: RwSignal<bool>,
    pub last_epoch: Cell<u64>,

    pub pending_scroll: Rc<Cell<Option<f64>>>,
    pub scroll_armed: Rc<Cell<bool>>,
    pub flush_armed: Rc<Cell<bool>>,

    /// While false, the DOM scroll echo must not touch the core. A programmatic
    /// scroll burst (a zoom tween, a sidebar slide, a resize drag) writes the
    /// surface every frame; the browser fires the corresponding scroll events
    /// a frame late, so echoing them back overwrites the core's anchor position
    /// with a stale value and the next anchored rescale oscillates around the
    /// true path — the content visibly jitters during the animation. The app
    /// flips this off for the duration of such a gesture and back on when the
    /// gesture commits.
    pub scroll_feedback: Cell<bool>,

    pub container_ro: RefCell<Option<ObserverBinding>>,
    pub item_ro: RefCell<Option<ObserverBinding>>,
    pub listeners: RefCell<Vec<ListenerBinding>>,
    pub scroll_end_timer: RefCell<Option<TimeoutHandle>>,

    pub range_cbs: RefCell<Vec<RangeCallback>>,
    pub idle_cbs: RefCell<Vec<IdleCallback>>,

    pub items_signal: OnceCell<Signal<Vec<VirtualItem>, LocalStorage>>,
    pub rows_signal: OnceCell<Signal<Vec<VirtualRow>, LocalStorage>>,
    pub total_signal: OnceCell<Signal<f64, LocalStorage>>,
    pub padding_signal: OnceCell<Signal<(f64, f64), LocalStorage>>,
    pub range_signal: OnceCell<Signal<Option<Window>, LocalStorage>>,
    pub dominant_signal: OnceCell<Signal<usize, LocalStorage>>,
    pub scrolling_signal: OnceCell<Signal<bool, LocalStorage>>,
}

impl VirtualizerInner {
    /// Apply an engine step.
    pub(crate) fn apply(&self, step: Step) {
        if step.layout_changed {
            self.layout_version.update(|version| *version += 1);
        }
        write_if_changed(self.range, step.range);
        if let Some(top) = step.scroll_write {
            if (top - self.scroll_top.get_untracked()).abs() > self.options.measure_epsilon {
                self.surface.set_scroll(top, false);
            }
            write_if_changed(self.scroll_top, top);
        }
    }

    /// Apply a step produced by a scroll COMMAND (the surface was already
    /// written by the core): signals only, no second DOM write. Instant
    /// writes carry the adopted position; smooth writes return no step and
    /// surface later through `handle_scroll`.
    pub(crate) fn apply_local(&self, step: Step) {
        if step.layout_changed {
            self.layout_version.update(|version| *version += 1);
        }
        write_if_changed(self.range, step.range);
        write_if_changed(self.scroll_top, self.core.borrow().scroll_top());
    }

    /// rAF-coalesced scroll handling.
    pub(crate) fn handle_scroll(self: &Rc<Self>, dom_top: f64) {
        if !self.scroll_feedback.get() {
            // A programmatic gesture owns the surface: its anchored writes are
            // authoritative and the echo is one frame stale. Adopting it here
            // would corrupt the anchor (see the field docs); the gesture's own
            // `apply` already published the position to the scroll_top signal.
            return;
        }
        let content = dom_top - self.options.padding_start;
        let step = self.core.borrow_mut().on_scroll(content);
        write_if_changed(self.scroll_top, content);
        write_if_changed(self.range, step.range);
        if let Some(top) = step.scroll_write {
            self.surface.set_scroll(top, false);
        }
        if !self.is_scrolling.get_untracked() {
            self.is_scrolling.set(true);
        }
        self.arm_scroll_end();
    }

    /// ε-guarded viewport updates.
    pub(crate) fn handle_viewport(self: &Rc<Self>, vp: Viewport) {
        let current = self.viewport.get_untracked();
        let eps = self.options.measure_epsilon;
        if (vp.main - current.main).abs() <= eps && (vp.cross - current.cross).abs() <= eps {
            return;
        }
        self.viewport.set(vp);
        let step = self.core.borrow_mut().on_viewport(vp);
        self.apply(step);
    }

    /// Arm the once-per-frame measurement flush.
    pub(crate) fn arm_flush(self: &Rc<Self>) {
        if self.flush_armed.get() || self.core.borrow().suspended() {
            return;
        }
        self.flush_armed.set(true);
        let inner = self.clone();
        raf(move || {
            inner.flush_armed.set(false);
            let flush = inner.core.borrow_mut().flush();
            if let Some(flush) = flush {
                inner.apply(flush.step);
            }
        });
    }

    /// Debounced scroll-idle timer.
    pub(crate) fn arm_scroll_end(self: &Rc<Self>) {
        if let Some(handle) = self.scroll_end_timer.borrow_mut().take() {
            handle.clear();
        }
        let inner = self.clone();
        let delay = Duration::from_millis(self.options.scroll_end_delay_ms as u64);
        if let Ok(handle) = set_timeout_with_handle(
            move || {
                inner.is_scrolling.set(false);
                let callbacks: Vec<_> = inner.idle_cbs.borrow().iter().cloned().collect();
                for callback in callbacks {
                    callback();
                }
            },
            delay,
        ) {
            *self.scroll_end_timer.borrow_mut() = Some(handle);
        }
    }

    /// Release every DOM handle.
    pub(crate) fn dispose(&self) {
        self.teardown_bindings();
        if let Some(handle) = self.scroll_end_timer.borrow_mut().take() {
            handle.clear();
        }
        self.surface.detach();
    }

    /// Drop listeners and observers associated with the bound container.
    pub(crate) fn teardown_bindings(&self) {
        for (element, name, closure) in self.listeners.borrow().iter() {
            let _ =
                element.remove_event_listener_with_callback(name, closure.as_ref().unchecked_ref());
        }
        self.listeners.borrow_mut().clear();
        if let Some((ro, _)) = self.container_ro.borrow_mut().take() {
            ro.disconnect();
        }
        if let Some((ro, _)) = self.item_ro.borrow_mut().take() {
            ro.disconnect();
        }
    }
}

/// The public handle. Cheap to clone (`Rc` inside).
#[derive(Clone)]
pub struct Virtualizer {
    inner: Rc<VirtualizerInner>,
}

/// Write a signal only when the value actually changed.
fn write_if_changed<T>(signal: RwSignal<T>, value: T)
where
    T: PartialEq + Copy + Send + Sync + 'static,
{
    if signal.get_untracked() != value {
        signal.set(value);
    }
}

fn dom_scroll_offset(el: &web_sys::HtmlElement, axis: crate::options::Axis) -> f64 {
    match axis {
        crate::options::Axis::Vertical => el.scroll_top() as f64,
        crate::options::Axis::Horizontal => el.scroll_left() as f64,
    }
}

impl Virtualizer {
    /// Bind the scroll container.
    pub fn bind_container(&self, el: web_sys::Element) {
        let inner = &self.inner;
        inner.teardown_bindings();
        inner.surface.attach(el.clone());

        let viewport = viewport_of(&el, inner.options.axis);
        inner.viewport.set(viewport);
        let step = inner.core.borrow_mut().on_viewport(viewport);
        let had_scroll_write = step.scroll_write.is_some();
        inner.apply(step);

        if !had_scroll_write {
            let current = inner.core.borrow().scroll_top();
            if current < 0.0 || current > inner.options.measure_epsilon {
                inner.surface.set_scroll(current, false);
            }
        }

        {
            let inner_for_listener = inner.clone();
            let closure = Closure::<dyn FnMut(Event)>::new(move |_| {
                let Some(element) = inner_for_listener.surface.element() else {
                    return;
                };
                let Ok(html) = element.dyn_into::<web_sys::HtmlElement>() else {
                    return;
                };
                let dom_top = dom_scroll_offset(&html, inner_for_listener.options.axis);
                inner_for_listener.pending_scroll.set(Some(dom_top));
                if !inner_for_listener.scroll_armed.get() {
                    inner_for_listener.scroll_armed.set(true);
                    let inner2 = inner_for_listener.clone();
                    raf(move || {
                        inner2.scroll_armed.set(false);
                        if let Some(dom) = inner2.pending_scroll.take() {
                            inner2.handle_scroll(dom);
                        }
                    });
                }
            });
            let _ = el.add_event_listener_with_callback("scroll", closure.as_ref().unchecked_ref());
            inner
                .listeners
                .borrow_mut()
                .push((el.clone(), "scroll", closure));
        }

        {
            let inner_for_observer = inner.clone();
            let callback = Closure::<dyn FnMut(js_sys::Array, ResizeObserver)>::new(
                move |entries: js_sys::Array, _| {
                    if let Some(last) = entries.iter().last() {
                        let entry: ResizeObserverEntry = last.unchecked_into();
                        let target = entry.target();
                        if let Some(target) = target.dyn_ref::<web_sys::Element>() {
                            let viewport = viewport_of(target, inner_for_observer.options.axis);
                            inner_for_observer.handle_viewport(viewport);
                        }
                    }
                },
            );
            if let Ok(observer) = ResizeObserver::new(callback.as_ref().unchecked_ref()) {
                observer.observe(&el);
                *inner.container_ro.borrow_mut() = Some((observer, callback));
            }
        }
    }

    /// Reactive mounted items.
    pub fn items(&self) -> Signal<Vec<VirtualItem>, LocalStorage> {
        *self.inner.items_signal.get_or_init(|| {
            let inner = self.inner.clone();
            Signal::derive_local(move || {
                let _ = inner.range.get();
                let _ = inner.layout_version.get();
                inner.core.borrow().items()
            })
        })
    }

    /// Reactive mounted rows.
    pub fn rows(&self) -> Signal<Vec<VirtualRow>, LocalStorage> {
        *self.inner.rows_signal.get_or_init(|| {
            let inner = self.inner.clone();
            Signal::derive_local(move || {
                let _ = inner.range.get();
                let _ = inner.layout_version.get();
                inner.core.borrow().rows()
            })
        })
    }

    /// Full spacer extent (paddings included).
    pub fn total_size(&self) -> Signal<f64, LocalStorage> {
        *self.inner.total_signal.get_or_init(|| {
            let inner = self.inner.clone();
            Signal::derive_local(move || {
                let _ = inner.layout_version.get();
                inner.core.borrow().total_size()
            })
        })
    }

    /// `(before, after)` spacer heights for [`Positioning::Padding`].
    pub fn padding(&self) -> Signal<(f64, f64), LocalStorage> {
        *self.inner.padding_signal.get_or_init(|| {
            let inner = self.inner.clone();
            Signal::derive_local(move || {
                let _ = inner.range.get();
                let _ = inner.layout_version.get();
                inner.core.borrow().padding()
            })
        })
    }

    /// The configured positioning mode.
    pub fn positioning(&self) -> Positioning {
        self.inner.options.positioning
    }

    /// Reactive mount window.
    pub fn range(&self) -> Signal<Option<Window>, LocalStorage> {
        *self.inner.range_signal.get_or_init(|| {
            let inner = self.inner.clone();
            Signal::derive_local(move || inner.range.get())
        })
    }

    /// Reactive dominant item.
    pub fn dominant(&self) -> Signal<usize, LocalStorage> {
        *self.inner.dominant_signal.get_or_init(|| {
            let inner = self.inner.clone();
            Signal::derive_local(move || {
                let _ = inner.scroll_top.get();
                let _ = inner.layout_version.get();
                inner.core.borrow().dominant()
            })
        })
    }

    /// Whether the container is scrolling.
    pub fn is_scrolling(&self) -> Signal<bool, LocalStorage> {
        *self.inner.scrolling_signal.get_or_init(|| {
            let inner = self.inner.clone();
            Signal::derive_local(move || inner.is_scrolling.get())
        })
    }

    /// The scroll position signal (content coordinates).
    pub fn scroll_offset(&self) -> RwSignal<f64> {
        self.inner.scroll_top
    }

    /// The viewport signal.
    pub fn viewport(&self) -> RwSignal<Viewport> {
        self.inner.viewport
    }

    /// Re-read the bound container's real viewport and scroll position.
    pub fn remeasure_container(&self) {
        let Some(el) = self.inner.surface.element() else {
            return;
        };
        let vp = viewport_of(&el, self.inner.options.axis);
        self.inner.handle_viewport(vp);

        let Ok(html) = el.dyn_into::<web_sys::HtmlElement>() else {
            return;
        };
        let content =
            dom_scroll_offset(&html, self.inner.options.axis) - self.inner.options.padding_start;
        if (content - self.inner.core.borrow().scroll_top()).abs()
            > self.inner.options.measure_epsilon
        {
            let step = self.inner.core.borrow_mut().on_scroll(content);
            write_if_changed(self.inner.scroll_top, content);
            write_if_changed(self.inner.range, step.range);
        }
    }

    /// Snapshot offset of an item, padding included.
    pub fn offset_of(&self, index: usize) -> f64 {
        self.inner.core.borrow().offset_of(index)
    }

    /// Reactive main-axis offset of one item, padding included.
    ///
    /// Create this once per mounted child. It depends only on the layout
    /// version, so it recomputes when geometry changes and never on plain
    /// scrolling.
    pub fn item_top(&self, index: usize) -> Signal<f64, LocalStorage> {
        let inner = self.inner.clone();
        Signal::derive_local(move || {
            let _ = inner.layout_version.get();
            inner.core.borrow().offset_of(index)
        })
    }

    /// Scroll to an absolute content offset.
    ///
    /// Instant writes are adopted into the local signals immediately (the
    /// core already wrote the DOM, so there is no second write); smooth
    /// writes surface through the coalesced scroll handling once the
    /// browser echoes them back.
    pub fn scroll_to_offset(&self, offset: f64, mode: ScrollMode) {
        let step = self
            .inner
            .core
            .borrow_mut()
            .scroll_to_offset(offset, mode, &self.inner.surface);
        if let Some(step) = step {
            self.inner.apply_local(step);
        }
    }

    /// Scroll to an item with an alignment.
    pub fn scroll_to_index(&self, index: usize, align: Align, mode: ScrollMode) {
        let step = self
            .inner
            .core
            .borrow_mut()
            .scroll_to_index(index, align, mode, &self.inner.surface);
        if let Some(step) = step {
            self.inner.apply_local(step);
        }
    }

    /// Abandon an in-flight scroll-to.
    pub fn cancel_scroll(&self) {
        self.inner.core.borrow_mut().cancel_scroll();
    }

    /// Register an item element for DOM measurement.
    pub fn measure_element(&self, index: usize, el: &web_sys::Element) {
        observe_item(&self.inner, index, el);
    }

    /// Report a size directly.
    pub fn report_size(&self, index: usize, size: f64) {
        self.inner.core.borrow_mut().queue_size(index, size);
        self.inner.arm_flush();
    }

    /// Buffer measurements without flushing.
    pub fn suspend_measurements(&self) {
        self.inner.core.borrow_mut().suspend();
    }

    /// Resume flushing.
    pub fn resume_measurements(&self) {
        let flush = self.inner.core.borrow_mut().resume();
        if let Some(flush) = flush {
            self.inner.apply(flush.step);
        }
    }

    /// Ignore the DOM scroll echo until [`resume_scroll_feedback`]. Use around
    /// a sustained programmatic scroll burst (zoom tween, sidebar slide,
    /// resize drag): the browser echoes those writes one frame late, and
    /// letting the echo overwrite the core's anchor position makes each
    /// anchored rescale pin from a stale offset — the content oscillates
    /// instead of gliding.
    pub fn suspend_scroll_feedback(&self) {
        self.inner.scroll_feedback.set(false);
    }

    /// Re-adopt the DOM scroll echo (see [`suspend_scroll_feedback`]).
    pub fn resume_scroll_feedback(&self) {
        self.inner.scroll_feedback.set(true);
    }

    /// Zoom: multiply every size by `factor` while keeping the viewport center pinned.
    pub fn rescale(&self, factor: f64, new_sizes: impl Fn(usize) -> f64) {
        let step = self.inner.core.borrow_mut().rescale(factor, &new_sizes);
        self.inner.apply(step);
    }

    /// Called whenever the mounted window changes.
    pub fn on_range_change(&self, cb: impl Fn(Option<Window>) + 'static) {
        self.inner.range_cbs.borrow_mut().push(Rc::new(cb));
    }

    /// Called when scrolling settles.
    pub fn on_scroll_idle(&self, cb: impl Fn() + 'static) {
        self.inner.idle_cbs.borrow_mut().push(Rc::new(cb));
    }
}
