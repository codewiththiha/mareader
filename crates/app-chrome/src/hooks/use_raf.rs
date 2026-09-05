//! The two animation-frame primitives: one frame per event burst, and one
//! loop that keeps its own next frame.
//!
//! [`raf_coalesce`] is for high-rate EVENTS. `scroll`, `pointermove` and
//! `resize` can all fire several times between two paints, and a handler that
//! reads layout (`getBoundingClientRect`, `innerHeight`) forces a synchronous
//! style + layout pass on each one. The extra passes buy nothing: nothing is
//! drawn until the next frame, so every result but the last is discarded. The
//! coalescer wraps such a handler so it runs at most once per frame, on the
//! frame itself — where the layout it reads is the layout that is about to be
//! painted.
//!
//! [`FrameLoop`] is for animations, which have no event to hang off: the frame
//! itself decides whether another one is needed. It owns the machinery every
//! such loop in the app was hand-rolling — a slot the running step re-arms
//! itself through, a flag a queued frame checks before it touches anything
//! reactive, the pending frame's id so a stop can cancel it, and the owner
//! cleanup that stops it all when the surface goes away. Two loops were built
//! out of that machinery twice over (the zoom tween and the floating-surface
//! spring, the second with a comment admitting it was "the same shape as" the
//! first), which is two places for a lifetime bug to live.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

/// Wrap `f` so that calling it any number of times before the next animation
/// frame schedules exactly one call, on that frame.
///
/// The returned closure is cheap to clone, so one coalescer can serve several
/// listeners (scroll and resize feeding the same recompute, say) and they will
/// share the frame rather than each getting one.
///
/// Must be called from inside a reactive scope: the pending flag is owned by
/// it, and a frame that lands after the owner is gone is dropped instead of
/// running `f` against disposed state.
pub fn raf_coalesce(f: impl Fn() + 'static) -> impl Fn() + Clone + 'static {
    let pending = StoredValue::new_local(false);
    let f = Rc::new(f);

    move || {
        // `None` = the owner is gone; treat it as "already pending" so no
        // further frames are queued.
        if pending.try_get_value().unwrap_or(true) {
            return;
        }
        pending.set_value(true);

        let f = Rc::clone(&f);
        request_animation_frame(move || {
            // Disposed between the schedule and the frame: there is nothing
            // left for `f` to write to.
            if pending.try_get_value().is_none() {
                return;
            }
            pending.set_value(false);
            f();
        });
    }
}

/// A slot holding the pending frame's id, so a stop can cancel it.
type RafId = Rc<Cell<Option<i32>>>;

/// Cancel the frame this loop has queued, if any.
fn cancel(raf: &RafId) {
    if let Some(id) = raf.take()
        && let Some(w) = web_sys::window()
    {
        let _ = w.cancel_animation_frame(id);
    }
}

/// Queue `f` for the next frame, replacing any frame this loop already had
/// queued: one loop, one pending frame, however often it is re-armed.
fn queue(raf: &RafId, f: impl FnOnce() + 'static) {
    cancel(raf);
    let Some(w) = web_sys::window() else {
        return;
    };
    let cb = Closure::once_into_js(f);
    if let Ok(id) = w.request_animation_frame(cb.as_ref().unchecked_ref()) {
        raf.set(Some(id));
    }
}

/// One self-rearming animation-frame loop.
///
/// [`arm`](Self::arm) hands it a step and makes sure exactly one frame is
/// queued; the step returns `true` for "another frame, please" and `false` for
/// "I am done". Arming a loop that is already running does NOT stack a second
/// frame — it replaces the step, and the live loop picks the new one up on the
/// frame it already has queued. That is what makes a retarget cheap: a loop
/// whose step reads its target from a signal adopts the new target with no
/// teardown at all, and a loop whose step closes over the old one starts
/// running the new closure instead.
///
/// ## Why the flag and the id, not one or the other
///
/// A frame callback cannot always be cancelled — it is already queued, and the
/// owner that would cancel it may be gone — so the callback checks
/// `alive` BEFORE it reads anything reactive. Reading a signal whose owner has
/// been cleaned up does not hand back `None`; it unwinds through a callback
/// nobody owns. The flag lives in the loop's own `Rc`, so it is the one piece
/// of evidence still safe to read. The id is the cheaper half: a `stop` that
/// can cancel does, and the flag then only has to catch the frames it could
/// not.
///
/// Build it inside a reactive scope. Cleanup is registered at construction, so
/// a loop dies with the surface that armed it rather than stepping once more
/// against disposed state.
#[derive(Clone)]
pub struct FrameLoop {
    /// The step, parked where the frame callback can find it. The callback
    /// holds only a WEAK reference, so replacing this slot is what retargets a
    /// running loop and dropping it is what ends one.
    slot: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    alive: Rc<Cell<bool>>,
    raf: RafId,
}

impl FrameLoop {
    /// A stopped loop, owned by the current reactive scope.
    pub fn new() -> Self {
        let alive = Rc::new(Cell::new(false));
        let raf: RafId = Rc::new(Cell::new(None));
        // Parked through a stored value rather than captured directly: a
        // cleanup closure may not hold an `Rc`, and the store is dropped before
        // the closure could reach it, hence the `try_` read.
        let store = StoredValue::new_local(Some((alive.clone(), raf.clone())));
        on_cleanup(move || {
            if let Some((alive, raf)) = store.try_get_value().flatten() {
                alive.set(false);
                cancel(&raf);
            }
        });
        Self { slot: Rc::new(RefCell::new(None)), alive, raf }
    }

    /// Run `step` once per frame until it returns `false`.
    pub fn arm(&self, step: impl Fn() -> bool + 'static) {
        let running = self.alive.get();
        let alive = Rc::clone(&self.alive);
        let raf = Rc::clone(&self.raf);
        let slot = Rc::clone(&self.slot);
        let weak = Rc::downgrade(&self.slot);
        let step = Rc::new(step);

        let trampoline: Rc<dyn Fn()> = Rc::new(move || {
            if !alive.get() {
                return;
            }
            if !step() {
                alive.set(false);
                *slot.borrow_mut() = None;
                cancel(&raf);
                return;
            }
            // Re-arm through the slot, not through this closure: whatever is in
            // the slot NOW is the step that runs next, which is how a retarget
            // mid-flight is adopted without a second loop.
            if let Some(next) = weak.upgrade().and_then(|s| s.borrow().clone()) {
                queue(&raf, move || next());
            }
        });

        *self.slot.borrow_mut() = Some(trampoline.clone());
        if running {
            return;
        }
        self.alive.set(true);
        queue(&self.raf, move || trampoline());
    }

    /// Stop: cancel the queued frame, drop the step, and let a later `arm`
    /// start a fresh loop.
    pub fn stop(&self) {
        self.alive.set(false);
        *self.slot.borrow_mut() = None;
        cancel(&self.raf);
    }
}

impl Default for FrameLoop {
    fn default() -> Self {
        Self::new()
    }
}
