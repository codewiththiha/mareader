//! The machinery a press gesture is made of, shared by
//! [`long_press`](super::long_press) (where a hold is the gesture) and
//! [`draggable_item`](super::draggable_item) (where a hold races a movement
//! and a release for the right to answer the press).
//!
//! Both used to carry their own copy of the pending-timer type, the clear
//! that drops the shim beside its JS handle, the squared-distance test and
//! the arm-a-timeout dance. Two copies of a lifetime-sensitive dance is two
//! places to get the `Closure` drop order wrong — and that mistake is a
//! timeout firing into freed memory, a crash no test would catch.
//!
//! Neither gesture's policy lives here: which answer a press turned into,
//! what a completed hold suppresses and whether a finger may drag are the
//! callers'.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

/// A pending hold timer: the JS timeout handle plus the wasm-shim closure it
/// keeps alive.
///
/// Parked in a `StoredValue` rather than a captured local so a re-run or
/// cleanup cannot free the closure while the timeout is queued: the shim is
/// a raw function pointer into wasm memory, and a `setTimeout` outliving its
/// `Closure` calls into freed memory rather than failing loudly.
pub type PendingTimer = Option<(i32, Closure<dyn FnMut()>)>;

/// Clear a pending timer and drop the shim parked beside it.
///
/// Harmless when the timer has already fired, so every cancellation path
/// calls it unconditionally instead of asking whether there is still
/// something to cancel.
pub fn clear_timer(timer: StoredValue<PendingTimer, LocalStorage>) {
    timer.with_value(|t| {
        if let Some((handle, _)) = t
            && let Some(win) = web_sys::window()
        {
            win.clear_timeout_with_handle(*handle);
        }
    });
    timer.set_value(None);
}

/// Queue `on_fire` for `after_ms` from now, parking the shim where
/// [`clear_timer`] can find it.
///
/// With no window (a host test, a platform with no DOM) it arms nothing and
/// answers silently, which makes the gesture primitives callable from tests
/// that have no browser to time out in.
pub fn arm_timer(
    timer: StoredValue<PendingTimer, LocalStorage>,
    after_ms: i32,
    on_fire: impl FnMut() + 'static,
) {
    let Some(win) = web_sys::window() else {
        return;
    };
    let cb = Closure::<dyn FnMut()>::new(on_fire);
    let f: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
    if let Ok(handle) = win.set_timeout_with_callback_and_timeout_and_arguments_0(&f, after_ms) {
        timer.set_value(Some((handle, cb)));
    }
}

/// Whether a pointer that started at an origin has left a radius around it.
///
/// Both sides squared, so the test that runs on every `pointermove` costs no
/// square root. The boundary itself counts as INSIDE, which is the whole of what
/// makes a radius of zero mean "any drift at all" rather than "no drift ever" —
/// a hold's slop and a drag's threshold are both radii a shaky finger stays
/// inside, and neither wants to fire on a press that did not move.
pub fn outside_radius(dx: f64, dy: f64, radius_px: f64) -> bool {
    dx * dx + dy * dy > radius_px * radius_px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_boundary_counts_as_inside() {
        assert!(!outside_radius(3.0, 4.0, 5.0), "exactly on the radius is inside");
        assert!(outside_radius(3.0, 4.0001, 5.0));
        assert!(!outside_radius(0.0, 0.0, 5.0));
    }

    #[test]
    fn a_radius_of_zero_means_any_drift_at_all() {
        // Which is what lets a caller say "this gesture does not tolerate
        // movement" with the same number the platform already measures in.
        assert!(outside_radius(0.0001, 0.0, 0.0));
        assert!(!outside_radius(0.0, 0.0, 0.0));
    }

    #[test]
    fn the_test_is_symmetric_in_the_two_axes() {
        assert_eq!(
            outside_radius(6.0, 8.0, 10.0),
            outside_radius(8.0, 6.0, 10.0),
            "a drift is a distance, not a direction"
        );
    }

    #[test]
    fn a_negative_drift_is_the_same_distance() {
        assert_eq!(
            outside_radius(-6.0, -8.0, 9.0),
            outside_radius(6.0, 8.0, 9.0)
        );
    }
}
