//! The two animation-frame primitives: a per-burst coalescer and a
//! self-rearming loop.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

/// Wrap `f` so any number of calls before the next frame run once.
pub fn raf_coalesce(f: impl Fn() + 'static) -> impl Fn() + Clone + 'static {
    let pending = StoredValue::new_local(false);
    let f = Rc::new(f);

    move || {
        // `None` = the owner is gone; treat it as already pending.
        if pending.try_get_value().unwrap_or(true) {
            return;
        }
        pending.set_value(true);

        let f = Rc::clone(&f);
        request_animation_frame(move || {
            // Disposed between the schedule and the frame: nothing to write to.
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

/// The loop's own step and the trampoline that re-queues it.
type Step = Rc<dyn Fn()>;

fn cancel(raf: &RafId) {
    if let Some(id) = raf.take()
        && let Some(w) = web_sys::window()
    {
        let _ = w.cancel_animation_frame(id);
    }
}

/// Queue `f` for the next frame, replacing any already queued.
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

/// One self-rearming animation-frame loop: a step returning true for
/// another frame, false for done.
#[derive(Clone)]
pub struct FrameLoop {
    /// The step, parked where the frame callback finds it, weakly.
    slot: Rc<RefCell<Option<Step>>>,
    alive: Rc<Cell<bool>>,
    raf: RafId,
}

impl FrameLoop {
    /// A stopped loop, owned by the current reactive scope.
    pub fn new() -> Self {
        let alive = Rc::new(Cell::new(false));
        let raf: RafId = Rc::new(Cell::new(None));
        // Parked in a stored value: a cleanup closure may not hold an `Rc`.
        let store = StoredValue::new_local(Some((alive.clone(), raf.clone())));
        on_cleanup(move || {
            if let Some((alive, raf)) = store.try_get_value().flatten() {
                alive.set(false);
                cancel(&raf);
            }
        });
        Self {
            slot: Rc::new(RefCell::new(None)),
            alive,
            raf,
        }
    }

    /// Run `step` once per frame until it returns `false`.
    pub fn arm(&self, step: impl Fn() -> bool + 'static) {
        let running = self.alive.get();
        let alive = Rc::clone(&self.alive);
        let raf = Rc::clone(&self.raf);
        let weak = Rc::downgrade(&self.slot);
        let step = Rc::new(step);

        let trampoline: Step = Rc::new(move || {
            if !alive.get() {
                return;
            }
            if !step() {
                alive.set(false);
                if let Some(slot) = weak.upgrade() {
                    *slot.borrow_mut() = None;
                }
                cancel(&raf);
                return;
            }
            // Re-arm through the slot: whatever is in it NOW runs next.
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

    /// Stop: cancel the frame, drop the step; a later `arm` starts fresh.
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
