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

use virtual_list::{Align, Layout, ReleaseLedger, ReleaseReason, Viewport, Window, release_sides};

use crate::engine::{Step, VirtualizerCore};
use crate::observe::{raf, viewport_of};
use crate::options::{ScrollMode, VirtualizerOptions};
use crate::render::{VirtualItem, VirtualItemState, VirtualRow};
use crate::retention::{
    RELEASE_LEDGER_CAPACITY, REVERSAL_GRACE_ITEMS, RetainedItem, RetentionPolicy, is_retained,
    next_deadline_ms, prune_retained, retain_evicted,
};

/// The speed below which a reader is not drifting, in pixels per second — well
/// under one tenth of a pixel in a frame. [`Virtualizer::motion_drifts`] is the
/// answer an effect can sample against without inventing its own threshold.
use crate::surface::{DomSurface, ScrollSurface};

type ObserverCallback = Closure<dyn FnMut(js_sys::Array, ResizeObserver)>;
type ListenerCallback = Closure<dyn FnMut(Event)>;
type IdleCallback = Rc<dyn Fn()>;
type ReleaseCallback = Rc<dyn Fn(usize, virtual_list::ReleaseReason)>;

/// The speed below which the reader is not drifting, pixels per second (well
/// under a tenth of a pixel in a frame). [`Virtualizer::motion_drifts`] is the
/// answer an effect samples against instead of inventing its own threshold.
pub const DRIFT_EPS_PX_S: f64 = 20.0;

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
            release: RefCell::new(ReleaseLedger::new(RELEASE_LEDGER_CAPACITY)),
            release_cbs: RefCell::new(Vec::new()),
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
    /// Whether the scroller has been quiet for `scroll_end_delay_ms` — the
    /// scroll-end timer's own notion of "the scroll ended", published. True
    /// until the first real scroll moves it; the timer `arm_scroll_end`
    /// re-arms on every scroll event restores it. Page strips gate a swept-in
    /// page's FIRST paint on it, so a fling pays a handful of rasterisations
    /// at its end instead of one per page it flew past.
    pub settled: RwSignal<bool>,
    pub viewport: RwSignal<Viewport>,
    pub range: RwSignal<Option<Window>>,
    pub layout_version: RwSignal<u64>,
    pub last_epoch: Cell<u64>,
    /// The core's band version as last published. A band change without a
    /// window change is invisible to `range`, so this is what makes the items
    /// (and the pages' Active/Blank state they carry) republish.
    pub last_band_version: Cell<u64>,

    pub pending_scroll: Rc<Cell<Option<f64>>>,
    pub scroll_armed: Rc<Cell<bool>>,
    pub flush_armed: Rc<Cell<bool>>,

    /// The pre-paint flush's once-per-batch latch (see
    /// [`VirtualizerInner::arm_now_flush`]). Separate from `flush_armed`: the
    /// two are armed by different reports and land at different checkpoints,
    /// so a batch that armed both must lose neither.
    pub now_flush_armed: Rc<Cell<bool>>,

    /// Measured sizes the scroller has not been told about: the anchored
    /// scroll corrections a MEASUREMENT flush produced while the reader was
    /// moving. The layout half of such a flush always lands (a stale model
    /// paints rows on top of each other); the correction is the half that
    /// fights momentum, so it is banked here and applied as ONE write when the
    /// scroll-end window closes. The window always comes — it is what made
    /// `settled` false — so this always drains.
    pub banked_scroll: Cell<f64>,

    /// While false, the DOM scroll echo must not touch the core. A
    /// programmatic scroll burst (zoom tween, sidebar slide, resize drag)
    /// writes the surface every frame and the browser echoes a frame late, so
    /// feeding the echo back would overwrite the core's anchor with a stale
    /// value and the next anchored rescale would oscillate — visible jitter
    /// during the animation. The app flips this off for the gesture and back
    /// on at its commit.
    pub scroll_feedback: Cell<bool>,

    pub container_ro: RefCell<Option<ObserverBinding>>,
    pub listeners: RefCell<Vec<ListenerBinding>>,
    pub scroll_end_timer: RefCell<Option<TimeoutHandle>>,

    /// Zombie retention bookkeeping (see `retention.rs`): evicted items
    /// still mounted, the reactive version that items() tracks, the currently
    /// effective policy (raised around a zoom commit), and the two wakers a
    /// bridge is released by: the expiry timer, and — for a policy counted in
    /// frames — the animation-frame chain.
    pub retained: RefCell<Vec<RetainedItem>>,
    pub retained_version: RwSignal<u64>,
    pub retention: Cell<RetentionPolicy>,
    pub retention_timer: RefCell<Option<TimeoutHandle>>,
    /// The adapter's frame counter: advanced once per rAF while a bridge is
    /// alive, and the only clock a `Frames` policy is measured against.
    pub frame_clock: Cell<u64>,
    /// Whether the frame chain is already queued (one chain at a time).
    pub frames_armed: Cell<bool>,

    pub idle_cbs: RefCell<Vec<IdleCallback>>,
    /// Releases the window moves have earned but no subscriber has taken yet
    /// (bounded, deduped by index), and the subscribers themselves. Content
    /// caches register through [`Virtualizer::on_release`]; a pane that mounts
    /// after a jump still gets the queue, so a bitmap cannot be stranded by the
    /// order the effects happened to run in.
    pub release: RefCell<ReleaseLedger>,
    pub release_cbs: RefCell<Vec<ReleaseCallback>>,

    pub items_signal: OnceCell<Signal<Vec<VirtualItem>, LocalStorage>>,
    pub rows_signal: OnceCell<Signal<Vec<VirtualRow>, LocalStorage>>,
    pub total_signal: OnceCell<Signal<f64, LocalStorage>>,
    pub dominant_signal: OnceCell<Signal<usize, LocalStorage>>,
}

impl VirtualizerInner {
    /// Republish the item list when the render band moved inside an unchanged
    /// window. The lead grows and shrinks with the measured scroll, so this is
    /// the path by which a page entering the band starts carrying content
    /// without waiting for a window move or a timer.
    fn sync_band_version(self: &Rc<Self>) {
        let version = self.core.borrow().band_version();
        if self.last_band_version.replace(version) == version {
            return;
        }
        self.retained_version.update(|v| *v += 1);
    }

    /// Hand the content caches the indices this window move left behind past
    /// [`REVERSAL_GRACE_ITEMS`], on the same tick the row unmounts.
    ///
    /// What this says to drop is a page's *content* — its rasters, canvases and
    /// snapshots. The item's measurement stays with the layout for as long as
    /// the layout keeps the item: the scrollbar and every anchor depend on it,
    /// and it costs 16 bytes. That split is the whole memory argument, and it
    /// is why an evicted row is not the end of a page's story while it is still
    /// near the window, and is the end of it as soon as it is not.
    fn release_for(self: &Rc<Self>, old: Option<Window>, new: Option<Window>) {
        let (Some(old), Some(new)) = (old, new) else {
            return;
        };
        let (below, above) = release_sides(old, new, REVERSAL_GRACE_ITEMS);
        {
            let mut ledger = self.release.borrow_mut();
            for side in below.into_iter().chain(above) {
                for index in side.iter() {
                    ledger.push(index, ReleaseReason::Evicted);
                }
            }
        }
        self.flush_releases();
    }

    /// Deliver everything pending, if anybody is listening. With no subscriber
    /// the ledger simply holds (bounded), which is what lets a release precede
    /// the effect that registers for it.
    fn flush_releases(self: &Rc<Self>) {
        if self.release_cbs.borrow().is_empty() {
            return;
        }
        let pending = self.release.borrow_mut().drain();
        if pending.is_empty() {
            return;
        }
        let callbacks: Vec<_> = self.release_cbs.borrow().iter().cloned().collect();
        for (index, reason) in pending {
            for callback in &callbacks {
                callback(index, reason);
            }
        }
    }

    /// Publish a new mount window: write-if-changed, and schedule zombie
    /// retention for the items the change evicted. Every range write in the
    /// adapter funnels through here so retention cannot miss a transition.
    fn publish_range(self: &Rc<Self>, new: Option<Window>) {
        self.sync_band_version();
        let Some(old) = self.range.try_get_untracked() else {
            return;
        };
        self.release_for(old, new);
        if old == new {
            return;
        }
        let policy = self.retention.get();
        // A motion-gated bridge is granted by the seek that moved the window;
        // every other policy ignores the verdict.
        let seeking = self.core.borrow().motion_engaged();
        if policy.bridges() {
            let now = now_ms();
            let frame = self.frame_clock.get();
            let evicted = retain_evicted(old, new, now, frame, &policy, seeking);
            if !evicted.is_empty() {
                let mut retained = self.retained.borrow_mut();
                // Merge: an index already retained keeps its original expiry
                // only if it is still outside the new window; re-entry drops it.
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

    /// Run one prune for both wakers: drop every bridge that has expired or
    /// come back inside the window, and publish the change when it cost
    /// something. `retained` is the state; the wakers only decide WHEN it is
    /// asked again, so a bridge can never be released by one clock and
    /// forgotten by the other.
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

    /// Arm both wakers a live bridge has. Each one is a no-op when nothing
    /// is bridged in its unit, so a millisecond grace never queues frames and
    /// a frame bridge never waits on a timer it does not need.
    fn arm_retention_clocks(self: &Rc<Self>) {
        self.arm_retention_timer();
        self.arm_retention_frames();
    }

    /// Arm (once) the timer that prunes expired zombies. Re-arms itself
    /// while anything is still retained: it holds every bridge's wall-clock
    /// deadline, including the ceiling a frame-counted one may not pass.
    fn arm_retention_timer(self: &Rc<Self>) {
        if self.retention_timer.borrow().is_some() {
            return;
        }
        let inner = self.clone();
        if let Ok(handle) = set_timeout_with_handle(
            move || {
                // Flush-time fire: the owner's signals may already be
                // purged (dispose runs later than the purge) — the retained
                // write below belongs to a living reader only.
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

    /// Whether any live bridge still owes a frame tick. A `Grace` item's
    /// limit is `u64::MAX`, so this is exactly "a frame bridge is pending" —
    /// and when it reads false the chain stops, leaving no rAF callback
    /// registered against an idle list.
    fn waits_on_frames(self: &Rc<Self>) -> bool {
        let frame = self.frame_clock.get();
        self.retained
            .borrow()
            .iter()
            .any(|item| item.frame_limit != u64::MAX && item.frame_limit > frame)
    }

    /// Advance the frame clock one animation frame at a time while a
    /// `Frames` bridge is alive. Frames, not milliseconds, are what the
    /// bridge was bought for: a stalled renderer and a smooth one both spend
    /// the same number of them, and the chain ends with the bridge.
    fn arm_retention_frames(self: &Rc<Self>) {
        if self.frames_armed.get() || !self.waits_on_frames() {
            return;
        }
        self.frames_armed.set(true);
        let inner = self.clone();
        raf(move || {
            // The same purge window the timer guards: a disposed owner has
            // no signals left to publish into, and no bridge to keep.
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

    /// The indices a live bridge keeps mounted: unexpired and outside the
    /// active window (inside it, an item is simply active).
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

    /// Publish a MEASUREMENT flush: the layout lands NOW, the scroll
    /// correction waits for the reader to stop.
    ///
    /// The two halves of a flush pull in opposite directions. A size the
    /// model has not learned yet is painted where the model says, so a row
    /// whose own content is taller than its slot renders on top of its
    /// neighbour — the stacked look a stream of text shows for as long as the
    /// model lags, and the reason the measured size cannot wait for a settle.
    /// The anchored scroll correction is the opposite: it is a write under the
    /// reader's finger while momentum owns the scroller, the stutter every
    /// native list avoids. So the correction is banked and one write lands
    /// when the movement ends (see [`Self::flush_banked_scroll`]).
    fn apply_measurements(self: &Rc<Self>, step: Step) {
        self.apply_step(step, self.settled.try_get_untracked() == Some(true));
    }

    fn apply_step(self: &Rc<Self>, step: Step, write_scroll: bool) {
        // The writes below wake cross-subscribed effects; after the owner's
        // purge every one of them belongs to a dead world. `settled` is the
        // scope's liveness probe (same arena as range/scroll_top).
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
        let delta = top - self.scroll_top.get_untracked();
        if delta.abs() <= self.options.measure_epsilon {
            return;
        }
        if !write_scroll {
            // The DOM stays where the reader left it and the core's own
            // offset keeps following the scroll echoes; only the correction is
            // held back, so what lands at the settle is the sum of the ones
            // the movement outran.
            self.banked_scroll.set(self.banked_scroll.get() + delta);
            return;
        }
        self.surface.set_scroll(top, false);
        write_if_changed(self.scroll_top, top);
    }

    /// Apply every banked correction as one write. The scroll-end timer's cue,
    /// and safe to call when nothing is banked.
    ///
    /// The core's offset is deliberately NOT written here: the browser fires a
    /// scroll event for the write and the echo (`handle_scroll`) adopts what
    /// the DOM actually holds — which is also how a write the browser clamped
    /// at either end of the content is told apart from one that landed.
    fn flush_banked_scroll(self: &Rc<Self>) {
        let banked = self.banked_scroll.replace(0.0);
        if banked.abs() <= self.options.measure_epsilon {
            return;
        }
        let Some(top) = self.scroll_top.try_get_untracked() else {
            return;
        };
        self.surface.set_scroll((top + banked).max(0.0), false);
    }

    /// Apply a step produced by a scroll COMMAND (the surface was already
    /// written by the core): signals only, no second DOM write. Instant
    /// writes carry the adopted position; smooth writes return no step and
    /// surface later through `handle_scroll`.
    fn apply_local(self: &Rc<Self>, step: Step) {
        if self.settled.try_get_untracked().is_none() {
            return;
        }
        if step.layout_changed {
            self.layout_version.update(|version| *version += 1);
        }
        self.publish_range(step.range);
        write_if_changed(self.scroll_top, self.core.borrow().scroll_top());
    }

    fn handle_scroll(self: &Rc<Self>, dom_top: f64) {
        // Flush-time callers arrive with the owner's signals already gone
        // (dispose runs later than the purge); a disposed `settled` ends
        // the echo here before any write below.
        if self.settled.try_get_untracked().is_none() {
            return;
        }
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
        if (content - self.core.borrow().scroll_top()).abs() <= self.options.measure_epsilon {
            return;
        }
        let step = self.core.borrow_mut().on_scroll_at(content, now_ms());
        write_if_changed(self.scroll_top, content);
        // The strip is moving again: first paints wait for the scroll-end
        // window this re-arms (see the field docs).
        write_if_changed(self.settled, false);
        self.publish_range(step.range);
        if let Some(top) = step.scroll_write {
            self.surface.set_scroll(top, false);
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
            // Same dispose window as the scroll rAF: the frame can be the
            // first thing that runs after the reader went away.
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

    /// Land the queued measurements before the frame paints, TOGETHER: the
    /// pre-paint half of [`Self::arm_flush`], for reports the browser has
    /// already batched — one `ResizeObserver` notification, or one measure
    /// pass over the mounted window.
    ///
    /// The first report arms the latch and every report beside it rides along,
    /// so a window of resized rows costs ONE layout rebuild instead of one per
    /// row. The vehicle is the microtask checkpoint, which runs as soon as the
    /// reporting callback returns and still precedes the paint — the guarantee
    /// the synchronous flush gave, without the rebuild per row it also cost.
    fn arm_now_flush(self: &Rc<Self>) {
        if self.now_flush_armed.get() {
            return;
        }
        self.now_flush_armed.set(true);
        let inner = self.clone();
        queue_microtask(move || {
            // Same dispose window as the other armed flushes: this checkpoint
            // can be the first thing that runs after the reader went away.
            // The latch clears FIRST, so a row the new layout resized arms a
            // fresh flush of its own.
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
        if let Ok(handle) = set_timeout_with_handle(
            move || {
                // Disposed while this was pending: the settled write and the
                // idle callbacks belong to a reader that is gone. The signal
                // read doubles as the guard — a disposed `settled` is the
                // flush-time answer to "is this owner still here".
                if inner.surface.element().is_none() || inner.settled.try_get_untracked().is_none()
                {
                    return;
                }
                // The scroller has been quiet for the whole window: the strip
                // is settled, and the first paints its gate held back run now.
                write_if_changed(inner.settled, true);
                // The motion is over: the band closes with it, so every
                // mounted item becomes real content, and a bridge the seek had
                // earned is earning nothing. Both changes are state changes
                // without a range change, so they publish through the version
                // `items()` tracks.
                let step = inner.core.borrow_mut().note_scroll_end();
                inner.publish_range(step.range);
                inner.prune_retained_tick();
                // Every correction the fling outran lands NOW, in the same
                // window the first paints do — one write, against a scroller
                // nobody is moving.
                inner.flush_banked_scroll();
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

    /// Teardown, callable by the handle's dispose (the reader runtime's
    /// sequence) and by the owning component's cleanup. Idempotent:
    /// bindings, timers and observers tear down once.
    pub(crate) fn dispose(&self) {
        self.teardown_bindings();
        // A release closure is the caller's code with the caller's handles: it
        // must not outlive the virtualizer that owns the window they describe.
        self.release_cbs.borrow_mut().clear();
        self.release.borrow_mut().drain();
        if let Some(handle) = self.scroll_end_timer.borrow_mut().take() {
            handle.clear();
        }
        if let Some(handle) = self.retention_timer.borrow_mut().take() {
            handle.clear();
        }
        self.surface.detach();
    }

    /// Drop listeners and observers associated with the bound container.
    /// Removal happens with the SAME closure identity that was added (JS
    /// `removeEventListener` matches by function reference) and the closure
    /// is dropped immediately after — a rebound container never leaks a WASM
    /// closure, and a disposed virtualizer releases every DOM handle.
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
    pub(crate) fn from_inner(inner: Rc<VirtualizerInner>) -> Self {
        Self { inner }
    }
}

/// Handle identity: two handles are equal when they wrap the same inner
/// virtualizer. The app's diagnostics registry uses this to remove exactly
/// the handle that was disposed, so a registry entry can never outlive its
/// owner's cleanup.
impl PartialEq for Virtualizer {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

fn write_if_changed<T>(signal: RwSignal<T>, value: T)
where
    T: PartialEq + Copy + Send + Sync + 'static,
{
    // `try_get` keeps the flush-time callers honest: a disposed signal
    // reads as None and the write into the dead world is skipped instead
    // of panicking.
    match signal.try_get_untracked() {
        Some(current) if current != value => signal.set(value),
        Some(_) => {}
        None => {}
    }
}

/// The retention clock, in milliseconds. `performance.now()` is monotonic
/// and sub-millisecond, so a zombie's `expires_at` is a true duration — a
/// wall-clock NTP step or DST switch cannot stretch or cut a grace period
/// mid-zoom. `Date::now()` is the fallback where `performance` is
/// unavailable. (The pure retention maths in `retention.rs` stays
/// host-testable without this.)
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

impl Virtualizer {
    /// Explicit teardown by the resource OWNER (the reader runtime's
    /// dispose sequence, Phase 1): bindings, observers, timers and the
    /// surface detach, and a late event frame finds nothing attached.
    /// Idempotent — a virtualizer already disposed by its component's
    /// cleanup tears down again as a no-op.
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

        {
            let inner_for_listener = inner.clone();
            let closure = Closure::<dyn FnMut(Event)>::new(move |_| {
                // The listener unbinds in dispose(), which runs one teardown
                // beat after the signals are gone; an event landing in that
                // window must not write into them.
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
                        // The frame is not tracked by dispose() (the closure
                        // is handed to the browser uncancellable): a close or
                        // swap can dispose the virtualizer while this frame
                        // was pending, and every write below would land on a
                        // disposed signal. A detached surface is the
                        // "disposed" bit.
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
                // A grid renders whole rows, so a bridge holds rows rather
                // than items: a rail that flings past a row and comes back
                // finds its own canvases still mounted, not the gap the window
                // change cut out of the list.
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

    /// Whether the scroller has settled: false while scroll events are still
    /// arriving, true once `scroll_end_delay_ms` of quiet has passed. A
    /// virtualized page strip's hosts gate an unpainted page's first render
    /// on it — the fling shows the cached thumbnail while it sweeps past and
    /// the crisp rasterisation lands, paced by the engine's render lane, once
    /// the strip is quiet. Read-only on purpose: the timer that restores it
    /// is the adapter's, not the app's.
    pub fn settled(&self) -> ReadSignal<bool> {
        self.inner.settled.read_only()
    }

    /// [`settled`](Self::settled) as a plain, panic-free question, for callers
    /// that can outlive their owner: a measurement pass scheduled on a rAF or
    /// a scroll-end timer still runs after the strip it belongs to is torn
    /// down, and a dead scroller reads `false` here instead of aborting the
    /// wasm the way a bare read of a disposed signal would.
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

    /// The bound container's scroll offset as the DOM reports it right now,
    /// in content coordinates (padding removed); `None` while unbound. The
    /// core's own offset is what the last command or echo made it; this is
    /// what the browser actually holds, which differs when a write was
    /// clamped against a box not yet laid out. A mount-time anchor compares
    /// the two to know whether it landed.
    pub fn surface_offset(&self) -> Option<f64> {
        let el = self.inner.surface.element()?;
        let html = el.dyn_into::<web_sys::HtmlElement>().ok()?;
        Some(dom_scroll_offset(&html, self.inner.options.axis) - self.inner.options.padding_start)
    }

    /// Re-read the bound container's viewport extent only, leaving the
    /// scroll position to whoever is about to command it. The mount-time
    /// anchor uses this: adopting the DOM offset there would rewindow to a
    /// position the very next call replaces.
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

    /// Reactive main-axis extent of one item.
    ///
    /// Same lifetime rules as [`Self::item_top`]: layout-version only. A
    /// stream's blank placeholders read their height here — the layout's own
    /// number, so a measurement landing under a blank patches one style
    /// attribute instead of waiting for the row to render.
    pub fn item_size(&self, index: usize) -> Signal<f64, LocalStorage> {
        let inner = self.inner.clone();
        Signal::derive_local(move || {
            let _ = inner.layout_version.get();
            inner.core.borrow().layout().size(index)
        })
    }

    /// Reactive render state of one mounted item.
    ///
    /// A `For` child does not re-run when its key persists, so the child
    /// cannot read its state off the [`VirtualItem`] it was handed: a scroll
    /// that crosses the row into or out of the render band would leave the
    /// stale answer in place. This signal re-derives on the things that move
    /// the band and the window — scroll, viewport, range, layout, retention —
    /// and the view rebuilds only when the state itself crosses.
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
                // Retention is the adapter's clock: a zombie is whatever the
                // retained set still holds, and its state outranks the band.
                let frame = inner.frame_clock.get();
                if is_retained(&inner.retained.borrow(), index, now_ms(), frame) {
                    return VirtualItemState::Zombie;
                }
            }
            inner.core.borrow().item_state(index)
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
        // Belt-and-braces on top of the callers' guards: a report that
        // outlived the owner queues a size into a dead flush cycle.
        if self.inner.settled.try_get_untracked().is_none() {
            return;
        }
        self.inner.core.borrow_mut().queue_size(index, size);
        self.inner.arm_flush();
    }

    /// [`report_size`](Self::report_size) applied in the frame it is reported
    /// in, for a caller the browser has already batched — a row's
    /// `ResizeObserver` notification arrives after layout and BEFORE paint, so
    /// a size applied here is in the model by the time the row is painted.
    /// That is the whole difference between a row of text whose real height is
    /// correct in the frame it first paints and one that spends a frame
    /// painted on top of the row below it.
    ///
    /// The size is QUEUED, not flushed: the reports of one browser-delivered
    /// batch land together on a single pre-paint flush
    /// ([`VirtualizerInner::arm_now_flush`]), so sweeping a whole window costs
    /// the layout one rebuild rather than one per row.
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

    /// Choose how the items a window change evicts are retired.
    ///
    /// [`RetentionPolicy::Immediate`](crate::RetentionPolicy::Immediate) ends
    /// a bridge in the tick that evicted it; a policy with a bridge keeps the
    /// item's DOM (and the engine surface behind it) alive for a bounded
    /// moment, so the change that moved the window is never visible. Items
    /// already bridged keep the deadlines they were given: this decides the
    /// NEXT eviction, which is what lets a caller raise a bridge around a
    /// commit and stand it back down after.
    pub fn set_retention_policy(&self, policy: RetentionPolicy) {
        self.inner.retention.set(policy);
    }

    /// Return the retirement policy to the one this virtualizer was built
    /// with (see [`Self::set_retention_policy`]).
    pub fn reset_retention_policy(&self) {
        self.inner.retention.set(self.inner.options.retention);
    }

    /// End every bridge whose clock has run out, publishing the change so its
    /// DOM (and the engine surface it holds) unmounts: the soft endpoint, and
    /// what the armed wakers do on their own tick.
    ///
    /// The wakers make this unnecessary in normal operation. It exists because
    /// they are per-eviction bookkeeping on the item's owner, so a zombie
    /// bridged around a zoom can outlive the transaction that raised its grace
    /// and sit on a large (recently zoomed) bitmap until the window moves. A
    /// caller that knows the change is over calls this instead of waiting for
    /// the next scroll.
    pub fn kill_retained(&self) {
        self.inner.prune_retained_tick();
    }

    /// [`Self::kill_retained`] without the clock: every bridge ends THIS tick,
    /// however much of it is left. The hard endpoint, for a caller that has
    /// stopped needing the pixels a bridge was holding — and the reason a
    /// bridge can be a cache rather than a risk.
    pub fn remove_retained_now(&self) {
        if self.inner.retained.borrow().is_empty() {
            return;
        }
        self.inner.retained.borrow_mut().clear();
        self.inner.retained_version.update(|v| *v += 1);
    }

    /// Zoom: multiply every size by `factor` while keeping the viewport center pinned.
    pub fn rescale(&self, factor: f64, new_sizes: impl Fn(usize) -> f64) {
        let step = self.inner.core.borrow_mut().rescale(factor, &new_sizes);
        self.inner.apply(step);
    }

    /// [`Self::rescale`] without touching the surface: the layout, the window
    /// and the scroll signal move now, the DOM scroll offset does not. The
    /// caller writes it (`scroll_to_offset`) once the DOM that positions the
    /// items has been patched from those signals — writing it first shows
    /// the new offset over the old item positions for as long as the patch
    /// takes to arrive.
    pub fn rescale_detached(&self, factor: f64, new_sizes: impl Fn(usize) -> f64) {
        let step = self.inner.core.borrow_mut().rescale(factor, &new_sizes);
        self.inner.apply_local(step);
    }

    /// Report the caller's measured fill pipeline: the median milliseconds one
    /// item's content takes to become real, and how many the caller makes at
    /// once. The content band is decided against this, which is why a machine
    /// that can fill faster never shows a placeholder for the same scroll. A
    /// reader that reports nothing leaves the decision to the speed floor.
    ///
    /// Called from the pipeline's own telemetry, once its median has moved
    /// meaningfully — not per frame.
    pub fn set_fill_profile(&self, fill_ms: f64, lanes: usize) {
        let mut pipeline = self.inner.core.borrow().pipeline();
        pipeline.fill_ms = fill_ms.max(0.0);
        pipeline.lanes = lanes;
        self.inner.core.borrow_mut().set_pipeline(pipeline);
        // The band may have opened or closed without the window moving, and
        // that is a render-state change: `items()` recomputes, the DOM does not
        // move.
        self.inner.sync_band_version();
    }

    /// Whether the current scroll outruns the reported pipeline: the state in
    /// which a mounted item outside the band renders as a placeholder, and the
    /// only state in which a retention bridge is granted.
    pub fn motion_engaged(&self) -> bool {
        self.inner.core.borrow().motion_engaged()
    }

    /// Whether the reader is moving at all, measured rather than assumed:
    /// true while the estimate is above [`DRIFT_EPS_PX_S`]. A sampler that only
    /// wants to know "should I be looking at the page or at the scroll" reads
    /// this instead of watching scroll events, and an effect that must go quiet
    /// while the reader is reading has one threshold to share.
    pub fn motion_drifts(&self) -> bool {
        self.inner.core.borrow().motion_speed() > DRIFT_EPS_PX_S
    }

    /// Where a mounted index ranks in the fill order right now: the viewport,
    /// then the side the reader is approaching, then behind them, then the
    /// placeholders. A queue that works in this order fills the blank the
    /// reader is about to look at first.
    pub fn fill_priority(&self, index: usize) -> virtual_list::FillPriority {
        self.inner.core.borrow().fill_priority(index)
    }

    /// The index the viewport is expected to reach by the time the current fill
    /// finishes: where to aim a prefetch so it is not wrong twice.
    pub fn landing_index(&self) -> usize {
        self.inner.core.borrow().landing_index()
    }

    /// Subscribe to content releases: `f(index, reason)` runs on the tick the
    /// index left the window past the reversal grace (
    /// [`REVERSAL_GRACE_ITEMS`](crate::REVERSAL_GRACE_ITEMS)), and anything that
    /// queued before the subscription is delivered immediately.
    ///
    /// A subscriber owns the expensive half of an item — the raster, the canvas,
    /// the snapshot — and this is the only signal that says the reader has
    /// travelled far enough that keeping it is a cost and not a benefit. Held
    /// until the pane unmounts instead, it is what makes a reader's RAM grow
    /// with the session rather than with the window.
    pub fn on_release(&self, cb: impl Fn(usize, ReleaseReason) + 'static) {
        self.inner.release_cbs.borrow_mut().push(Rc::new(cb));
        self.inner.flush_releases();
    }

    /// Called when scrolling settles.
    pub fn on_scroll_idle(&self, cb: impl Fn() + 'static) {
        self.inner.idle_cbs.borrow_mut().push(Rc::new(cb));
    }

    /// How many items the live window currently mounts (diagnostics).
    ///
    /// The virtualizer keeps `budget`-worth of rows mounted; this is that
    /// window's size right now, read untracked so a diagnostics snapshot can
    /// take it without subscribing. Zero while nothing is bound or the list
    /// is empty.
    pub fn live_window_items(&self) -> usize {
        match self.inner.core.borrow().range() {
            Some(window) => window.last.saturating_sub(window.first) + 1,
            None => 0,
        }
    }

    /// How many evicted items are still kept rendered by the retention grace
    /// (diagnostics). Zombies are bounded by `max_retained`, so this is the
    /// reader's standing extra-DOM count, not a leak signal by itself — but
    /// it must return to zero once the reader is gone.
    pub fn retained_items(&self) -> usize {
        self.inner.retained.borrow().len()
    }

    /// Live DOM/event bookkeeping counts (diagnostics): event listener
    /// bindings, `ResizeObserver` bindings, and armed timers (the scroll-end
    /// debounce and the zombie-retention expiry). The ownership document
    /// lists these as resources the virtualizer holds; the baseline reads
    /// them here so a dispose that left one behind is visible, not inferred.
    pub fn listener_bindings(&self) -> usize {
        self.inner.listeners.borrow().len()
    }

    /// How many `ResizeObserver` bindings the virtualizer currently holds
    /// (diagnostics; 0 or 1 — the container observer).
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

// only the changed file was rewritten
