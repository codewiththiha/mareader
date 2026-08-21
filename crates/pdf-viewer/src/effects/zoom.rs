//! The zoom coordinator: a zoom is a layout animation of bitmaps we already
//! painted, followed by ONE crisp re-render. `request_zoom` posts
//! `(target, animate, token)`; `zoom_system` drives `display_scale` on rAF,
//! re-scaling `page_heights` and re-anchoring `scroll_top` each frame; on
//! settle it writes `scale` + `render_scale` once. Retargeting is
//! first-class (mashing `+` re-aims from wherever the animation is), and
//! `prefers-reduced-motion` collapses the animation to a single anchored step.
//!
//! Split out of `fit.rs`: fit (`fit_effect`) and zoom are two systems sharing
//! a small hand-off (`commit_scale` / `gesture_owns_layout` /
//! `take_commit_echo`).

use std::cell::{Cell, RefCell};

/// rAF step that can re-arm itself. StoredValue already wraps a RefCell, so
/// we only need one extra Rc for the Weak self-reference the loop upgrades.
type StepSlot = std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>>>;
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use pdf_core::layout::{anchored_scroll, total_height_css, PAGE_GAP};
use pdf_core::math::clamp_scale;
use crate::state::ViewerState;
use crate::dom::page_list;

/// Duration of the zoom layout animation. Long enough to read as motion,
/// short enough that the crisp render feels immediate.
const ZOOM_ANIM_MS: f64 = 200.0;

/// The anchor point, as a fraction of viewport height. 0.5 = keep whatever is
/// at the centre of the viewport at the centre of the viewport.
const ANCHOR_FRAC: f64 = 0.5;

/// Standard decelerating ease. Fast off the mark, settles gently.
fn ease_out_cubic(t: f64) -> f64 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

/// True when the OS asks for reduced motion.
fn prefers_reduced_motion() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok())
        .flatten()
        .map(|m| m.matches())
        .unwrap_or(false)
}

/// The scrollport's height, used as the anchor reference.
fn viewport_h(state: ViewerState) -> f64 {
    page_list()
        .map(|el| el.client_height() as f64)
        .filter(|h| *h > 1.0)
        .unwrap_or_else(|| state.viewer.container_size.get_untracked().1)
}

/// THE single entry point for changing zoom. Every control routes through here
/// so that layout, scroll anchoring and rendering stay in one coordinator
/// instead of racing each other from a dozen call sites.
///
/// `animate = false` still anchors — it just skips the tween (used by fit,
/// window resize and other programmatic relayouts, which should look instant).
pub fn request_zoom(state: ViewerState, target: f64, animate: bool) {
    let target = clamp_scale(target);
    // A zoom gesture is the ONLY thing that changes what the reader wants.
    //
    // Recording it here — the single entry point every control and shortcut
    // already goes through — is what lets a resize shrink the page to fit
    // without destroying the choice, and what gives the page a definite place
    // to stop growing when the space returns. A resize deliberately does NOT
    // write this.
    state.viewer.desired_scale.set(target);
    // Claim the layout for this gesture, so `fit_effect` stops writing
    // `display_scale` underneath the animation. Released by `commit_scale`.
    set_gesture_owns_layout(true);
    // Monotonic token: makes every request distinct so two identical targets
    // in a row both register, and so in-flight frames can detect they are stale.
    let token = ZOOM_TOKEN.with(|t| {
        let n = t.get() + 1;
        t.set(n);
        n
    });
    state.viewer.zoom_request.set(Some((target, animate, token)));
}

thread_local! {
    static ZOOM_TOKEN: Cell<u64> = const { Cell::new(0) };
    /// True while a zoom GESTURE (not a resize/slide) owns the layout.
    ///
    /// Both `zoom_system` and `fit_effect` animate `display_scale` and both
    /// raise `zoom_animating`, so that signal cannot say WHICH of them is
    /// driving. This can, and only the gesture may lock the other out.
    static ZOOM_GESTURE: Cell<bool> = const { Cell::new(false) };
    /// Set by `commit_scale`, consumed by the `fit_effect` run it triggers.
    static COMMIT_ECHO: Cell<bool> = const { Cell::new(false) };
}

/// Whether a zoom gesture currently owns the layout.
pub(super) fn gesture_owns_layout() -> bool {
    ZOOM_GESTURE.with(|g| g.get())
}

/// Mark the start/end of a zoom gesture's ownership of the layout.
fn set_gesture_owns_layout(owns: bool) {
    ZOOM_GESTURE.with(|g| g.set(owns));
}

/// Consume the "a commit just happened" marker, returning whether it was set.
///
/// One-shot: the next `fit_effect` run after a commit is the echo of that
/// commit's own signal writes, and must not be mistaken for a resize.
pub(super) fn take_commit_echo() -> bool {
    COMMIT_ECHO.with(|c| c.replace(false))
}

/// Applies a scale change to the layout IMMEDIATELY and atomically: page
/// heights are rescaled by `factor` and the scroll is re-anchored in the same
/// synchronous step, so no frame is ever laid out at a mixed scale.
///
/// This is what a "relayout" means here — pure arithmetic on already-known
/// geometry. No render is involved, and none is waited for.
pub fn relayout_to(state: ViewerState, factor: f64) {
    if factor <= 0.0 || !factor.is_finite() || (factor - 1.0).abs() < 1e-12 {
        return;
    }
    let heights = state.doc.page_heights.get_untracked();
    if heights.is_empty() {
        return;
    }
    let vh = viewport_h(state);
    let st = state.viewer.scroll_top.get_untracked();
    let anchor = vh * ANCHOR_FRAC;
    let new_st = anchored_scroll(st, vh, &heights, PAGE_GAP, factor, anchor);

    // Heights first, then scroll: the wrappers' `top:` values are derived from
    // heights, so this order means the scroll write always lands in a column
    // that is already the right size.
    state
        .doc
        .page_heights
        .set(heights.iter().map(|h| h * factor).collect());

    if let Some(new_st) = new_st {
        state.viewer.scroll_top.set(new_st);
        if let Some(list) = page_list()
        {
            // Grow the scrollable extent BEFORE moving the scrollbar.
            //
            // `scrollTop` is clamped by the browser to the CURRENT
            // `scrollHeight - clientHeight`. Leptos applies the spacer's new
            // height in its own effect pass, i.e. after this function returns,
            // so when zooming IN the element is still the old (shorter) size
            // at this instant and a deep scroll target is silently truncated.
            // The scroll listener then reads that truncated value back into
            // `viewer.scroll_top`, so every frame of a zoom-in gesture loses a
            // little more distance and the reader drifts backwards through the
            // document — tens of pages over a full gesture, always toward the
            // end of the book, which is where the clamp bites hardest.
            //
            // Writing the spacer height synchronously here makes the extent
            // correct before the scroll write. Leptos's own effect writes the
            // identical value moments later, so this is idempotent, not a
            // competing source of truth.
            if let Some(spacer) = list
                .first_element_child()
                .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
            {
                let total = total_height_css(
                    &heights.iter().map(|h| h * factor).collect::<Vec<_>>(),
                    PAGE_GAP,
                );
                _ = spacer.style().set_property("height", &format!("{total}px"));
            }
            list.set_scroll_top(new_st.round() as i32);
        }
    }
}

/// The zoom coordinator. Must be called once from the app root (ReaderView),
/// next to `fit_effect`.
///
/// Owns `display_scale`, `zoom_animating`, `scale` and `render_scale` for the
/// duration of a gesture. Nothing else may write them while a zoom is running.
pub fn zoom_system(state: ViewerState) {
    // rAF plumbing. The step holds a Weak back-reference to its own holder so
    // it can re-arm itself; the strong Rc lives in this owner-scoped
    // StoredValue (the pattern proven in thumbnails_panel's glide).
    let anim_slot = StoredValue::new_local(None::<Rc<RefCell<Option<Rc<dyn Fn()>>>>>);
    // The token of the animation currently allowed to run. A frame whose token
    // no longer matches has been superseded and must die quietly.
    let live_token = StoredValue::new_local(Rc::new(Cell::new(0u64)));

    Effect::new(move |_| {
        let Some((target, animate, token)) = state.viewer.zoom_request.get() else {
            return;
        };

        let live = live_token.get_value();
        // Claim the animation slot; any older in-flight frame now sees a
        // mismatch on its next tick and stops.
        live.set(token);

        // Start from where the layout actually IS, not from the committed
        // `scale`. Mid-flight this is a partway value, which is exactly what
        // makes mashing `+` retarget fluidly rather than restart or queue.
        let from = state.viewer.display_scale.get_untracked();
        if (target - from).abs() < 1e-9 {
            // Nothing to move, but still commit so `scale`/render agree.
            //
            // MUST go through `commit_scale`: it is what releases the gesture's
            // claim on the layout. Hand-rolling the three signal writes here
            // (as this did) left the claim set forever, so `fit_effect` was
            // permanently locked out and the sidebar stopped resizing the page
            // at all — a dead reader, from one missing release.
            commit_scale(state, target);
            return;
        }

        // Commits the gesture: one crisp render at the final scale.
        let commit = move |final_scale: f64| commit_scale(state, final_scale);

        if !animate || prefers_reduced_motion() {
            // Instant — but still a proper anchored relayout, never a bare
            // scale write.
            relayout_to(state, target / from);
            commit(target);
            return;
        }

        state.viewer.zoom_animating.set(true);

        let start = js_sys::Date::now();
        let step_slot: StepSlot = Rc::new(RefCell::new(None));
        let step_self = Rc::downgrade(&step_slot);
        let live_step = live.clone();
        let step: Rc<dyn Fn()> = Rc::new(move || {
            // Superseded by a newer request: that request owns the layout now.
            if live_step.get() != token {
                return;
            }
            let t = ((js_sys::Date::now() - start) / ZOOM_ANIM_MS).clamp(0.0, 1.0);
            let eased = ease_out_cubic(t);
            let want = from + (target - from) * eased;
            let cur = state.viewer.display_scale.get_untracked();

            // Per-frame delta, applied to layout + scroll together.
            relayout_to(state, want / cur);
            state.viewer.display_scale.set(want);

            if t >= 1.0 {
                commit(target);
                return;
            }
            if let Some(next) = step_self.upgrade().and_then(|s| s.borrow().clone()) {
                request_animation_frame(move || next());
            }
        });
        *step_slot.borrow_mut() = Some(step.clone());
        anim_slot.set_value(Some(step_slot));
        request_animation_frame(move || step());
    });
}

/// Commit a settled scale: one crisp render at `s`, animation flag cleared.
///
/// Ordering matters — releasing `zoom_animating` before `render_scale` would
/// let the canvases render once at the stale scale first.
pub(super) fn commit_scale(state: ViewerState, s: f64) {
    // The gesture is over: hand the layout back to `fit_effect`, which will
    // re-run (it tracks `zoom_animating`) and reconcile this scale against the
    // space actually available.
    set_gesture_owns_layout(false);
    COMMIT_ECHO.with(|c| c.set(true));
    state.viewer.display_scale.set(s);
    state.viewer.scale.set(s);
    state.viewer.render_scale.set(s);
    state.viewer.zoom_animating.set(false);
}
