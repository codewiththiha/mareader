//! The reactive adapter: [`use_virtualizer`] wires the pure
//! [`crate::engine::VirtualizerCore`] to Leptos signals, the scroll container,
//! and two `ResizeObserver`s — applying the engine's [`crate::engine::Step`]s
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

use crate::engine::{Step, VirtualizerCore};
use crate::observe::{raf, viewport_of};
use crate::options::{ScrollMode, VirtualizerOptions};
use crate::render::{VirtualItem, VirtualItemState, VirtualRow};
use crate::retention::{prune_retained, retain_evicted};
use crate::surface::{DomSurface, ScrollSurface};

type ObserverCallback = Closure<dyn FnMut(js_sys::Array, ResizeObserver)>;
type ListenerCallback = Closure<dyn FnMut(Event)>;
type IdleCallback = Rc<dyn Fn()>;

/// One `ResizeObserver` and the wasm-bindgen closure that serves it. Named
/// so the pairing is explicit: the callback must stay alive exactly as long
/// as the observer is connected, and `dispose` drops both together.
pub(crate) struct ObserverBinding {
    observer: ResizeObserver,
    callback: ObserverCallback,
}

/// One event listener and the closure that serves it. Named for the same
/// reason: removing the listener with the SAME closure identity is what
/// releases the JS-side function (and the WASM closure behind it).
pub(crate) struct ListenerBinding {
    element: web_sys::Element,
    event: &'static str,
    callback: ListenerCallback,
}

impl VirtualizerInner {
    /// Build the shared state for a fresh virtualizer. All signal/lazy
    /// handles are created HERE, in the hook's owner, so their lifetimes are
    /// the component's (a signal created inside an effect run would be
    /// disposed with that run — see `use_virtualizer` for the full story).
    pub(crate) fn new(
        options: VirtualizerOptions,
        core: VirtualizerCore,
        initial_range: Option<Window>,
        initial_scroll: f64,
        initial_epoch: u64,
    ) -> Rc<Self> {
        // Read every option-derived value before the struct literal moves
        // `options` into the `options` field.
        let initial_viewport = options.initial_viewport;
        let initial_grace = options.retention_grace_ms;
        Rc::new(Self {
            surface: DomSurface::new(options.axis, options.padding_start),
            scroll_top: RwSignal::new(initial_scroll),
            viewport: RwSignal::new(initial_viewport),
            range: RwSignal::new(initial_range),
            layout_version: RwSignal::new(0),

            last_epoch: Cell::new(initial_epoch),
            options,
            core: RefCell::new(core),
            pending_scroll: Rc::new(Cell::new(None)),
            scroll_armed: Rc::new(Cell::new(false)),
            flush_armed: Rc::new(Cell::new(false)),
            scroll_feedback: Cell::new(true),
            container_ro: RefCell::new(None),
            listeners: RefCell::new(Vec::new()),
            scroll_end_timer: RefCell::new(None),
            retained: RefCell::new(Vec::new()),
            retained_version: RwSignal::new(0),
            retention_grace: Cell::new(initial_grace),
            retention_timer: RefCell::new(None),
            idle_cbs: RefCell::new(Vec::new()),
            items_signal: OnceCell::new(),
            rows_signal: OnceCell::new(),
            total_signal: OnceCell::new(),
            dominant_signal: OnceCell::new(),
        })
    }
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
    pub listeners: RefCell<Vec<ListenerBinding>>,
    pub scroll_end_timer: RefCell<Option<TimeoutHandle>>,

    /// Zombie retention bookkeeping (see `retention.rs`): evicted items
    /// still mounted, the reactive version that items() tracks, the
    /// currently effective grace (raised around a zoom commit), and the
    /// expiry timer.
    pub retained: RefCell<Vec<crate::retention::RetainedItem>>,
    pub retained_version: RwSignal<u64>,
    pub retention_grace: Cell<u32>,
    pub retention_timer: RefCell<Option<TimeoutHandle>>,

    pub idle_cbs: RefCell<Vec<IdleCallback>>,

    pub items_signal: OnceCell<Signal<Vec<VirtualItem>, LocalStorage>>,
    pub rows_signal: OnceCell<Signal<Vec<VirtualRow>, LocalStorage>>,
    pub total_signal: OnceCell<Signal<f64, LocalStorage>>,
    pub dominant_signal: OnceCell<Signal<usize, LocalStorage>>,
}

impl VirtualizerInner {
    /// Publish a new mount window: write-if-changed, and schedule zombie
    /// retention for the items the change evicted. Every range write in the
    /// adapter funnels through here so retention cannot miss a transition.
    pub(crate) fn publish_range(self: &Rc<Self>, new: Option<Window>) {
        let old = self.range.get_untracked();
        if old == new {
            return;
        }
        let grace = self.retention_grace.get();
        if grace > 0 && self.options.retention_max > 0 {
            let now = now_ms();
            let evicted =
                retain_evicted(old, new, now, grace, self.options.retention_max);
            if !evicted.is_empty() {
                let mut retained = self.retained.borrow_mut();
                // Merge: an index already retained keeps its original expiry
                // only if it is still outside the new window; re-entry drops it.
                *retained = prune_retained(
                    std::mem::take(&mut *retained),
                    new,
                    now,
                );
                for item in evicted {
                    if !retained.iter().any(|r| r.index == item.index) {
                        retained.push(item);
                    }
                }
                let max = self.options.retention_max;
                if retained.len() > max {
                    let drop = retained.len() - max;
                    retained.drain(0..drop);
                }
                drop(retained);
                self.retained_version.update(|v| *v += 1);
                self.arm_retention_timer();
            }
        }
        self.range.set(new);
    }

    /// Arm (once) the timer that prunes expired zombies. Re-arms itself
    /// while anything is still retained.
    pub(crate) fn arm_retention_timer(self: &Rc<Self>) {
        if self.retention_timer.borrow().is_some() {
            return;
        }
        let inner = self.clone();
        if let Ok(handle) = set_timeout_with_handle(
            move || {
                inner.retention_timer.borrow_mut().take();
                let now = now_ms();
                let active = inner.core.borrow().range();
                let mut retained = inner.retained.borrow_mut();
                let before = retained.len();
                *retained = prune_retained(std::mem::take(&mut *retained), active, now);
                let changed = before != retained.len();
                drop(retained);
                if changed {
                    inner.retained_version.update(|v| *v += 1);
                }
                if !inner.retained.borrow().is_empty() {
                    inner.arm_retention_timer();
                }
            },
            Duration::from_millis(retention_tick_ms(&self.retained.borrow(), now_ms())),
        ) {
            *self.retention_timer.borrow_mut() = Some(handle);
        }
    }

    /// Apply an engine step.
    pub(crate) fn apply(self: &Rc<Self>, step: Step) {
        if step.layout_changed {
            self.layout_version.update(|version| *version += 1);
        }
        self.publish_range(step.range);
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
    pub(crate) fn apply_local(self: &Rc<Self>, step: Step) {
        if step.layout_changed {
            self.layout_version.update(|version| *version += 1);
        }
        self.publish_range(step.range);
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
        // Sub-epsilon echo (fractional-pixel wheel deltas fire events whose
        // movement is below measure_epsilon): the engine would ignore the
        // rewindow anyway, so skip the signal write, the range publish and
        // the idle-timer re-arm entirely — dominant-page tracking and
        // navigation sync stay quiet during sub-pixel movement.
        if (content - self.core.borrow().scroll_top()).abs()
            <= self.options.measure_epsilon
        {
            return;
        }
        let step = self.core.borrow_mut().on_scroll(content);
        write_if_changed(self.scroll_top, content);
        self.publish_range(step.range);
        if let Some(top) = step.scroll_write {
            self.surface.set_scroll(top, false);
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
        if let Some(handle) = self.retention_timer.borrow_mut().take() {
            handle.clear();
        }
        self.surface.detach();
    }

    /// Drop listeners and observers associated with the bound container.
    ///
    /// The removal happens with the SAME closure identity that was added
    /// (the JS `removeEventListener` matches by function reference), and the
    /// closure is dropped immediately after — so a rebound container can
    /// never leak a WASM closure, and a disposed virtualizer releases every
    /// DOM handle it took.
    pub(crate) fn teardown_bindings(&self) {
        for binding in self.listeners.borrow_mut().drain(..) {
            let _ = binding.element.remove_event_listener_with_callback(
                binding.event,
                binding.callback.as_ref().unchecked_ref(),
            );
        }
        if let Some(binding) = self.container_ro.borrow_mut().take() {
            let ObserverBinding { observer, callback } = binding;
            observer.disconnect();
            // Release the wasm-bindgen closure AFTER the observer is dead;
            // dropping it before disconnect would leave the observer holding
            // a dangling JS callback.
            drop(callback);
        }
    }
}

/// The public handle. Cheap to clone (`Rc` inside).
#[derive(Clone)]
pub struct Virtualizer {
    inner: Rc<VirtualizerInner>,
}

impl Virtualizer {
    /// Wrap freshly created inner state (the hook's constructor).
    pub(crate) fn from_inner(inner: Rc<VirtualizerInner>) -> Self {
        Self { inner }
    }
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

/// The retention clock, in milliseconds.
///
/// `performance.now()` is monotonic (time origin) and sub-millisecond, so a
/// zombie's `expires_at` is a true duration — a wall-clock NTP step or a DST
/// switch cannot extend or cut a grace period short mid-zoom. `Date::now()`
/// is the fallback for the webviews where `performance` is unavailable. (The
/// pure retention maths in `retention.rs` stays host-testable without this.)
fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or_else(|| js_sys::Date::now())
}

/// Milliseconds until the next zombie expiry (always at least 1, so a timer
/// is always armed into the future).
fn retention_tick_ms(retained: &[crate::retention::RetainedItem], now: f64) -> u64 {
    retained
        .iter()
        .map(|item| (item.expires_at - now).max(1.0))
        .fold(f64::INFINITY, f64::min)
        .ceil() as u64
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
            inner.listeners.borrow_mut().push(ListenerBinding {
                element: el.clone(),
                event: "scroll",
                callback: closure,
            });
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
                *inner.container_ro.borrow_mut() = Some(ObserverBinding { observer, callback });
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
                let _ = inner.retained_version.get();
                let active = inner.core.borrow().items();
                // Zombies: retained, unexpired, outside the active window.
                // Rendered with live layout geometry so they sit exactly
                // where the layout says, at the committed scale.
                let now = now_ms();
                let window = inner.core.borrow().range();
                let retained: Vec<usize> = inner
                    .retained
                    .borrow()
                    .iter()
                    .filter(|r| r.expires_at > now)
                    .map(|r| r.index)
                    .filter(|index| {
                        window.map(|w| *index < w.first || *index > w.last).unwrap_or(false)
                    })
                    .collect();
                if retained.is_empty() {
                    return active;
                }
                let mut items = active;
                for index in retained {
                    let mut item = inner.core.borrow().item_at(index);
                    item.state = VirtualItemState::Zombie;
                    items.push(item);
                }
                items.sort_by_key(|item| item.index);
                items
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
            self.inner.publish_range(step.range);
        }
    }

    /// Snapshot offset of an item, padding included.
    pub fn offset_of(&self, index: usize) -> f64 {
        self.inner.core.borrow().offset_of(index)
    }

    /// Index of the item whose span contains `pos` (leading-edge semantics),
    /// `O(log n)` over the strip's prefix sums. The inverse of
    /// [`offset_of`](Self::offset_of): a position handed back by that method
    /// resolves to the same item it came from.
    pub fn index_at(&self, pos: f64) -> usize {
        self.inner.core.borrow().index_at(pos)
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

    /// Raise the zombie retention grace (e.g. for the duration of a zoom
    /// transaction, whose geometry commit evicts pages that are still on
    /// screen). Items evicted while the raised grace is in force keep it
    /// until their own expiry. Call [`Self::reset_retention_grace`] to
    /// return to the configured default.
    pub fn set_retention_grace(&self, ms: u32) {
        self.inner.retention_grace.set(ms);
    }

    /// Return the retention grace to the configured default.
    pub fn reset_retention_grace(&self) {
        self.inner
            .retention_grace
            .set(self.inner.options.retention_grace_ms);
    }

    /// Drop every retained zombie whose grace has already expired, publishing
    /// the change so its DOM (and the engine surface it holds) unmounts.
    ///
    /// The ordinary path handles this with the expiry timer armed by each
    /// eviction. That timer is per-eviction bookkeeping on the item's owner,
    /// however, so a zombie retained around a zoom can outlive the transaction
    /// that raised its grace and sit on a large (recently zoomed) bitmap until
    /// the window moves. The zoom-settle hook calls this once the grace window
    /// closes, so a zoom's retained surfaces are released right after the
    /// commit instead of after the next scroll.
    pub fn prune_retained_now(&self) {
        let now = now_ms();
        let active = self.inner.core.borrow().range();
        let mut retained = self.inner.retained.borrow_mut();
        let before = retained.len();
        *retained = prune_retained(std::mem::take(&mut *retained), active, now);
        let changed = before != retained.len();
        drop(retained);
        if changed {
            self.inner.retained_version.update(|v| *v += 1);
        }
    }

    /// Zoom: multiply every size by `factor` while keeping the viewport center pinned.
    pub fn rescale(&self, factor: f64, new_sizes: impl Fn(usize) -> f64) {
        let step = self.inner.core.borrow_mut().rescale(factor, &new_sizes);
        self.inner.apply(step);
    }

    /// Called when scrolling settles.
    pub fn on_scroll_idle(&self, cb: impl Fn() + 'static) {
        self.inner.idle_cbs.borrow_mut().push(Rc::new(cb));
    }
}
