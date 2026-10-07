//! Grabbing empty space: the hand cursor, drag-to-pan with a fling, and
//! hold-to-lift in a split.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use super::lift::HOLD_TO_LIFT_MS;
use app_chrome::hooks::use_raf::FrameLoop;

/// Hand cursor: `ready` over empty space, `grabbing` under a press
/// (styles/components/shell.css).
const PAN_ATTR: &str = "data-pan";
/// Present while a hold counts down to a lift; the stylesheet draws the ring.
const HOLD_ATTR: &str = "data-pan-hold";

/// Never a grab: controls, links, images, marks; text is decided by
/// its glyphs ([`text_at`]).
const NOT_EMPTY: &str = "button, a, input, textarea, select, label, summary, img, svg, video, \
     mark, [contenteditable], [role=menu], [role=menuitem], [role=dialog], [role=separator], \
     [role=slider], [role=button], [data-pane-close], [data-no-grab]";

/// Slack (px) around a line's glyph boxes that still counts as the text.
const TEXT_SLOP_PX: f64 = 1.0;

/// Movement (px) that turns a press into a pan and cancels a pending lift.
const PAN_THRESHOLD_PX: f64 = 4.0;
/// A release this long after the last movement is a rest, not a fling.
const FLING_REST_MS: f64 = 80.0;
/// Below this speed (px/ms) a fling has stopped.
const FLING_MIN_SPEED: f64 = 0.02;
/// Per-16ms decay of a fling's speed.
const FLING_DECAY: f64 = 0.94;

#[derive(Clone, Copy, Default)]
enum Phase {
    #[default]
    Idle,
    /// Pressed on empty space; not yet moved past the threshold.
    Pressed,
    /// Dragging the scroller.
    Panning,
    /// The pane is lifted and follows the pointer.
    Lifted,
}

#[derive(Default)]
struct Grab {
    phase: Phase,
    pointer: i32,
    origin: (f64, f64),
    scroller: Option<web_sys::Element>,
    scroll_start: (f64, f64),
    hold: Option<TimeoutHandle>,
    fling: FrameLoop,
    /// Last pointer sample (client x, y, time) and the smoothed velocity.
    last: (f64, f64, f64),
    velocity: (f64, f64),
    /// Bumped by every press and every end: a fling frame whose number is
    /// stale stops.
    generation: u32,
}

impl Grab {
    fn cancel_hold(&mut self, entry: &web_sys::Element) {
        if let Some(handle) = self.hold.take() {
            handle.clear();
        }
        let _ = entry.remove_attribute(HOLD_ATTR);
    }
}

/// Where a hold-to-lift goes: the workspace, which maps the entry's
/// client points into its own.
pub trait LiftSink {
    /// Whether a hold may lift the pane now (the workspace holds others).
    fn can_lift(&self) -> bool;
    /// The hold completed at `at`: lift the pane. `false` when refused.
    fn begin(&self, at: (f64, f64)) -> bool;
    fn moved(&self, at: (f64, f64));
    /// The press ended: drop the pane where it is (`commit`) or put it back.
    fn end(&self, commit: bool);
}

/// Install the grab listeners on a pane's entry element.
pub fn install(entry: &web_sys::Element, sink: Rc<dyn LiftSink>) {
    let state = Rc::new(RefCell::new(Grab::default()));
    let move_sink = sink.clone();
    let up_sink = sink.clone();

    let hover_entry = entry.clone();
    let on_hover =
        Closure::<dyn Fn(web_sys::PointerEvent)>::new(move |ev: web_sys::PointerEvent| {
            if ev.buttons() != 0 {
                return;
            }
            let ready = is_empty_space(&ev);
            let now = hover_entry.get_attribute(PAN_ATTR);
            match (ready, now.as_deref()) {
                (true, None) => {
                    let _ = hover_entry.set_attribute(PAN_ATTR, "ready");
                }
                (false, Some("ready")) => {
                    let _ = hover_entry.remove_attribute(PAN_ATTR);
                }
                _ => {}
            }
        });

    let leave_entry = entry.clone();
    let on_leave =
        Closure::<dyn Fn(web_sys::PointerEvent)>::new(move |_: web_sys::PointerEvent| {
            if leave_entry.get_attribute(PAN_ATTR).as_deref() == Some("ready") {
                let _ = leave_entry.remove_attribute(PAN_ATTR);
            }
        });

    let down_entry = entry.clone();
    let down_state = state.clone();
    let on_down =
        Closure::<dyn Fn(web_sys::PointerEvent)>::new(move |ev: web_sys::PointerEvent| {
            if ev.button() != 0 || !ev.is_primary() || !is_empty_space(&ev) {
                return;
            }
            // The default stays: it moves focus here, so clicking the margin
            // keeps the keyboard.
            let at = (f64::from(ev.client_x()), f64::from(ev.client_y()));
            let scroller = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                .and_then(|el| scroller_for(&el, &down_entry));
            let mut grab = down_state.borrow_mut();
            grab.cancel_hold(&down_entry);
            grab.generation = grab.generation.wrapping_add(1);
            grab.phase = Phase::Pressed;
            grab.pointer = ev.pointer_id();
            grab.origin = at;
            grab.scroll_start = scroller.as_ref().map_or((0.0, 0.0), |s| {
                (f64::from(s.scroll_left()), f64::from(s.scroll_top()))
            });
            grab.scroller = scroller;
            grab.last = (at.0, at.1, now_ms());
            grab.velocity = (0.0, 0.0);
            let _ = down_entry.set_pointer_capture(ev.pointer_id());
            let _ = down_entry.set_attribute(PAN_ATTR, "grabbing");

            // In a split a hold lifts the pane; the ring shows it filling.
            if sink.can_lift() {
                let rect = down_entry.get_bounding_client_rect();
                if let Some(el) = down_entry.dyn_ref::<web_sys::HtmlElement>() {
                    let style = web_sys::HtmlElement::style(el);
                    let _ = style.set_property("--hold-duration", &format!("{HOLD_TO_LIFT_MS}ms"));
                    let _ = style.set_property("--hold-x", &format!("{}px", at.0 - rect.left()));
                    let _ = style.set_property("--hold-y", &format!("{}px", at.1 - rect.top()));
                }
                let _ = down_entry.set_attribute(HOLD_ATTR, "");
                let timer_state = down_state.clone();
                let timer_entry = down_entry.clone();
                let generation = grab.generation;
                let timer_sink = sink.clone();
                grab.hold = set_timeout_with_handle(
                    move || {
                        let mut grab = timer_state.borrow_mut();
                        grab.hold = None;
                        let _ = timer_entry.remove_attribute(HOLD_ATTR);
                        let current =
                            grab.generation == generation && matches!(grab.phase, Phase::Pressed);
                        // A pane closed mid-hold: its entry is gone, and so is
                        // anything to lift.
                        if !current || !timer_entry.is_connected() {
                            return;
                        }
                        if timer_sink.begin(grab.origin) {
                            grab.phase = Phase::Lifted;
                        }
                    },
                    Duration::from_millis(HOLD_TO_LIFT_MS),
                )
                .ok();
            }
        });

    let move_entry = entry.clone();
    let move_state = state.clone();
    let on_move =
        Closure::<dyn Fn(web_sys::PointerEvent)>::new(move |ev: web_sys::PointerEvent| {
            let mut grab = move_state.borrow_mut();
            if ev.pointer_id() != grab.pointer {
                return;
            }
            let at = (f64::from(ev.client_x()), f64::from(ev.client_y()));
            let phase = grab.phase;
            match phase {
                Phase::Idle => {}
                Phase::Pressed => {
                    let (dx, dy) = (at.0 - grab.origin.0, at.1 - grab.origin.1);
                    if dx.hypot(dy) > PAN_THRESHOLD_PX {
                        grab.cancel_hold(&move_entry);
                        grab.phase = Phase::Panning;
                        pan(&mut grab, at);
                    }
                }
                Phase::Panning => pan(&mut grab, at),
                Phase::Lifted => {
                    drop(grab);
                    move_sink.moved(at);
                }
            }
        });

    let up_entry = entry.clone();
    let up_state = state.clone();
    let release_sink = up_sink.clone();
    let on_up = Closure::<dyn Fn(web_sys::PointerEvent)>::new(move |ev: web_sys::PointerEvent| {
        let mut grab = up_state.borrow_mut();
        if ev.pointer_id() != grab.pointer || matches!(grab.phase, Phase::Idle) {
            return;
        }
        let commit = ev.type_() == "pointerup";
        grab.cancel_hold(&up_entry);
        let phase = std::mem::take(&mut grab.phase);
        let _ = up_entry.remove_attribute(PAN_ATTR);
        match phase {
            Phase::Panning if commit => {
                let resting = now_ms() - grab.last.2 > FLING_REST_MS;
                let velocity = grab.velocity;
                let scroller = grab.scroller.take();
                if let Some(scroller) = scroller.filter(|_| !resting) {
                    let generation = grab.generation;
                    drop(grab);
                    fling(up_state.clone(), scroller, velocity, generation, now_ms());
                }
            }
            Phase::Lifted => {
                drop(grab);
                release_sink.end(commit);
            }
            _ => {}
        }
    });

    let listeners = vec![
        ("pointermove", on_hover),
        ("pointerleave", on_leave),
        ("pointerdown", on_down),
        ("pointermove", on_move),
        ("pointerup", on_up),
    ];
    for (name, listener) in &listeners {
        let _ = entry.add_event_listener_with_callback(name, listener.as_ref().unchecked_ref());
    }
    // pointercancel and lost capture reuse the end handler, registered
    // and removed once.
    let on_up = listeners.last().expect("end listener").1.as_ref();
    for name in ["pointercancel", "lostpointercapture"] {
        let _ = entry.add_event_listener_with_callback(name, on_up.unchecked_ref());
    }
    let owned = StoredValue::new_local(Some((entry.clone(), state, up_sink, listeners)));
    on_cleanup(move || {
        let Some(Some((entry, state, sink, listeners))) = owned.try_update_value(Option::take)
        else {
            return;
        };
        for (name, listener) in &listeners {
            let _ =
                entry.remove_event_listener_with_callback(name, listener.as_ref().unchecked_ref());
        }
        let on_up = listeners.last().expect("end listener").1.as_ref();
        for name in ["pointercancel", "lostpointercapture"] {
            let _ = entry.remove_event_listener_with_callback(name, on_up.unchecked_ref());
        }
        let mut grab = state.borrow_mut();
        grab.cancel_hold(&entry);
        grab.fling.stop();
        grab.generation = grab.generation.wrapping_add(1);
        let lifted = matches!(grab.phase, Phase::Lifted);
        grab.phase = Phase::Idle;
        grab.scroller = None;
        let _ = entry.release_pointer_capture(grab.pointer);
        let _ = entry.remove_attribute(PAN_ATTR);
        drop(grab);
        if lifted {
            sink.end(false);
        }
    });
}

/// Scroll the scroller so content follows the pointer; keeps a smoothed
/// velocity for the fling.
fn pan(grab: &mut Grab, at: (f64, f64)) {
    let now = now_ms();
    let dt = (now - grab.last.2).max(1.0);
    let instant = ((at.0 - grab.last.0) / dt, (at.1 - grab.last.1) / dt);
    grab.velocity = (
        instant.0 * 0.8 + grab.velocity.0 * 0.2,
        instant.1 * 0.8 + grab.velocity.1 * 0.2,
    );
    grab.last = (at.0, at.1, now);
    if let Some(scroller) = &grab.scroller {
        let x = grab.scroll_start.0 - (at.0 - grab.origin.0);
        let y = grab.scroll_start.1 - (at.1 - grab.origin.1);
        scroller.scroll_to_with_x_and_y(x, y);
    }
}

/// Carry a released pan on, decaying, until it stops or a newer
/// generation claims it.
fn fling(
    state: Rc<RefCell<Grab>>,
    scroller: web_sys::Element,
    velocity: (f64, f64),
    generation: u32,
    last: f64,
) {
    let weak = Rc::downgrade(&state);
    let velocity = Cell::new(velocity);
    let last = Cell::new(last);
    state.borrow().fling.arm(move || {
        let Some(state) = weak.upgrade() else {
            return false;
        };
        if state.borrow().generation != generation || !scroller.is_connected() {
            return false;
        }
        let now = now_ms();
        let dt = (now - last.replace(now)).clamp(1.0, 32.0);
        let decay = FLING_DECAY.powf(dt / 16.0);
        let (vx, vy) = velocity.get();
        let speed = (vx * decay, vy * decay);
        velocity.set(speed);
        if speed.0.hypot(speed.1) < FLING_MIN_SPEED {
            return false;
        }
        let (left, top) = (
            f64::from(scroller.scroll_left()),
            f64::from(scroller.scroll_top()),
        );
        scroller.scroll_to_with_x_and_y(left - speed.0 * dt, top - speed.1 * dt);
        true
    });
}

/// Whether the event landed on empty space: nothing to select or press
/// under the pointer.
fn is_empty_space(ev: &web_sys::PointerEvent) -> bool {
    let Some(el) = ev
        .target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
    else {
        return false;
    };
    if el.closest(NOT_EMPTY).ok().flatten().is_some() {
        return false;
    }
    !text_at(&el, f64::from(ev.client_x()), f64::from(ev.client_y()))
}

/// Whether a direct text run of `el` has glyphs over (x, y).
fn text_at(el: &web_sys::Element, x: f64, y: f64) -> bool {
    let Some(doc) = el.owner_document() else {
        return false;
    };
    let Ok(range) = doc.create_range() else {
        return false;
    };
    let children = el.child_nodes();
    for i in 0..children.length() {
        let Some(node) = children.item(i) else {
            continue;
        };
        if node.node_type() != web_sys::Node::TEXT_NODE
            || node.text_content().is_none_or(|t| t.trim().is_empty())
        {
            continue;
        }
        if range.select_node_contents(&node).is_err() {
            continue;
        }
        let Some(rects) = range.get_client_rects() else {
            continue;
        };
        for r in 0..rects.length() {
            if let Some(rect) = rects.item(r)
                && x >= rect.left() - TEXT_SLOP_PX
                && x <= rect.right() + TEXT_SLOP_PX
                && y >= rect.top() - TEXT_SLOP_PX
                && y <= rect.bottom() + TEXT_SLOP_PX
            {
                return true;
            }
        }
    }
    false
}

/// The nearest scrollable element from `from` up to (and including) `entry`.
fn scroller_for(from: &web_sys::Element, entry: &web_sys::Element) -> Option<web_sys::Element> {
    let window = web_sys::window()?;
    let mut node = Some(from.clone());
    while let Some(el) = node {
        let overflows = el.scroll_height() > el.client_height() + 1
            || el.scroll_width() > el.client_width() + 1;
        if overflows && let Ok(Some(style)) = window.get_computed_style(&el) {
            let scrolls = |axis: &str| {
                matches!(
                    style.get_property_value(axis).as_deref(),
                    Ok("auto") | Ok("scroll") | Ok("overlay")
                )
            };
            if scrolls("overflow-y") || scrolls("overflow-x") {
                return Some(el);
            }
        }
        if el == *entry {
            return None;
        }
        node = el.parent_element();
    }
    None
}

fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}
