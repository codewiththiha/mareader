//! Fit-mode effect: recomputes the render scale while FitMode::Width or
//! FitMode::Page is active. Runs once from the app root (ReaderView) so fit
//! works in BOTH the single and continuous views, whichever is mounted.
//!
//! Reactively tracks `fit`, `container_size`, and `page1_size`; when a fit mode
//! is active it computes the matching scale and writes `viewer.scale` +
//! `viewer.render_scale`. `scale` is read untracked so the write-back does not
//! retrigger this effect (no loop).
//!
//! The scale write is DEBOUNCED: the sidebar `<aside>` animates its width over
//! 300ms, emitting a burst of `container_size` writes, and an immediate
//! recompute per frame would cancel every in-flight render (`PageCanvas`
//! re-renders on a scale change), flashing the visible pages. Scheduling the
//! recompute 120ms after the size has been stable yields exactly one re-render
//! per sidebar toggle, at the end of the slide. `container_size` itself stays
//! live — page tracking and the visible-page math still need it — only the
//! scale write is debounced. Changes that leave the fit scale essentially
//! unchanged are skipped: if the recomputed scale is within `0.0005` of the
//! current `render_scale`, the write is a no-op and does not force a
//! full re-render of every `PageCanvas`.
//!
//! ---------------------------------------------------------------------------
//! This module also owns the ZOOM COORDINATOR (`request_zoom` / `zoom_system`).
//!
//! The rule that makes zoom feel like Preview: **a zoom is a layout animation
//! of bitmaps we have already painted, followed by ONE crisp re-render** — it
//! is never a render-driven relayout.
//!
//! What used to happen: a control wrote `scale` and `render_scale` at the same
//! instant. Every mounted `PageCanvas` cancelled its bitmap and started
//! rasterising; page wrappers kept their OLD `top:` and the spacer its OLD
//! height until each render resolved and reported geometry back. So for a few
//! frames the column was laid out at the old scale with new-scale pages landing
//! in it one by one, and the scroll offset — untouched — pointed at a
//! completely different page by the time it settled. That is the "zoom jumps to
//! another page" bug, and the intermediate states are the flicker.
//!
//! What happens now, per gesture:
//!   1. `request_zoom` posts `(target, animate, token)`. Controls never write
//!      `scale`/`render_scale` themselves.
//!   2. `zoom_system` drives `display_scale` over `ZOOM_ANIM_MS` on rAF. Each
//!      frame it rescales `doc.page_heights` by the frame's factor and
//!      re-anchors `scroll_top` with `anchored_scroll`, so layout and scroll
//!      move together, in the same frame, and the point under the viewport
//!      centre never moves. Pages CSS-stretch; nothing renders.
//!   3. On settle it writes `scale` + `render_scale` once and drops
//!      `zoom_animating`, releasing exactly one crisp render pass.
//!
//! Retargeting is first-class: a new request mid-flight re-aims from wherever
//! the animation currently is (mashing `+` accelerates smoothly instead of
//! queueing). The token guards against a stale frame resurrecting a cancelled
//! animation. `prefers-reduced-motion` collapses the animation to a single
//! anchored step — still anchored, just instant.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;

use crate::core::layout::{anchored_scroll, PAGE_GAP};
use crate::core::math::{clamp_scale, fit_scale, FitMode};
use crate::core::state::AppState;

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
fn viewport_h(state: AppState) -> f64 {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("page-list"))
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
pub fn request_zoom(state: AppState, target: f64, animate: bool) {
    let target = clamp_scale(target);
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
}

/// Applies a scale change to the layout IMMEDIATELY and atomically: page
/// heights are rescaled by `factor` and the scroll is re-anchored in the same
/// synchronous step, so no frame is ever laid out at a mixed scale.
///
/// This is what a "relayout" means here — pure arithmetic on already-known
/// geometry. No render is involved, and none is waited for.
pub fn relayout_to(state: AppState, factor: f64) {
    if !(factor > 0.0) || !factor.is_finite() || (factor - 1.0).abs() < 1e-12 {
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
        if let Some(list) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("page-list"))
        {
            let _ = list.set_scroll_top(new_st.round() as i32);
        }
    }
}

/// The zoom coordinator. Must be called once from the app root (ReaderView),
/// next to `fit_effect`.
///
/// Owns `display_scale`, `zoom_animating`, `scale` and `render_scale` for the
/// duration of a gesture. Nothing else may write them while a zoom is running.
pub fn zoom_system(state: AppState) {
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
            state.viewer.scale.set(target);
            state.viewer.render_scale.set(target);
            state.viewer.zoom_animating.set(false);
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
        let step_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
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
fn commit_scale(state: AppState, s: f64) {
    state.viewer.display_scale.set(s);
    state.viewer.scale.set(s);
    state.viewer.render_scale.set(s);
    state.viewer.zoom_animating.set(false);
}

/// Must be called once from the app root (ReaderView).
pub fn fit_effect(state: AppState) {
    // Width of the window at the last refit, used to tell a WINDOW resize from
    // a sidebar slide: both move `container_size`, but only the former moves
    // `window.innerWidth`. No timers, no guessing.
    let last_win_w: StoredValue<f64> = StoredValue::new(f64::NAN);

    Effect::new(move |_| {
        let fit = state.viewer.fit.get();
        if fit == FitMode::None {
            return;
        }
        let (cw, ch) = state.viewer.container_size.get();
        let Some(p) = state.doc.page1_size.get() else {
            return;
        };

        let win_w = web_sys::window()
            .and_then(|w| w.inner_width().ok())
            .and_then(|v| v.as_f64())
            .unwrap_or(f64::NAN);
        let prev_win_w = last_win_w.get_value();
        // First run (NaN) is the document opening, which always fits.
        let first_run = prev_win_w.is_nan();
        let window_resized = first_run || (win_w - prev_win_w).abs() >= 0.5;
        if window_resized {
            last_win_w.set_value(win_w);
        }

        let target = fit_scale(
            fit,
            cw,
            ch,
            p.width,
            p.height,
            48.0,
            state.viewer.scale.get_untracked(),
        );

        // --- the sidebar slide ------------------------------------------------
        // The `<aside>` animates its width over 300ms, so `container_size`
        // arrives as a burst of per-frame values. FREEZING the scale through
        // that burst (the previous approach) keeps the page host wider than the
        // content box it now has to fit in — and because the host is a flex
        // child, the browser SQUISHES it: width shrinks, the inline height
        // doesn't, and a letter page goes from a 0.77 aspect to 0.61. The page
        // is visibly distorted, then snaps back at the end. That snap is the
        // "flicker then instantly switch" being reported.
        //
        // So: follow the slide continuously in LAYOUT only. Each frame writes
        // `display_scale` (pages CSS-stretch, aspect preserved) and re-anchors
        // the scroll, with `zoom_animating` held true so nothing renders. The
        // debounce below then commits ONE crisp render when the slide settles.
        // Same rule as a zoom gesture, same machinery — a smooth ride, one
        // render, and no distortion at any point along the way.
        if !first_run {
            let cur = state.viewer.display_scale.get_untracked();
            if (target - cur).abs() >= 0.0005 {
                state.viewer.zoom_animating.set(true);
                relayout_to(state, target / cur);
                state.viewer.display_scale.set(target);
            }
        }

        // Debounce: each `container_size` change re-runs this effect and clears
        // the previous timer, so the commit fires once the size has been stable
        // for ~120ms — one render per slide or per resize drag, at the end.
        let handle = set_timeout_with_handle(
            move || {
                if first_run {
                    // Opening a document: no layout to animate from.
                    commit_scale(state, target);
                    return;
                }
                let prev = state.viewer.render_scale.get_untracked();
                if (target - prev).abs() >= 0.0005 {
                    commit_scale(state, target);
                } else {
                    // Already rendered at this scale (e.g. the sidebar returned
                    // to where it started): just release the gate.
                    state.viewer.zoom_animating.set(false);
                }
            },
            Duration::from_millis(120),
        )
        .ok();
        on_cleanup(move || {
            if let Some(h) = handle {
                h.clear();
            }
        });
    });
}
