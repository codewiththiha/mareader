//! Fit-mode effect + the zoom coordinator.
//!
//! Fit: recomputes the render scale while FitMode::Width/Page is active,
//! debounced 120ms after `container_size` settles so a sidebar slide yields
//! exactly one re-render.
//!
//! Zoom: a zoom is a layout animation of bitmaps we already painted, followed
//! by ONE crisp re-render. `request_zoom` posts `(target, animate, token)`;
//! `zoom_system` drives `display_scale` on rAF, re-scaling `page_heights` and
//! re-anchoring `scroll_top` each frame; on settle it writes `scale` +
//! `render_scale` once. Retargeting is first-class (mashing `+` re-aims from
//! wherever the animation is), and `prefers-reduced-motion` collapses the
//! animation to a single anchored step.

use std::cell::{Cell, RefCell};

/// rAF step that can re-arm itself. StoredValue already wraps a RefCell, so
/// we only need one extra Rc for the Weak self-reference the loop upgrades.
type StepSlot = std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>>>;
use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use pdf_core::layout::{anchored_scroll, total_height_css, PAGE_GAP, TOOLBAR_H};
use pdf_core::math::{clamp_scale, constrained_scale, fit_scale, page_intrinsic, FitMode};
use crate::state::{ViewerState, SidebarMode};
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
fn gesture_owns_layout() -> bool {
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
fn take_commit_echo() -> bool {
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
fn commit_scale(state: ViewerState, s: f64) {
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

/// Must be called once from the app root (ReaderView).
pub fn fit_effect(state: ViewerState) {
    // Width of the window at the last refit, used to tell a WINDOW resize from
    // a sidebar slide: both move `container_size`, but only the former moves
    // `window.innerWidth`. No timers, no guessing.
    let last_win_w: StoredValue<f64> = StoredValue::new(f64::NAN);
    // Last page we computed a fit for. A page-only change must NOT follow
    // the layout on the same frame (that would zoom on every row boundary
    // while the reader is scrolling); it waits for the existing debounce.
    let last_fit_page: StoredValue<u32> = StoredValue::new(0);

    Effect::new(move |_| {
        let fit = state.viewer.fit.get();
        let (cw, ch) = state.viewer.container_size.get();
        // Tracked: a wide plate scrolling into view is the same kind of
        // "the space the page needs changed" as the sidebar opening.
        let page = state.viewer.page.get();
        let widths = state.doc.page_widths.get();
        let intrins_h = state.doc.page_sizes.get();
        // A zoom GESTURE owns the layout while it runs.
        //
        // `apply_zoom` writes `fit` (to None) and then calls `request_zoom`, so
        // this effect re-runs at the very start of every zoom. Without a guard
        // it recomputed the same target and wrote `display_scale` straight to
        // it — the rAF animation was then interpolating from a value that had
        // already arrived, so every zoom SNAPPED in a single frame.
        //
        // The flag must distinguish a GESTURE from this effect's own slide
        // following, which also raises `zoom_animating`. Keying off
        // `zoom_animating` alone would make the effect block itself: the first
        // container_size of a sidebar slide would set it, and every subsequent
        // frame of that slide would bail out — turning the smooth slide into a
        // one-frame jump, i.e. trading one snap for another.
        //
        // `zoom_animating` is still read REACTIVELY so that when the gesture
        // commits and the flag drops, this effect re-runs and reconciles the
        // settled scale against the space available — that is what still
        // shrinks a zoom-in that would overflow a narrow window.
        //
        // The ownership flag alone is the gate — NOT `zoom_animating && owned`.
        // `request_zoom` claims ownership before `zoom_system` has raised
        // `zoom_animating` (the request is a signal write; the system reacts to
        // it afterwards). During that gap the guard would still be open, and
        // this effect — re-run by the `fit` write in `apply_zoom` — would move
        // `display_scale` all the way to the target. `zoom_system` then started
        // its animation with `from == target` and had nothing left to
        // interpolate, which is exactly the snap that survived the first fix.
        let _animating = state.viewer.zoom_animating.get();
        if gesture_owns_layout() {
            return;
        }
        // `commit_scale` writes `scale`/`display_scale`/`render_scale` and
        // releases ownership, and this effect re-runs as a result. That run
        // must NOT re-enter the slide path: doing so raised `zoom_animating`
        // again and armed another commit, a self-feeding loop that turned one
        // render into dozens.
        //
        // Comparing the container width is NOT a reliable way to detect it —
        // the effect legitimately runs twice for each container size during a
        // slide (measured), so half of a real slide's frames would be
        // misclassified as "just committed". An explicit one-shot marker set by
        // `commit_scale` is unambiguous.
        let just_committed = take_commit_echo();
        // Tracked (and deliberately unused) so a sidebar toggle re-runs this
        // effect the moment it starts, not only once the animation has begun
        // moving the container. The value itself no longer matters: the page
        // is sized from the space that is actually available, whatever took it.
        let _sidebar_open = state.sidebar.get() != SidebarMode::None;
        let Some(p1) = state.doc.page1_size.get() else {
            return;
        };
        // The page under the eyes, not page 1. A landscape insert is cropped
        // (and a following portrait page stays over-shrunk) if we keep using
        // the first sheet's size for every page.
        let (pw, ph) = page_intrinsic(page, &widths, &intrins_h, p1.width, p1.height);
        let prev_page = last_fit_page.get_value();
        let page_changed = prev_page != 0 && prev_page != page;
        last_fit_page.set_value(page);

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


        // The scale this run is aiming at.
        //
        // With a fit mode, that is whatever fits the new container. WITHOUT one
        // (the reader has zoomed by hand, so `fit` is `None`) there is nothing
        // to recompute — but the sidebar must still push the page around, so
        // the zoom is carried across the slide PROPORTIONALLY: the page keeps
        // the same fraction of the container width it had before, which is what
        // makes opening the panel shrink the page and closing it grow the page
        // back to exactly where it was.
        //
        // This is deliberately scoped to a sidebar slide. A window resize with
        // no fit mode leaves the scale alone, which is what every other reader
        // does: making the window bigger must not silently re-zoom the document.
        // The scrollport now runs the full window height so pages can slide
        // under the glass toolbar, so the height actually available for reading
        // is the container MINUS that inset. Without this subtraction fit-page
        // would size the sheet to a viewport 48px taller than the reader can
        // see, and the bottom of every page would sit under the bar.
        let ch_visible = (ch - TOOLBAR_H).max(1.0);

        let target = if fit != FitMode::None {
            let t = fit_scale(
                fit,
                cw,
                ch_visible,
                pw,
                ph,
                48.0,
                state.viewer.scale.get_untracked(),
            );
            // A fit mode IS a deliberate choice, so it owns the ceiling too.
            // Without this, leaving fit mode would resurrect a `desired_scale`
            // from some earlier gesture and the page would jump to it.
            state.viewer.desired_scale.set(t);
            t
        } else if cw > 1.0 {
            // NO FIT MODE: the reader picked this zoom by hand.
            //
            // Their choice is remembered in `desired_scale` and shown whenever
            // it fits. When it does not — a narrowed window, or the sidebar
            // taking room — the page is SHRUNK TO FIT instead of being cropped,
            // because a cropped page hides content with no affordance to
            // recover it. When the room comes back the page grows again, and
            // stops exactly at `desired_scale`: it is a ceiling, so the app
            // never overrides a deliberate zoom by growing past it.
            //
            // Computing from `desired_scale` (not from the current scale) is
            // what makes this lossless. The old code multiplied the live scale
            // by the container ratio each run, so a slide accumulated rounding
            // and the page never quite returned to where it started; and it
            // only ran during a sidebar slide, which is why narrowing the
            // WINDOW just cropped the page.
            let fit_w = fit_scale(
                FitMode::Width,
                cw,
                ch,
                pw,
                ph,
                48.0,
                state.viewer.scale.get_untracked(),
            );
            let desired = state.viewer.desired_scale.get_untracked();
            constrained_scale(desired, fit_w)
        } else {
            // Container not measured yet: a zero width would "fit" nothing and
            // slam the page to the minimum scale.
            return;
        };

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
        if just_committed {
            // A gesture just landed: leave it exactly where the reader put it.
            //
            // The shrink-to-fit ceiling answers "the space got smaller", NOT
            // "the reader asked for more". Reconciling here applied the ceiling
            // to the gesture itself, so from a fit-width start every `+` was
            // computed, animated, and then immediately undone — the zoom
            // control looked broken because the page could never grow past the
            // window. Zooming in past the fit is deliberate and allowed; the
            // page simply overflows and scrolls, as in every desktop reader.
            //
            // The ceiling still applies on the next real container change,
            // which is the case it was written for.
            return;
        }
        // Sidebar / window changes follow the layout on every frame so the
        // page does not squish. A PAGE change must not: scrolling through a
        // mixed-size book would zoom on every row boundary. Those wait for
        // the debounce below, which fires once the reader pauses.
        if !first_run && !page_changed {
            let cur = state.viewer.display_scale.get_untracked();
            if (target - cur).abs() >= 0.0005 {
                state.viewer.zoom_animating.set(true);
                relayout_to(state, target / cur);
                state.viewer.display_scale.set(target);
            }
        }

        // Nothing to do? Then do NOTHING — do not arm the timer.
        //
        // This effect TRACKS `zoom_animating`, and the timer's quiet path below
        // writes it. Arming a timer when the layout is already settled
        // therefore fed the effect its own output: timer fires -> writes
        // `zoom_animating` -> effect re-runs -> arms another timer -> forever,
        // a self-sustaining 120ms loop that re-rendered the page endlessly.
        //
        // It showed up after zooming in past the fit in a NARROW window and
        // then widening it: the page ends up at a scale that already fits and
        // is already rendered, so every run took the quiet path. The page never
        // moved, which is why the width looked stable while the reader saw
        // constant flicker. A zoom click "fixed" it only because a gesture ends
        // in `commit_scale`, whose echo makes the next run return early and
        // breaks the cycle.
        let settled = (target - state.viewer.render_scale.get_untracked()).abs() < 0.0005
            && (target - state.viewer.display_scale.get_untracked()).abs() < 0.0005;
        if settled && !state.viewer.zoom_animating.get_untracked() {
            return;
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
                // A page-change refit skipped the per-frame relayout (so
                // scrolling a mixed-size book does not zoom on every row).
                // Do that relayout NOW, before the crisp render, or the
                // heights stay at the old scale and the scroll teleports.
                let cur = state.viewer.display_scale.get_untracked();
                if (target - cur).abs() >= 0.0005 {
                    relayout_to(state, target / cur);
                    state.viewer.display_scale.set(target);
                }
                let prev = state.viewer.render_scale.get_untracked();
                if (target - prev).abs() >= 0.0005 {
                    commit_scale(state, target);
                } else if state.viewer.zoom_animating.get_untracked() {
                    // Already rendered at this scale (e.g. the sidebar returned
                    // to where it started): just release the gate.
                    //
                    // Guarded because a Leptos `set` notifies even when the
                    // value is unchanged, and this effect tracks this signal —
                    // an unconditional write here is a self-retrigger.
                    state.viewer.zoom_animating.set(false);
                }
            },
            Duration::from_millis(180),
        )
        .ok();
        on_cleanup(move || {
            if let Some(h) = handle {
                h.clear();
            }
        });
    });
}
