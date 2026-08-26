//! The zoom coordinator: a zoom is a layout animation of bitmaps we already
//! painted, followed by one crisp re-render. `request_zoom` posts
//! `(target, animate, token)`; `zoom_system` drives `display_scale` on rAF,
//! rescales `css_heights`, and asks the virtualizer to rescale/anchor the
//! continuous layout each frame.
//!
//! Split out of `fit.rs`: fit (`fit_effect`) and zoom are two systems sharing
//! a small hand-off (`commit_scale` / `gesture_owns_layout` /
//! `take_commit_echo`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use leptos::prelude::*;
use virtual_list_leptos::{ScrollMode, Virtualizer};

use pdf_core::math::clamp_scale;

use crate::state::ReaderState;

/// rAF step that can re-arm itself. StoredValue already wraps a RefCell, so we
/// only need one extra Rc for the Weak self-reference the loop upgrades.
type StepSlot = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// Duration of the zoom layout animation. Long enough to read as motion, short
/// enough that the crisp render feels immediate.
const ZOOM_ANIM_MS: f64 = 200.0;

fn ease_out_cubic(t: f64) -> f64 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

fn prefers_reduced_motion() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok())
        .flatten()
        .map(|m| m.matches())
        .unwrap_or(false)
}

pub fn request_zoom(state: ReaderState, target: f64, animate: bool) {
    let target = clamp_scale(target);
    state.viewer.zoom.desired.set(target);
    set_gesture_owns_layout(true);
    let token = ZOOM_TOKEN.with(|t| {
        let next = t.get() + 1;
        t.set(next);
        next
    });
    state
        .viewer
        .zoom
        .request
        .set(Some((target, animate, token)));
}

thread_local! {
    static ZOOM_TOKEN: Cell<u64> = const { Cell::new(0) };
    static ZOOM_GESTURE: Cell<bool> = const { Cell::new(false) };
    static COMMIT_ECHO: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn gesture_owns_layout() -> bool {
    ZOOM_GESTURE.with(|g| g.get())
}

fn set_gesture_owns_layout(owns: bool) {
    ZOOM_GESTURE.with(|g| g.set(owns));
}

pub(super) fn take_commit_echo() -> bool {
    COMMIT_ECHO.with(|c| c.replace(false))
}

pub fn relayout_to(state: ReaderState, factor: f64, virtualizer: &Virtualizer) {
    if factor <= 0.0 || !factor.is_finite() || (factor - 1.0).abs() < 1e-12 {
        return;
    }

    state.document.metrics.css_heights.update(|heights| {
        for height in heights.iter_mut() {
            *height *= factor;
        }
    });

    let heights = state
        .document
        .metrics
        .css_heights
        .with_untracked(|heights| heights.clone());
    if heights.is_empty() {
        return;
    }

    virtualizer.rescale(factor, {
        let heights = heights.clone();
        move |index| heights.get(index).copied().unwrap_or(0.0)
    });

    let scroll_top = virtualizer.scroll_offset().get_untracked();
    if (scroll_top - state.viewer.scroll_top.get_untracked()).abs() >= 0.5 {
        state.viewer.scroll_top.set(scroll_top);
    }

    // Growing content needs one more scroll assertion, one frame later.
    //
    // `rescale` clamps the new scroll offset against the NEW layout and emits
    // a scroll write that `apply()` performs synchronously — but the spacer
    // `<div>` that gives the scroll container its `scrollHeight` is patched by
    // Leptos only after this synchronous call returns. The browser therefore
    // clamps the write to the OLD, shorter scrollHeight. One frame later the
    // spacer has grown, yet the scroll position stays pinned at the stale
    // clamp. Mid-document the anchor correction hides the error; at the end
    // of the document the clamp distance is at its maximum, which is why the
    // jump is only visible on the last pages (and only when the content is
    // growing — a sidebar CLOSING, not opening).
    //
    // Re-assert the target scroll on the next animation frame, after the
    // spacer has been laid out at its new height.
    if factor > 1.0 {
        let v = virtualizer.clone();
        let target_scroll = scroll_top;
        request_animation_frame(move || {
            v.scroll_to_offset(target_scroll, ScrollMode::Instant);
        });
    }
}

pub fn zoom_system(state: ReaderState, virtualizer: Virtualizer) {
    // While any scale animation is in flight (a zoom gesture, a sidebar slide,
    // a window-resize drag — all raise `zoom_animating` and all end in
    // `commit_scale`, which clears it), the virtualizer's DOM scroll echo must
    // not overwrite the core anchor: the browser fires those per-frame scroll
    // events one frame late, so the echo is stale and the next anchored
    // rescale pins from it, making the content oscillate instead of gliding.
    // The echo is re-adopted the moment the animation commits.
    {
        let v = virtualizer.clone();
        Effect::new(move |_| {
            if state.viewer.zoom_animating.get() {
                v.suspend_scroll_feedback();
            } else {
                v.resume_scroll_feedback();
            }
        });
    }

    let anim_slot = StoredValue::new_local(None::<StepSlot>);
    let live_token = StoredValue::new_local(Rc::new(Cell::new(0u64)));

    Effect::new(move |_| {
        let Some((target, animate, token)) = state.viewer.zoom.request.get() else {
            return;
        };

        let live = live_token.get_value();
        live.set(token);

        let from = state.viewer.zoom.display.get_untracked();
        if (target - from).abs() < 1e-9 {
            commit_scale(state, target);
            virtualizer.resume_measurements();
            return;
        }

        virtualizer.suspend_measurements();

        let commit = {
            let virtualizer = virtualizer.clone();
            move |final_scale: f64| {
                commit_scale(state, final_scale);
                virtualizer.resume_measurements();
            }
        };

        if !animate || prefers_reduced_motion() {
            relayout_to(state, target / from, &virtualizer);
            commit(target);
            return;
        }

        state.viewer.zoom_animating.set(true);

        let start = js_sys::Date::now();
        let step_slot: StepSlot = Rc::new(RefCell::new(None));
        let step_self = Rc::downgrade(&step_slot);
        let live_step = live.clone();
        let step_virtualizer = virtualizer.clone();
        let step: Rc<dyn Fn()> = Rc::new(move || {
            if live_step.get() != token {
                return;
            }
            let t = ((js_sys::Date::now() - start) / ZOOM_ANIM_MS).clamp(0.0, 1.0);
            let eased = ease_out_cubic(t);
            let want = from + (target - from) * eased;
            let cur = state.viewer.zoom.display.get_untracked();

            relayout_to(state, want / cur, &step_virtualizer);
            state.viewer.zoom.display.set(want);

            if t >= 1.0 {
                commit(target);
                return;
            }
            if let Some(next) = step_self.upgrade().and_then(|slot| slot.borrow().clone()) {
                request_animation_frame(move || next());
            }
        });
        *step_slot.borrow_mut() = Some(step.clone());
        anim_slot.set_value(Some(step_slot));
        request_animation_frame(move || step());
    });
}

pub(super) fn commit_scale(state: ReaderState, scale: f64) {
    set_gesture_owns_layout(false);
    COMMIT_ECHO.with(|c| c.set(true));
    state.viewer.zoom.display.set(scale);
    state.viewer.zoom.scale.set(scale);
    state.viewer.zoom.render.set(scale);
    state.viewer.zoom_animating.set(false);
}
