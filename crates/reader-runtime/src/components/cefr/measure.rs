//! Range measurement, batched into single animation frames.

use leptos::prelude::request_animation_frame;
use std::cell::{Cell, RefCell};

thread_local! {
    /// Planned walks waiting for the next frame; plain closures only.
    static QUEUE: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
    /// One flush at a time is in flight.
    static DUE: Cell<bool> = const { Cell::new(false) };
}

/// Hand one measuring task to the next frame; callers share a flush.
pub(crate) fn measure(task: impl FnOnce() + 'static) {
    QUEUE.with(|queue| queue.borrow_mut().push(Box::new(task)));
    if DUE.with(|due| due.replace(true)) {
        return;
    }
    request_animation_frame(drain);
}

fn drain() {
    DUE.with(|due| due.set(false));
    let tasks = QUEUE.with(|queue| queue.borrow_mut().drain(..).collect::<Vec<_>>());
    for task in tasks {
        task();
    }
}
