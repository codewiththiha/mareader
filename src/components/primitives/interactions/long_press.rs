//! Long-press gesture: press-start bookkeeping, movement slop, the hold
//! timer, cancellation on up/cancel/slop, and one-shot suppression of the
//! `click` / synthetic `contextmenu` that a completed press generates.
//!
//! Extracted from the gloss mark layer, where it was inline; the same
//! gesture serves annotations, thumbnails and future touch interactions.

use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

/// What the gesture needs from the caller.
pub struct LongPressOptions {
    /// How long a press must hold before it completes (ms).
    pub press_ms: i32,
    /// Pointer may drift this far (px) without cancelling the gesture.
    pub slop_px: f64,
    /// Take pointer capture on press so the gesture survives drifting off the
    /// element (e.g. a 12-px stroke).
    pub capture_pointer: bool,
    /// When false, a pointerdown starts nothing (e.g. selection mode active).
    pub enabled: Signal<bool>,
    /// Fired when the hold completes.
    pub on_press: Callback<()>,
}

/// Handlers to spread onto the element, plus the live "pressing" tint and the
/// one-shot suppression probes for the events that follow a completed press.
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
    /// One-shot: `true` when the synthetic contextmenu after a completed
    /// press must be swallowed; resets on read.
    pub swallow_context: Rc<dyn Fn() -> bool>,
}

/// Stop an in-flight press: the finger lifted, drifted past slop, or the
/// gesture already completed. Clears the pending timer (harmless if it
/// already fired) and drops the parked closure.
fn cancel_press(
    press_active: StoredValue<bool, LocalStorage>,
    timer: StoredValue<Option<(i32, Closure<dyn FnMut()>)>, LocalStorage>,
) {
    press_active.set_value(false);
    timer.with_value(|t| {
        if let Some((handle, _)) = t {
            if let Some(win) = web_sys::window() {
                win.clear_timeout_with_handle(*handle);
            }
        }
    });
    timer.set_value(None);
}

/// Whether a pointer that started at the press origin has stayed within the
/// slop radius. Both sides squared — no sqrt, and a slop of zero means any
/// drift at all cancels the gesture. The boundary itself counts as inside,
/// matching the strict `>` of the original inline cancel check.
fn within_slop(dx: f64, dy: f64, slop_px: f64) -> bool {
    dx * dx + dy * dy <= slop_px * slop_px
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
    let timer = StoredValue::new_local(None::<(i32, Closure<dyn FnMut()>)>);
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
            && let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        {
            let _ = el.set_pointer_capture(ev.pointer_id());
        }
        press_active.set_value(true);
        pressing.set(true);
        suppress_click.set_value(false);
        suppress_context.set_value(false);
        press_start.set_value(Some((ev.client_x(), ev.client_y())));

        let Some(win) = web_sys::window() else {
            return;
        };
        let cb = Closure::<dyn FnMut()>::new(move || {
            if !press_active.get_value() {
                return;
            }
            press_active.set_value(false);
            pressing.set(false);
            suppress_click.set_value(true);
            suppress_context.set_value(true);
            on_press.run(());
        });
        let f: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
        if let Ok(handle) = win.set_timeout_with_callback_and_timeout_and_arguments_0(&f, press_ms)
        {
            timer.set_value(Some((handle, cb)));
        }
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
