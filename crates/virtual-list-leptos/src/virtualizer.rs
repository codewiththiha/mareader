//! The reactive adapter: the pure core wired to Leptos signals, the
//! container and two observers.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::{Event, ResizeObserver, ResizeObserverEntry};

use virtual_list::{Align, Layout, Viewport, Window};

use crate::engine::{Step, VirtualizerCore};
use crate::observe::{raf, viewport_of};
use crate::options::{ScrollMode, VirtualizerOptions};
use crate::render::{VirtualItem, VirtualItemState, VirtualRow};
use crate::retention::{
    RetainedItem, RetentionPolicy, is_retained, next_deadline_ms, prune_retained, retain_evicted,
};
use crate::surface::{DomSurface, ScrollSurface};

type ObserverCallback = Closure<dyn FnMut(js_sys::Array, ResizeObserver)>;
type ListenerCallback = Closure<dyn FnMut(Event)>;
type IdleCallback = Rc<dyn Fn()>;

/// End a gesture: land every correction, then say the scroller is quiet.
///
/// The order is the contract: `settled` is the liveness probe deferred first
/// paints wait on, so nothing it wakes may see a window this still has to
/// move.
fn settle(inner: &Rc<VirtualizerInner>) {
    // Disposed while pending: the settled write belongs to nobody.
    if inner.surface.element().is_none() || inner.settled.try_get_untracked().is_none() {
        return;
    }
    // Measurements the fling outran still owe the reader their correction.
    // The borrow ends at the statement, not the `if let`: applying the step
    // reads the core again.
    let pending = inner.core.borrow_mut().flush();
    if let Some(flush) = pending {
        inner.apply_measurements(flush.step);
    }
    // The motion is over: the band closes, bridges earn nothing.
    let ended = inner.core.borrow_mut().note_scroll_end();
    inner.publish_range(ended.range);
    inner.prune_retained_tick();
    // Every correction the gesture outran lands now, one write.
    inner.flush_banked_scroll();
    // The scroller is quiet: the held-back first paints run.
    write_if_changed(inner.settled, true);
    let callbacks: Vec<_> = inner.idle_cbs.borrow().iter().cloned().collect();
    for callback in callbacks {
        callback();
    }
}

/// One `ResizeObserver` and the closure that serves it.
pub(crate) struct ObserverBinding {
    observer: ResizeObserver,
    callback: ObserverCallback,
}

/// One event listener and the closure that serves it.
pub(crate) struct ListenerBinding {
    element: web_sys::Element,
    event: &'static str,
    callback: ListenerCallback,
}

impl VirtualizerInner {
    /// Build the shared state for a fresh virtualizer.
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
        let initial_retention = options.retention;
        Rc::new(Self {
            surface: DomSurface::new(options.axis, options.padding_start),
            scroll_top: RwSignal::new(initial_scroll),
            settled: RwSignal::new(true),
            viewport: RwSignal::new(initial_viewport),
            range: RwSignal::new(initial_range),
            layout_version: RwSignal::new(0),

            last_epoch: Cell::new(initial_epoch),
            last_band_version: Cell::new(0),
            options,
            core: RefCell::new(core),
            pending_scroll: Rc::new(Cell::new(None)),
            scroll_armed: Rc::new(Cell::new(false)),
            flush_armed: Rc::new(Cell::new(false)),
            now_flush_armed: Rc::new(Cell::new(false)),
            banked_scroll: Cell::new(0.0),
            scroll_feedback: Cell::new(true),
            container_ro: RefCell::new(None),
            listeners: RefCell::new(Vec::new()),
            scroll_end_timer: RefCell::new(None),
            retained: RefCell::new(Vec::new()),
            retained_version: RwSignal::new(0),
            retention: Cell::new(initial_retention),
            retention_timer: RefCell::new(None),
            frame_clock: Cell::new(0),
            frames_armed: Cell::new(false),
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
    /// Whether the scroller has been quiet for the scroll-end delay.
    pub settled: RwSignal<bool>,
    pub viewport: RwSignal<Viewport>,
    pub range: RwSignal<Option<Window>>,
    pub layout_version: RwSignal<u64>,
    pub last_epoch: Cell<u64>,
    /// The core's band version as last published.
    pub last_band_version: Cell<u64>,

    pub pending_scroll: Rc<Cell<Option<f64>>>,
    pub scroll_armed: Rc<Cell<bool>>,
    pub flush_armed: Rc<Cell<bool>>,

    /// The pre-paint flush's once-per-batch latch.
    pub now_flush_armed: Rc<Cell<bool>>,

    /// The offset the DOM holds that the core has already corrected past: the
    /// anchor correction a still-running gesture has not taken yet.
    pub banked_scroll: Cell<f64>,

    /// While false, the DOM scroll echo must not touch the core.
    pub scroll_feedback: Cell<bool>,

    pub container_ro: RefCell<Option<ObserverBinding>>,
    pub listeners: RefCell<Vec<ListenerBinding>>,
    pub scroll_end_timer: RefCell<Option<TimeoutHandle>>,

    /// Zombie retention bookkeeping and the two wakers that end a bridge.
    pub retained: RefCell<Vec<RetainedItem>>,
    pub retained_version: RwSignal<u64>,
    pub retention: Cell<RetentionPolicy>,
    pub retention_timer: RefCell<Option<TimeoutHandle>>,
    /// The frame counter a frame-counted bridge is measured against.
    pub frame_clock: Cell<u64>,
    /// Whether the frame chain is already queued (one chain at a time).
    pub frames_armed: Cell<bool>,

    pub idle_cbs: RefCell<Vec<IdleCallback>>,

    pub items_signal: OnceCell<Signal<Vec<VirtualItem>, LocalStorage>>,
    pub rows_signal: OnceCell<Signal<Vec<VirtualRow>, LocalStorage>>,
    pub total_signal: OnceCell<Signal<f64, LocalStorage>>,
    pub dominant_signal: OnceCell<Signal<usize, LocalStorage>>,
}

impl VirtualizerInner {
    /// Republish the items when the band moved inside an unchanged window.
    fn sync_band_version(self: &Rc<Self>) {
        let version = self.core.borrow().band_version();
        if self.last_band_version.replace(version) == version {
            return;
        }
        self.retained_version.update(|v| *v += 1);
    }

    /// Publish a new mount window and schedule retention for the evicted.
    fn publish_range(self: &Rc<Self>, new: Option<Window>) {
        self.sync_band_version();
        let Some(old) = self.range.try_get_untracked() else {
            return;
        };
        if old == new {
            return;
        }
        let policy = self.retention.get();
        // A motion-gated bridge is granted by the seek; others ignore it.
        let seeking = self.core.borrow().motion_engaged();
        if policy.bridges() {
            let now = now_ms();
            let frame = self.frame_clock.get();
            let evicted = retain_evicted(old, new, now, frame, &policy, seeking);
            if !evicted.is_empty() {
                let mut retained = self.retained.borrow_mut();
                // Merge: a retained index keeps its expiry.
                *retained = prune_retained(std::mem::take(&mut *retained), new, now, frame);
                for item in evicted {
                    if !retained.iter().any(|r| r.index == item.index) {
                        retained.push(item);
                    }
                }
                let max = policy.max();
                if retained.len() > max {
                    let drop = retained.len() - max;
                    retained.drain(0..drop);
                }
                drop(retained);
                self.retained_version.update(|v| *v += 1);
                self.arm_retention_clocks();
            }
        }
        self.range.set(new);
    }

    /// Run one prune for both wakers and publish when it cost something.
    fn prune_retained_tick(self: &Rc<Self>) {
        let now = now_ms();
        let frame = self.frame_clock.get();
        let active = self.core.borrow().range();
        let mut retained = self.retained.borrow_mut();
        let before = retained.len();
        *retained = prune_retained(std::mem::take(&mut *retained), active, now, frame);
        let changed = before != retained.len();
        drop(retained);
        if changed {
            self.retained_version.update(|v| *v += 1);
        }
    }

    /// Arm both wakers a live bridge has.
    fn arm_retention_clocks(self: &Rc<Self>) {
        self.arm_retention_timer();
        self.arm_retention_frames();
    }

    /// Arm the timer that prunes expired zombies.
    fn arm_retention_timer(self: &Rc<Self>) {
        if self.retention_timer.borrow().is_some() {
            return;
        }
        let inner = self.clone();
        if let Ok(handle) = set_timeout_with_handle(
            move || {
                // Flush-time fire: the owner's signals may already be purged.
                if inner.settled.try_get_untracked().is_none() {
                    return;
                }
                inner.retention_timer.borrow_mut().take();
                inner.prune_retained_tick();
                if !inner.retained.borrow().is_empty() {
                    inner.arm_retention_timer();
                }
            },
            Duration::from_millis(next_deadline_ms(&self.retained.borrow(), now_ms())),
        ) {
            *self.retention_timer.borrow_mut() = Some(handle);
        }
    }

    /// Whether any live bridge still owes a frame tick.
    fn waits_on_frames(self: &Rc<Self>) -> bool {
        let frame = self.frame_clock.get();
        self.retained
            .borrow()
            .iter()
            .any(|item| item.frame_limit != u64::MAX && item.frame_limit > frame)
    }

    /// Advance the frame clock while a `Frames` bridge is alive.
    fn arm_retention_frames(self: &Rc<Self>) {
        if self.frames_armed.get() || !self.waits_on_frames() {
            return;
        }
        self.frames_armed.set(true);
        let inner = self.clone();
        raf(move || {
            // The same purge window the timer guards.
            if inner.settled.try_get_untracked().is_none() {
                return;
            }
            inner.frames_armed.set(false);
            inner.frame_clock.set(inner.frame_clock.get() + 1);
            inner.prune_retained_tick();
            if inner.waits_on_frames() {
                inner.arm_retention_frames();
            }
        });
    }

    /// The indices a live bridge keeps mounted.
    fn bridged_indices(self: &Rc<Self>) -> Vec<usize> {
        let now = now_ms();
        let frame = self.frame_clock.get();
        let window = self.core.borrow().range();
        self.retained
            .borrow()
            .iter()
            .filter(|item| item.alive(now, frame))
            .map(|item| item.index)
            .filter(|index| {
                window
                    .map(|w| *index < w.first || *index > w.last)
                    .unwrap_or(false)
            })
            .collect()
    }

    pub(crate) fn apply(self: &Rc<Self>, step: Step) {
        self.apply_step(step, true);
    }

    /// Publish a measurement flush: the layout now, the correction at the
    /// settle.
    fn apply_measurements(self: &Rc<Self>, step: Step) {
        self.apply_step(step, self.settled.try_get_untracked() == Some(true));
    }

    fn apply_step(self: &Rc<Self>, step: Step, write_scroll: bool) {
        // These writes wake cross-subscribed effects; settled is the liveness
        // probe.
        if self.settled.try_get_untracked().is_none() {
            return;
        }
        if step.layout_changed {
            self.layout_version.update(|version| *version += 1);
        }
        self.publish_range(step.range);
        let Some(top) = step.scroll_write else {
            return;
        };
        if write_scroll {
            self.commit_scroll(top);
            return;
        }
        // A gesture owns the surface: the layout lands now and the scroll
        // correction waits for the settle, exactly once per correction.
        let banked = self.banked_scroll.get() + step.scroll_delta;
        self.banked_scroll.set(banked);
    }

    /// Land one offset on the DOM, the signal and the core together.
    fn commit_scroll(self: &Rc<Self>, top: f64) {
        self.banked_scroll.set(0.0);
        self.surface.set_scroll(top, false);
        write_if_changed(self.scroll_top, top);
    }

    /// Apply every banked correction as one write, at the settle.
    fn flush_banked_scroll(self: &Rc<Self>) {
        let banked = self.banked_scroll.replace(0.0);
        if banked.abs() <= self.options.measure_epsilon {
            return;
        }
        let Some(top) = self.scroll_top.try_get_untracked() else {
            return;
        };
        let target = (top + banked).max(0.0);
        self.commit_scroll(target);
        // The core was left at the pre-correction offset; its window has to
        // follow the offset the scroller now holds.
        let step = self.core.borrow_mut().adopt_offset(target);
        self.publish_range(step.range);
    }

    /// Apply a step a scroll command produced: no second DOM write.
    fn apply_local(self: &Rc<Self>, step: Step) {
        if self.settled.try_get_untracked().is_none() {
            return;
        }
        if step.layout_changed {
            self.layout_version.update(|version| *version += 1);
        }
        self.publish_range(step.range);
        // The core already moved; the command owns the DOM write.
        self.banked_scroll.set(0.0);
        write_if_changed(self.scroll_top, self.core.borrow().scroll_top());
    }

    fn handle_scroll(self: &Rc<Self>, dom_top: f64) {
        // Flush-time callers arrive with the signals already gone.
        if self.settled.try_get_untracked().is_none() {
            return;
        }
        if !self.scroll_feedback.get() {
            // A programmatic gesture owns the surface: the echo is stale.
            return;
        }
        let content = dom_top - self.options.padding_start;
        // The signal is the scroll offset every consumer reads, so it is
        // synced even when the core's geometry does not have to move.
        write_if_changed(self.scroll_top, content);
        // Sub-epsilon echo of a correction this adapter wrote: the core's
        // geometry does not have to move.
        if (content - self.core.borrow().scroll_top()).abs() <= self.options.measure_epsilon {
            return;
        }
        let step = self.core.borrow_mut().on_scroll_at(content, now_ms());
        // The strip is moving again: first paints wait for the settle.
        write_if_changed(self.settled, false);
        self.publish_range(step.range);
        if let Some(top) = step.scroll_write {
            self.commit_scroll(top);
        }
        self.arm_scroll_end();
    }

    fn handle_viewport(self: &Rc<Self>, vp: Viewport) {
        if self.viewport.try_get_untracked().is_none() {
            return;
        }
        let current = self.viewport.get_untracked();
        let eps = self.options.measure_epsilon;
        if (vp.main - current.main).abs() <= eps && (vp.cross - current.cross).abs() <= eps {
            return;
        }
        self.viewport.set(vp);
        let step = self.core.borrow_mut().on_viewport(vp);
        self.apply(step);
    }

    fn arm_flush(self: &Rc<Self>) {
        if self.flush_armed.get() || self.core.borrow().suspended() {
            return;
        }
        self.flush_armed.set(true);
        let inner = self.clone();
        raf(move || {
            // Same dispose window as the scroll rAF.
            if inner.surface.element().is_none() || inner.settled.try_get_untracked().is_none() {
                inner.flush_armed.set(false);
                return;
            }
            inner.flush_armed.set(false);
            let flush = inner.core.borrow_mut().flush();
            if let Some(flush) = flush {
                inner.apply_measurements(flush.step);
            }
        });
    }

    /// Land the queued measurements before the paint, together.
    fn arm_now_flush(self: &Rc<Self>) {
        if self.now_flush_armed.get() {
            return;
        }
        self.now_flush_armed.set(true);
        let inner = self.clone();
        queue_microtask(move || {
            // Same dispose window as the other armed flushes.
            inner.now_flush_armed.set(false);
            if inner.settled.try_get_untracked().is_none() {
                return;
            }
            let flush = inner.core.borrow_mut().flush();
            if let Some(flush) = flush {
                inner.apply_measurements(flush.step);
            }
        });
    }

    fn arm_scroll_end(self: &Rc<Self>) {
        if let Some(handle) = self.scroll_end_timer.borrow_mut().take() {
            handle.clear();
        }
        let inner = self.clone();
        let delay = Duration::from_millis(self.options.scroll_end_delay_ms as u64);
        if let Ok(handle) = set_timeout_with_handle(move || settle(&inner), delay) {
            *self.scroll_end_timer.borrow_mut() = Some(handle);
        }
    }

    /// Run the settle transaction now, on a native `scrollend`.
    fn settle_now(self: &Rc<Self>) {
        if let Some(handle) = self.scroll_end_timer.borrow_mut().take() {
            handle.clear();
        }
        settle(self);
    }

    /// Teardown, idempotent, callable by the handle or the component.
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

    /// Drop the bound container's listeners and observers.
    fn teardown_bindings(&self) {
        for binding in self.listeners.borrow_mut().drain(..) {
            let _ = binding.element.remove_event_listener_with_callback(
                binding.event,
                binding.callback.as_ref().unchecked_ref(),
            );
        }
        if let Some(binding) = self.container_ro.borrow_mut().take() {
            let ObserverBinding { observer, callback } = binding;
            observer.disconnect();
            // Release the closure AFTER the observer is dead.
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
    pub(crate) fn from_inner(inner: Rc<VirtualizerInner>) -> Self {
        Self { inner }
    }
}

/// Handle identity: equal when they wrap the same inner virtualizer.
impl PartialEq for Virtualizer {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

fn write_if_changed<T>(signal: RwSignal<T>, value: T)
where
    T: PartialEq + Copy + Send + Sync + 'static,
{
    // `try_get` keeps flush-time callers honest.
    match signal.try_get_untracked() {
        Some(current) if current != value => signal.set(value),
        Some(_) => {}
        None => {}
    }
}

/// The retention clock, in milliseconds, from `performance.now()`.
fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or_else(js_sys::Date::now)
}

fn dom_scroll_offset(el: &web_sys::HtmlElement, axis: crate::options::Axis) -> f64 {
    match axis {
        crate::options::Axis::Vertical => el.scroll_top() as f64,
        crate::options::Axis::Horizontal => el.scroll_left() as f64,
    }
}

/// Whether this engine ends a gesture by itself or needs the debounce.
fn supports_scroll_end() -> bool {
    web_sys::window().is_some_and(|w| {
        js_sys::Reflect::has(w.as_ref(), &wasm_bindgen::JsValue::from_str("onscrollend"))
            .unwrap_or(false)
    })
}

impl Virtualizer {
    /// Explicit teardown by the resource owner, idempotent.
    pub fn dispose(&self) {
        self.inner.dispose();
    }

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

        // A container that binds long after the core was built would
        // otherwise have its first scroll sample measured against the clock's
        // origin: the reader's whole session in the document looks like one
        // very slow gesture.
        let seeded = inner.core.borrow().scroll_top();
        inner.core.borrow_mut().seed_motion(seeded, now_ms());

        {
            let inner_for_listener = inner.clone();
            let closure = Closure::<dyn FnMut(Event)>::new(move |_| {
                // The listener unbinds in dispose(), after the signals go.
                if inner_for_listener.settled.try_get_untracked().is_none() {
                    return;
                }
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
                        // The frame is untracked: a detached surface is gone.
                        if inner2.surface.element().is_none()
                            || inner2.settled.try_get_untracked().is_none()
                        {
                            return;
                        }
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

        if supports_scroll_end() {
            // The browser's own "this gesture is over" beats a fixed delay:
            // a long fling stops paying 150 ms of latency on every settle.
            // The timer stays armed as the fallback for engines without it.
            let inner_for_end = inner.clone();
            let closure = Closure::<dyn FnMut(Event)>::new(move |_| {
                if inner_for_end.settled.try_get_untracked().is_none() {
                    return;
                }
                inner_for_end.settle_now();
            });
            let _ =
                el.add_event_listener_with_callback("scrollend", closure.as_ref().unchecked_ref());
            inner.listeners.borrow_mut().push(ListenerBinding {
                element: el.clone(),
                event: "scrollend",
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
                // Zombies: retained, unexpired, outside the window.
                let retained = inner.bridged_indices();
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

    /// Reactive mounted rows, bridged rows included.
    pub fn rows(&self) -> Signal<Vec<VirtualRow>, LocalStorage> {
        *self.inner.rows_signal.get_or_init(|| {
            let inner = self.inner.clone();
            Signal::derive_local(move || {
                let _ = inner.range.get();
                let _ = inner.layout_version.get();
                let _ = inner.retained_version.get();
                let mut rows = inner.core.borrow().rows();
                // A grid bridges rows, not items.
                for index in inner.bridged_indices() {
                    let row = inner.core.borrow().row_at(index);
                    if !rows.iter().any(|mounted| mounted.row == row.row) {
                        rows.push(row);
                    }
                }
                rows.sort_by_key(|row| row.row);
                rows
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

    /// Whether the scroller has settled.
    pub fn settled(&self) -> ReadSignal<bool> {
        self.inner.settled.read_only()
    }

    /// [`settled`](Self::settled) as a plain, panic-free question.
    pub fn settled_now(&self) -> bool {
        self.inner.settled.try_get_untracked().unwrap_or(false)
    }

    /// The viewport signal.
    pub fn viewport(&self) -> RwSignal<Viewport> {
        self.inner.viewport
    }

    /// Whether a scroll container is currently bound.
    pub fn is_bound(&self) -> bool {
        self.inner.surface.element().is_some()
    }

    /// The bound container's scroll offset as the DOM reports it.
    pub fn surface_offset(&self) -> Option<f64> {
        let el = self.inner.surface.element()?;
        let html = el.dyn_into::<web_sys::HtmlElement>().ok()?;
        Some(dom_scroll_offset(&html, self.inner.options.axis) - self.inner.options.padding_start)
    }

    /// Re-read the container's viewport extent only.
    pub fn remeasure_viewport(&self) {
        let Some(el) = self.inner.surface.element() else {
            return;
        };
        let vp = viewport_of(&el, self.inner.options.axis);
        self.inner.handle_viewport(vp);
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
            let step = self.inner.core.borrow_mut().on_scroll_at(content, now_ms());
            write_if_changed(self.inner.scroll_top, content);
            self.inner.publish_range(step.range);
        }
    }

    /// Snapshot offset of an item, padding included.
    pub fn offset_of(&self, index: usize) -> f64 {
        self.inner.core.borrow().offset_of(index)
    }

    /// Index of the item whose span contains `pos`, by leading edge.
    pub fn index_at(&self, pos: f64) -> usize {
        self.inner.core.borrow().index_at(pos)
    }

    /// Reactive main-axis offset of one item, padding included.
    pub fn item_top(&self, index: usize) -> Signal<f64, LocalStorage> {
        let inner = self.inner.clone();
        Signal::derive_local(move || {
            let _ = inner.layout_version.get();
            inner.core.borrow().offset_of(index)
        })
    }

    /// Reactive main-axis extent of one item.
    pub fn item_size(&self, index: usize) -> Signal<f64, LocalStorage> {
        let inner = self.inner.clone();
        Signal::derive_local(move || {
            let _ = inner.layout_version.get();
            inner.core.borrow().layout().size(index)
        })
    }

    /// Reactive render state of one mounted item.
    pub fn item_state(&self, index: usize) -> Signal<VirtualItemState, LocalStorage> {
        let inner = self.inner.clone();
        Signal::derive_local(move || {
            let _ = inner.viewport.get();
            let _ = inner.range.get();
            let _ = inner.layout_version.get();
            let _ = inner.scroll_top.get();
            let _ = inner.retained_version.get();
            let in_window = inner
                .core
                .borrow()
                .range()
                .map(|window| window.contains(index))
                .unwrap_or(false);
            if !in_window {
                // Retention is the adapter's clock: a zombie outranks the band.
                let frame = inner.frame_clock.get();
                if is_retained(&inner.retained.borrow(), index, now_ms(), frame) {
                    return VirtualItemState::Zombie;
                }
            }
            inner.core.borrow().item_state(index)
        })
    }

    /// Scroll to an absolute content offset.
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
        let step =
            self.inner
                .core
                .borrow_mut()
                .scroll_to_index(index, align, mode, &self.inner.surface);
        if let Some(step) = step {
            self.inner.apply_local(step);
        }
    }

    /// Report a size directly.
    pub fn report_size(&self, index: usize, size: f64) {
        // Belt-and-braces: a report outliving the owner queues nothing.
        if self.inner.settled.try_get_untracked().is_none() {
            return;
        }
        self.inner.core.borrow_mut().queue_size(index, size);
        self.inner.arm_flush();
    }

    /// [`report_size`](Self::report_size) applied in the frame it is
    /// reported in.
    pub fn report_size_now(&self, index: usize, size: f64) {
        if self.inner.settled.try_get_untracked().is_none() {
            return;
        }
        self.inner.core.borrow_mut().queue_size(index, size);
        self.inner.arm_now_flush();
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

    /// Ignore the DOM scroll echo until [`resume_scroll_feedback`].
    pub fn suspend_scroll_feedback(&self) {
        self.inner.scroll_feedback.set(false);
    }

    /// Re-adopt the DOM scroll echo (see [`suspend_scroll_feedback`]).
    pub fn resume_scroll_feedback(&self) {
        self.inner.scroll_feedback.set(true);
    }

    /// Choose how evicted items are retired.
    pub fn set_retention_policy(&self, policy: RetentionPolicy) {
        self.inner.retention.set(policy);
    }

    /// Return the retirement policy to the one this virtualizer was built
    /// with (see [`Self::set_retention_policy`]).
    pub fn reset_retention_policy(&self) {
        self.inner.retention.set(self.inner.options.retention);
    }

    /// Every bridge ends THIS tick: the hard endpoint.
    pub fn remove_retained_now(&self) {
        if self.inner.retained.borrow().is_empty() {
            return;
        }
        self.inner.retained.borrow_mut().clear();
        self.inner.retained_version.update(|v| *v += 1);
    }

    /// Zoom: rescale every size with the viewport center pinned.
    pub fn rescale(&self, factor: f64, new_sizes: impl Fn(usize) -> f64) {
        let step = self.inner.core.borrow_mut().rescale(factor, &new_sizes);
        self.inner.apply(step);
    }

    /// [`Self::rescale`] without touching the surface.
    pub fn rescale_detached(&self, factor: f64, new_sizes: impl Fn(usize) -> f64) {
        let step = self.inner.core.borrow_mut().rescale(factor, &new_sizes);
        self.inner.apply_local(step);
    }

    /// Report one item's fill cost and lane count.
    pub fn set_fill_profile(&self, fill_ms: f64, lanes: usize) {
        let mut pipeline = self.inner.core.borrow().pipeline();
        pipeline.fill_ms = fill_ms.max(0.0);
        pipeline.lanes = lanes;
        self.inner.core.borrow_mut().set_pipeline(pipeline);
        // The band can open or close without the window moving.
        self.inner.sync_band_version();
    }

    /// Whether the current scroll outruns the reported pipeline.
    pub fn motion_engaged(&self) -> bool {
        self.inner.core.borrow().motion_engaged()
    }

    /// Where a mounted index ranks in the fill order right now.
    pub fn fill_priority(&self, index: usize) -> virtual_list::FillPriority {
        self.inner.core.borrow().fill_priority(index)
    }

    /// The index the viewport reaches by the time the fill finishes.
    pub fn landing_index(&self) -> usize {
        self.inner.core.borrow().landing_index()
    }

    /// Called when scrolling settles.
    pub fn on_scroll_idle(&self, cb: impl Fn() + 'static) {
        self.inner.idle_cbs.borrow_mut().push(Rc::new(cb));
    }

    /// How many items the live window mounts, for diagnostics.
    pub fn live_window_items(&self) -> usize {
        match self.inner.core.borrow().range() {
            Some(window) => window.last.saturating_sub(window.first) + 1,
            None => 0,
        }
    }

    /// How many evicted items the retention grace still keeps.
    pub fn retained_items(&self) -> usize {
        self.inner.retained.borrow().len()
    }

    /// Live DOM and event bookkeeping counts, for diagnostics.
    pub fn listener_bindings(&self) -> usize {
        self.inner.listeners.borrow().len()
    }

    /// How many `ResizeObserver` bindings are held.
    pub fn observer_bindings(&self) -> usize {
        usize::from(self.inner.container_ro.borrow().is_some())
    }

    /// How many timers are armed right now (diagnostics): the scroll-end
    /// debounce plus the zombie-retention expiry.
    pub fn armed_timers(&self) -> usize {
        usize::from(self.inner.scroll_end_timer.borrow().is_some())
            + usize::from(self.inner.retention_timer.borrow().is_some())
    }
}
