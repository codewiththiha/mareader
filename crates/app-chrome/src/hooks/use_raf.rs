//! Frame coalescing for high-rate events.
//!
//! `scroll`, `pointermove` and `resize` can all fire several times between two
//! paints, and a handler that reads layout (`getBoundingClientRect`,
//! `innerHeight`) forces a synchronous style + layout pass on each one. The
//! extra passes buy nothing: nothing is drawn until the next frame, so every
//! result but the last is discarded.
//!
//! [`raf_coalesce`] wraps such a handler so it runs at most once per frame,
//! on the frame itself — where the layout it reads is the layout that is about
//! to be painted.

use std::rc::Rc;

use leptos::prelude::*;

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
