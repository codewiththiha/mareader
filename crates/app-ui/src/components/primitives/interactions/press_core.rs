//! The machinery a press gesture is made of; the policies live in the
//! callers.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

/// A pending hold timer: the JS handle plus the shim boxed with it.
pub type PendingTimer = Option<(i32, Closure<dyn FnMut()>)>;

/// Clear a pending timer and drop the shim beside it.
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

/// Queue `on_fire` for `after_ms`, parking the shim for the clear;
/// with no window nothing arms.
pub fn arm_timer(
    timer: StoredValue<PendingTimer, LocalStorage>,
    after_ms: i32,
    on_fire: impl FnMut() + 'static,
) {
    // A second pointerdown replaces the first hold; clear its handle first.
    clear_timer(timer);
    let Some(win) = web_sys::window() else {
        return;
    };
    let cb = Closure::<dyn FnMut()>::new(on_fire);
    let f: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
    if let Ok(handle) = win.set_timeout_with_callback_and_timeout_and_arguments_0(&f, after_ms) {
        timer.set_value(Some((handle, cb)));
    }
}

/// Whether a pointer has left a radius around its origin, squared.
pub fn outside_radius(dx: f64, dy: f64, radius_px: f64) -> bool {
    dx * dx + dy * dy > radius_px * radius_px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_boundary_counts_as_inside() {
        assert!(
            !outside_radius(3.0, 4.0, 5.0),
            "exactly on the radius is inside"
        );
        assert!(outside_radius(3.0, 4.0001, 5.0));
        assert!(!outside_radius(0.0, 0.0, 5.0));
    }

    #[test]
    fn a_radius_of_zero_means_any_drift_at_all() {
        // A caller can then say "no movement at all" with radius zero.
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
