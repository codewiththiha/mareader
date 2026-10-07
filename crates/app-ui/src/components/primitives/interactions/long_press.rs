//! Long-press gesture: bookkeeping, slop, the hold timer, and the
//! one-shot click suppression that follows.

use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::press_core::{self, PendingTimer};

/// How long a press must hold to become a SELECTION gesture.
pub const SELECT_PRESS_MS: i32 = 450;

/// How far the pointer may drift during that hold.
pub const SELECT_SLOP_PX: f64 = 8.0;

/// What the gesture needs from the caller.
pub struct LongPressOptions {
    /// How long a press must hold before it completes (ms).
    pub press_ms: i32,
    /// Pointer may drift this far (px) without cancelling the gesture.
    pub slop_px: f64,
    /// Take pointer capture on press, so a drift off the element survives.
    pub capture_pointer: bool,
    /// When false, a pointerdown starts nothing (e.g. selection mode active).
    pub enabled: Signal<bool>,
    /// Fired when the hold completes.
    pub on_press: Callback<()>,
}

/// Handlers to spread onto the element, plus the live tint.
pub struct LongPressHandlers {
    pub on_pointerdown: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    pub on_pointermove: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    pub on_pointerup: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    pub on_pointercancel: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    /// Reactive "currently pressing" flag (instant feedback pre-completion).
    pub pressing: RwSignal<bool>,
    /// One-shot: `true` when the click following a completed press must be
    /// swallowed; resets on read.
    pub swallow_click: Rc<dyn Fn() -> bool>,
    /// One-shot: true when the synthetic contextmenu must be swallowed.
    pub swallow_context: Rc<dyn Fn() -> bool>,
}

/// Stop an in-flight press; the timer half is `clear_timer`'s.
fn cancel_press(
    press_active: StoredValue<bool, LocalStorage>,
    timer: StoredValue<PendingTimer, LocalStorage>,
) {
    press_active.set_value(false);
    press_core::clear_timer(timer);
}

/// Whether a pointer has stayed within the slop radius, squared.
fn within_slop(dx: f64, dy: f64, slop_px: f64) -> bool {
    !press_core::outside_radius(dx, dy, slop_px)
}

/// Build the long-press handlers, owned by the current reactive owner.
pub fn use_long_press(options: LongPressOptions) -> LongPressHandlers {
    let LongPressOptions {
        press_ms,
        slop_px,
        capture_pointer,
        enabled,
        on_press,
    } = options;

    let press_active = StoredValue::new_local(false);
    let press_start = StoredValue::new_local(None::<(i32, i32)>);
    let timer: StoredValue<PendingTimer, LocalStorage> = StoredValue::new_local(None);
    let suppress_click = StoredValue::new_local(false);
    let suppress_context = StoredValue::new_local(false);
    let pressing = RwSignal::new(false);

    let cancel: Rc<dyn Fn()> = Rc::new(move || cancel_press(press_active, timer));

    let on_pointerdown: Rc<dyn Fn(&leptos::ev::PointerEvent)> = Rc::new(move |ev| {
        if !enabled.get_untracked() {
            return;
        }
        // Keep receiving move/up even when the pointer drifts off a small
        // target.
        if capture_pointer
            && let Some(el) = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        {
            let _ = el.set_pointer_capture(ev.pointer_id());
        }
        press_active.set_value(true);
        pressing.set(true);
        suppress_click.set_value(false);
        suppress_context.set_value(false);
        press_start.set_value(Some((ev.client_x(), ev.client_y())));

        press_core::arm_timer(timer, press_ms, move || {
            if !press_active.get_value() {
                return;
            }
            press_active.set_value(false);
            pressing.set(false);
            suppress_click.set_value(true);
            suppress_context.set_value(true);
            on_press.run(());
        });
    });

    let cancel_move = Rc::clone(&cancel);
    let on_pointermove: Rc<dyn Fn(&leptos::ev::PointerEvent)> = Rc::new(move |ev| {
        if !press_active.get_value() {
            return;
        }
        let Some((sx, sy)) = press_start.get_value() else {
            return;
        };
        let dx = (ev.client_x() - sx) as f64;
        let dy = (ev.client_y() - sy) as f64;
        if !within_slop(dx, dy, slop_px) {
            cancel_move();
            pressing.set(false);
        }
    });

    let cancel_up = Rc::clone(&cancel);
    let on_pointerup: Rc<dyn Fn(&leptos::ev::PointerEvent)> = Rc::new({
        move |_| {
            cancel_up();
            pressing.set(false);
        }
    });

    let cancel_cancel = Rc::clone(&cancel);
    let on_pointercancel: Rc<dyn Fn(&leptos::ev::PointerEvent)> = Rc::new({
        move |_| {
            cancel_cancel();
            pressing.set(false);
        }
    });

    let swallow_click: Rc<dyn Fn() -> bool> = Rc::new(move || {
        if suppress_click.get_value() {
            suppress_click.set_value(false);
            true
        } else {
            false
        }
    });

    let swallow_context: Rc<dyn Fn() -> bool> = Rc::new(move || {
        if suppress_context.get_value() {
            suppress_context.set_value(false);
            true
        } else {
            false
        }
    });

    on_cleanup(move || cancel_press(press_active, timer));

    LongPressHandlers {
        on_pointerdown,
        on_pointermove,
        on_pointerup,
        on_pointercancel,
        pressing,
        swallow_click,
        swallow_context,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slop_radius_keeps_a_steady_hold() {
        assert!(within_slop(0.0, 0.0, 8.0));
        // Diagonal drift counts its Euclidean length, not per-axis.
        assert!(within_slop(5.0, 5.0, 8.0));
        // Exactly on the boundary is still inside (the cancel is strict).
        assert!(within_slop(8.0, 0.0, 8.0));
        assert!(!within_slop(8.01, 0.0, 8.0));
        assert!(!within_slop(-9.0, 0.0, 8.0));
        assert!(!within_slop(6.0, 6.0, 8.0));
    }

    #[test]
    fn a_zero_slop_only_tolerates_a_perfectly_still_hold() {
        assert!(within_slop(0.0, 0.0, 0.0));
        assert!(!within_slop(0.5, 0.0, 0.0));
    }
}
