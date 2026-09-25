//! One gesture wrapper for a card: tap, hold and drag, decided once.
//!
//! A tap opens, a hold starts a multi-select and a movement files the book
//! elsewhere — and all three arrive as the same `pointerdown`. Handled
//! separately they race: a completed hold's exhaust click opens the book it
//! meant to select, or a two-pixel drift cancels a gesture still being made.
//! So the mode is decided once and locked until release; the first of these
//! wins and the others become unreachable:
//!
//!   * the hold timer fires → [`Mode::Hold`], and the click and synthetic
//!     contextmenu that follow are swallowed;
//!   * the pointer travels past [`DRAG_THRESHOLD_PX`] → [`Mode::Drag`] when
//!     the caller allows a drag, [`Mode::Abandoned`] when it does not;
//!   * release having done neither → [`Mode::Tap`].
//!
//! Travelling past the threshold while nothing is draggable is its own answer
//! rather than a tap: on a touch surface that movement is a scroll, and a
//! touch pointer is therefore never a drag, whatever the caller allows.
//!
//! This module owns the decision and nothing else: what a movement then does
//! (payload, targets, drop) belongs to the caller — see
//! `library_runtime::features::library::dnd` for the shelf's. That is why drag start
//! and end carry coordinates and the wrapper keeps no drag flag: the visible
//! half of a drag is a session that can hold four cards at once.

use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::press_core::{self, PendingTimer};

/// What a press has to have begun on for the card to stand aside.
///
/// A card's own controls (the ✕, the relink) stop their `click`, but not the
/// `pointerdown`/`pointerup` either side of it — a tap decided from that pair
/// would open the book the reader was asking to remove. Handing the press to
/// the control here is one rule rather than a `stop_propagation` on every
/// pointer event of every control a card ever grows.
const OWNED_BY_A_CONTROL: &str = "button, a, input, select, textarea, [contenteditable]";

/// How far the pointer may travel before the press commits to a drag.
///
/// Smaller than the long-press slop (`long_press::SELECT_SLOP_PX`, the
/// distance a hold survives) on purpose: a drag must be decided before the
/// hold it replaces would have been cancelled, or the two overlap in a band
/// where the reader gets neither. Six pixels is inside a shaky finger's drift
/// and outside a deliberate move.
pub const DRAG_THRESHOLD_PX: f64 = 6.0;

/// What the gesture decided. Locked from the moment it is anything but
/// [`Mode::Undecided`] until the pointer is released.
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
    /// The pointer travelled and no drag was allowed — or the pointer is a
    /// finger, whose movement is a scroll.
    Abandoned,
}

/// What the gesture needs from the caller. Every field is a `Copy` handle, so
/// one options value can feed all four handlers without being taken apart.
pub struct DraggableItemOptions {
    /// How long a press must hold before it becomes a selection. Callers
    /// pass `long_press::SELECT_PRESS_MS`: a book on a shelf and a stroke on
    /// a page are one gesture to a reader, and two spellings of one number
    /// eventually stop matching.
    pub press_ms: i32,
    /// How far the pointer travels before the press commits to a drag.
    pub drag_threshold_px: f64,
    /// Whether a drag is allowed at all. Off while the shelf is choosing: the
    /// pointer is selecting, not filing.
    pub draggable: Signal<bool>,
    /// Whether a hold may start a selection. Off once one is running, where a
    /// tap already toggles and a second gesture per card would be a second way
    /// to do the thing the tap now does.
    pub selectable: Signal<bool>,
    /// The press ended as a tap.
    pub on_tap: Callback<()>,
    /// The hold completed: enter the selection with this card already in it.
    pub on_long_press: Callback<()>,
    /// The pointer committed to a drag, at the coordinates it committed at:
    /// the pointerdown that started the press is six pixels and a decision
    /// behind by now.
    pub on_drag_start: Callback<(f64, f64), ()>,
    /// The pointer is dragging, with its client coordinates. The `()` answer
    /// type is spelled out: a stream of coordinates is not an answer anybody
    /// wants back.
    pub on_drag_move: Callback<(f64, f64), ()>,
    /// The drag ended on a release, at the coordinates the pointer came up at.
    pub on_drag_end: Callback<(f64, f64), ()>,
    /// The drag was taken away rather than released — a `pointercancel`:
    /// a scroll claiming the pointer, the window losing focus, or an engine
    /// deciding the drag was its own.
    ///
    /// Separate from [`DraggableItemOptions::on_drag_end`] because the two
    /// want opposite things: a release puts down what the reader was holding;
    /// a cancellation puts it back.
    pub on_drag_cancel: Callback<()>,
}

/// The four pointer handlers to spread onto the element, the live flag a card
/// paints itself from, and the one-shot probes for the events a completed
/// hold generates.
///
/// There is deliberately no "is dragging" flag: the press is this wrapper's
/// and the drag is the caller's. A shelf drag can hold four cards at once and
/// the card the press began on cannot answer for the other three, so the
/// drag's visible half is painted from the session that owns it. What stays
/// here is `pressing` — a fact about this element and no other.
pub struct DraggableItemHandle {
    pub on_pointerdown: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    pub on_pointermove: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    pub on_pointerup: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    pub on_pointercancel: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    /// Reactive "the press is counting" flag — the tint that arrives on the
    /// frame the finger does, before anything has been decided.
    pub pressing: RwSignal<bool>,
    /// One-shot: `true` when the click following a completed hold must be
    /// swallowed; resets on read.
    pub swallow_click: Rc<dyn Fn() -> bool>,
    /// One-shot: `true` when the synthetic contextmenu after a completed hold
    /// must be swallowed; resets on read.
    pub swallow_context: Rc<dyn Fn() -> bool>,
}

/// Stop an in-flight hold through [`press_core::clear_timer`]: a stale
/// `setTimeout` calling into a dropped wasm shim is a crash rather than a
/// wrong answer, and there is one right way to drop it.
fn cancel_hold(timer: StoredValue<PendingTimer, LocalStorage>) {
    press_core::clear_timer(timer);
}

/// Whether the pointer has left the threshold around its origin. The
/// arithmetic is [`press_core::outside_radius`]'s, shared with the
/// long-press slop: same question, two gestures, and the boundary counting
/// as inside keeps a threshold of zero from firing on a still pointer.
fn travelled(x: f64, y: f64, origin: (f64, f64), threshold_px: f64) -> bool {
    press_core::outside_radius(x - origin.0, y - origin.1, threshold_px)
}

/// Whether a press that has travelled may become a drag.
///
/// Two refusals, neither about the pointer's position: `allowed` is the
/// caller's (a shelf the disk places is not one a hand gets to place);
/// `touch` is the platform's — a finger's movement across a page of covers is
/// a scroll, and without a `touch-action` saying otherwise the reader gets a
/// shelf sliding away under a ghost.
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
        on_drag_move,
        on_drag_end,
        on_drag_cancel,
    } = options;

    let mode = StoredValue::new_local(Mode::Undecided);
    let origin = StoredValue::new_local(None::<(f64, f64)>);
    // Finger vs mouse/pen, held for the life of the press: the decision it
    // feeds is made later, on the move that crosses the threshold, by which
    // time the event that could answer it is gone.
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
            // Only the primary button starts a gesture; a secondary one
            // ends whatever was in flight: the contextmenu it brings is the
            // card's to answer, and a pointerup following it into `on_tap`
            // would open the book the reader was asking about.
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
            // A finger's movement is a scroll until held long enough, and a
            // drag that fought the scroll would lose the reader the shelf
            // they were swiping. An engine that does not name the pointer
            // gets the benefit of the doubt: the empty answer is a mouse's.
            touch.set_value(ev.pointer_type() == "touch");
            pressing.set(true);

            // Capture, so the gesture survives the pointer drifting off a
            // narrow cover and the drag's stream keeps arriving here.
            //
            // Captured on the element the handler is bound to, not the
            // event's target: the target can be a cover `<img>` deep inside a
            // card, and covers land asynchronously — a background render
            // replacing the node under a captured press detaches the capture
            // and the browser answers with a `pointercancel` that kills the
            // drag in its first frames. The card itself is keyed by id and
            // survives every reactive swap inside it. (`current_target` is
            // that element here: this app attaches listeners directly,
            // leptos delegation being off.)
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
                        on_drag_move.run(point);
                    } else {
                        mode.set_value(Mode::Abandoned);
                    }
                }
                Mode::Drag => on_drag_move.run(point),
                // A hold has already answered this press; an abandoned one
                // has nothing left to answer with.
                Mode::Tap | Mode::Hold | Mode::Abandoned => {}
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
                // The release point rather than the last sampled move: a
                // fast drag
                // ends with the pointer somewhere no move was reported for, and
                // where the reader let go is the answer they meant.
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
        // Diagonal drift counts its Euclidean length, not per-axis: 5px each way
        // is 7.07px of travel and a drag, which a per-axis test would miss.
        assert!(travelled(105.0, 105.0, at, 6.0));
        assert!(!travelled(104.0, 104.0, at, 6.0));
        // And in every direction, with the same boundary on the far side: six
        // pixels left of the origin is still a press, six and a hair is a drag.
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
        // Both refusals answer the same question the pointer's position cannot:
        // a drag needs a caller that allows one AND a pointer that is not a
        // finger, because the movement a finger makes across a shelf is a scroll.
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

    // The two numbers are in different modules and one rule holds them
    // together: a drag must be decided before the hold it replaces would
    // have been cancelled, so the reader is never in a band where the
    // pointer has travelled far enough to lose the selection and not far
    // enough to have a drag. A compile-time constant rather than a test: a
    // build that breaks the rule cannot ship at all.
    const _: () = assert!(DRAG_THRESHOLD_PX < SELECT_SLOP_PX);
}
