//! One gesture wrapper for a card: tap, hold and drag, decided once.

use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::press_core::{self, PendingTimer};

/// What a press must begin on for the card to stand aside.
const OWNED_BY_A_CONTROL: &str = "button, a, input, select, textarea, [contenteditable]";

/// How far the pointer may travel before a drag commits.
pub const DRAG_THRESHOLD_PX: f64 = 6.0;

/// The gesture's decision, locked until release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Nothing decided yet.
    Undecided,
    /// Released without travelling — the press was an open.
    Tap,
    /// The hold completed — selection owns the pointer now.
    Hold,
    /// The pointer travelled and a drag was allowed — the drag owns it.
    Drag,
    /// The pointer travelled and no drag was allowed.
    Abandoned,
}

/// What the gesture needs from the caller; all fields are `Copy`.
pub struct DraggableItemOptions {
    /// How long a press must hold to become a selection; the value
    /// comes from `long_press`.
    pub press_ms: i32,
    /// How far the pointer travels before the press commits to a drag.
    pub drag_threshold_px: f64,
    /// Whether a drag is allowed at all.
    pub draggable: Signal<bool>,
    /// Whether a hold may start a selection.
    pub selectable: Signal<bool>,
    /// The press ended as a tap.
    pub on_tap: Callback<()>,
    /// The hold completed: enter the selection with this card already in it.
    pub on_long_press: Callback<()>,
    /// The pointer committed to a drag, at those coordinates.
    pub on_drag_start: Callback<(f64, f64), ()>,
    /// The drag ended on a release, at the coordinates the pointer came up at.
    pub on_drag_end: Callback<(f64, f64), ()>,
    /// A cancellation, not a release: it puts the drag back.
    pub on_drag_cancel: Callback<()>,
}

/// The handlers to spread onto the element, plus `pressing` and the
/// one-shot swallow probes.
pub struct DraggableItemHandle {
    pub on_pointerdown: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    pub on_pointermove: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    pub on_pointerup: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    pub on_pointercancel: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    /// Reactive "the press is counting" flag.
    pub pressing: RwSignal<bool>,
    /// One-shot: `true` when the click following a completed hold must be
    /// swallowed; resets on read.
    pub swallow_click: Rc<dyn Fn() -> bool>,
    /// One-shot: swallow the synthetic contextmenu after a hold.
    pub swallow_context: Rc<dyn Fn() -> bool>,
}

/// Drop an in-flight hold through [`press_core::clear_timer`].
fn cancel_hold(timer: StoredValue<PendingTimer, LocalStorage>) {
    press_core::clear_timer(timer);
}

/// Whether the pointer left the threshold around its origin.
fn travelled(x: f64, y: f64, origin: (f64, f64), threshold_px: f64) -> bool {
    press_core::outside_radius(x - origin.0, y - origin.1, threshold_px)
}

/// Whether a travelled press may become a drag: `allowed` and not
/// `touch`.
fn may_drag(allowed: bool, touch: bool) -> bool {
    allowed && !touch
}

/// Build the gesture handlers, owned by the current reactive owner.
pub fn use_draggable_item(options: DraggableItemOptions) -> DraggableItemHandle {
    let DraggableItemOptions {
        press_ms,
        drag_threshold_px,
        draggable,
        selectable,
        on_tap,
        on_long_press,
        on_drag_start,
        on_drag_end,
        on_drag_cancel,
    } = options;

    let mode = StoredValue::new_local(Mode::Undecided);
    let origin = StoredValue::new_local(None::<(f64, f64)>);
    // Finger versus mouse/pen, for the life of the press.
    let touch = StoredValue::new_local(false);
    let timer: StoredValue<PendingTimer, LocalStorage> = StoredValue::new_local(None);
    let suppress_click = StoredValue::new_local(false);
    let suppress_context = StoredValue::new_local(false);
    let pressing = RwSignal::new(false);

    let cancel: Rc<dyn Fn()> = Rc::new(move || cancel_hold(timer));
    let reset: Rc<dyn Fn()> = Rc::new({
        let cancel = Rc::clone(&cancel);
        move || {
            cancel();
            mode.set_value(Mode::Undecided);
            origin.set_value(None);
            touch.set_value(false);
            pressing.set(false);
        }
    });

    let on_pointerdown: Rc<dyn Fn(&leptos::ev::PointerEvent)> = Rc::new({
        let reset = Rc::clone(&reset);
        move |ev| {
            // Only the primary button starts a gesture.
            if ev.button() != 0 {
                reset();
                return;
            }
            reset();
            let Some(target) = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            else {
                return;
            };
            if target.closest(OWNED_BY_A_CONTROL).ok().flatten().is_some() {
                return;
            }
            suppress_click.set_value(false);
            suppress_context.set_value(false);
            origin.set_value(Some((ev.client_x() as f64, ev.client_y() as f64)));
            // A finger's movement is a scroll, not a drag.
            touch.set_value(ev.pointer_type() == "touch");
            pressing.set(true);

            // Capture on the bound element: a cover swapped under the
            // pointer cancels the drag.
            let host = ev
                .current_target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                .unwrap_or(target);
            let _ = host.set_pointer_capture(ev.pointer_id());

            if !selectable.get_untracked() {
                return;
            }
            press_core::arm_timer(timer, press_ms, move || {
                // Decided once: a hold firing mid-drag would be a second
                // answer to the same press.
                if mode.get_value() != Mode::Undecided {
                    return;
                }
                mode.set_value(Mode::Hold);
                pressing.set(false);
                suppress_click.set_value(true);
                suppress_context.set_value(true);
                on_long_press.run(());
            });
        }
    });

    let on_pointermove: Rc<dyn Fn(&leptos::ev::PointerEvent)> = Rc::new({
        let cancel = Rc::clone(&cancel);
        move |ev| {
            let Some(at) = origin.get_value() else {
                return;
            };
            let point = (ev.client_x() as f64, ev.client_y() as f64);
            match mode.get_value() {
                Mode::Undecided => {
                    if !travelled(point.0, point.1, at, drag_threshold_px) {
                        return;
                    }
                    cancel();
                    pressing.set(false);
                    if may_drag(draggable.get_untracked(), touch.get_value()) {
                        mode.set_value(Mode::Drag);
                        on_drag_start.run(point);
                    } else {
                        mode.set_value(Mode::Abandoned);
                    }
                }
                // A drag's stream is the session's; a hold has
                // already answered this press.
                Mode::Drag | Mode::Tap | Mode::Hold | Mode::Abandoned => {}
            }
        }
    });

    let on_pointerup: Rc<dyn Fn(&leptos::ev::PointerEvent)> = Rc::new({
        let reset = Rc::clone(&reset);
        move |ev| {
            // Not our press: no pointerdown of ours is in flight.
            if origin.get_value().is_none() {
                return;
            }
            match mode.get_value() {
                Mode::Undecided => {
                    mode.set_value(Mode::Tap);
                    on_tap.run(());
                }
                // The release point, not the last sampled move.
                Mode::Drag => on_drag_end.run((ev.client_x() as f64, ev.client_y() as f64)),
                Mode::Tap | Mode::Hold | Mode::Abandoned => {}
            }
            reset();
        }
    });

    let on_pointercancel: Rc<dyn Fn(&leptos::ev::PointerEvent)> = Rc::new({
        let reset = Rc::clone(&reset);
        move |_ev| {
            if mode.get_value() == Mode::Drag {
                on_drag_cancel.run(());
            }
            reset();
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

    on_cleanup(move || cancel_hold(timer));

    DraggableItemHandle {
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
    use crate::components::primitives::interactions::long_press::SELECT_SLOP_PX;

    #[test]
    fn the_threshold_is_a_radius_around_the_origin() {
        let at = (100.0, 100.0);
        assert!(!travelled(100.0, 100.0, at, 6.0));
        assert!(
            !travelled(106.0, 100.0, at, 6.0),
            "on the boundary is still a press"
        );
        assert!(travelled(106.1, 100.0, at, 6.0));
        // Diagonal drift counts its Euclidean length, not per-axis.
        assert!(travelled(105.0, 105.0, at, 6.0));
        assert!(!travelled(104.0, 104.0, at, 6.0));
        // The same boundary on the far side.
        assert!(!travelled(94.0, 100.0, at, 6.0));
        assert!(travelled(93.9, 100.0, at, 6.0));
        assert!(travelled(100.0, 93.0, at, 6.0));
    }

    #[test]
    fn a_zero_threshold_commits_on_any_drift_at_all() {
        let at = (0.0, 0.0);
        assert!(!travelled(0.0, 0.0, at, 0.0));
        assert!(travelled(0.5, 0.0, at, 0.0));
    }

    #[test]
    fn a_finger_scrolls_rather_than_drags() {
        // Both refusals answer what the pointer's position cannot.
        assert!(
            may_drag(true, false),
            "a mouse is the drag the wrapper is for"
        );
        assert!(!may_drag(true, true), "a finger's movement is the page's");
        assert!(
            !may_drag(false, false),
            "a card that is not draggable stays put"
        );
        assert!(!may_drag(false, true));
    }

    // A compile-time constant rather than a test: a build that
    // breaks the rule cannot ship.
    const _: () = assert!(DRAG_THRESHOLD_PX < SELECT_SLOP_PX);
}
