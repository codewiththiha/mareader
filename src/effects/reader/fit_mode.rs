//! Fit-mode effect: recomputes the render scale while FitMode::Width/Page is
//! active. The LAYOUT follows every `container_size` frame (a sidebar slide
//! must not let the stretched DOM and the virtualized heights diverge); the
//! crisp RENDER is debounced 180ms after the size settles, so a slide yields
//! exactly one re-render.
//!
//! The zoom machinery (gesture animation, anchoring, commit) lives in the
//! sibling `zoom` module; `fit_effect` hands scale changes to it via
//! `request_zoom`/`commit_scale` and reads `gesture_owns_layout` /
//! `take_commit_echo` to stay out of a gesture's way.

use std::time::Duration;

use leptos::prelude::*;

use pdf_core::layout::TOOLBAR_H;
use pdf_core::math::{FitMode, constrained_scale, fit_scale};
use virtual_list_leptos::Virtualizer;

use crate::state::{ReaderState, SidebarMode};

use super::zoom::{commit_scale, gesture_owns_layout, relayout_to, take_commit_echo};

/// Must be called once from the app root (ReaderPage).
pub fn fit_effect(
    state: ReaderState,
    // Sidebar open/close re-runs fit so the page re-centers (app chrome
    // state passed in explicitly).
    sidebar: RwSignal<SidebarMode>,
    virtualizer: Virtualizer,
) {
    // Last page we computed a fit for. Doubles as the first-run marker: it
    // starts at 0 and is only ever written with a real (>= 1) page once the
    // document is open, so "no fit computed yet" and "the document is
    // opening" are the same state. A page-only change must NOT follow the
    // layout on the same frame (that would zoom on every row boundary while
    // the reader is scrolling); it waits for the existing debounce.
    let last_fit_page: StoredValue<u32> = StoredValue::new(0);

    Effect::new(move |_| {
        let fit = state.viewer.fit.get();
        let (cw, ch) = state.viewer.container_size.get();
        // Tracked: a wide plate scrolling into view is the same kind of
        // "the space the page needs changed" as the sidebar opening.
        let page = state.viewer.page.get();
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
        let _sidebar_open = sidebar.get() != SidebarMode::None;
        let Some(p1) = state.document.page1_size.get() else {
            return;
        };
        // The page under the eyes, not page 1. A landscape insert is cropped
        // (and a following portrait page stays over-shrunk) if we keep using
        // the first sheet's size for every page.
        let (pw, ph) = state.document.metrics.intrinsic.with(|pages| {
            let i = page.saturating_sub(1) as usize;
            match pages.get(i) {
                Some(s) if s.width > 0.0 && s.height > 0.0 => (s.width, s.height),
                _ => (p1.width, p1.height),
            }
        });
        // First run is the document opening, which always fits.
        let prev_page = last_fit_page.get_value();
        let first_run = prev_page == 0;
        let page_changed = prev_page != 0 && prev_page != page;
        last_fit_page.set_value(page);

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
                state.viewer.zoom.scale.get_untracked(),
            );
            // A fit mode IS a deliberate choice, so it owns the ceiling too.
            // Without this, leaving fit mode would resurrect a `desired_scale`
            // from some earlier gesture and the page would jump to it.
            state.viewer.zoom.desired.set(t);
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
                state.viewer.zoom.scale.get_untracked(),
            );
            let desired = state.viewer.zoom.desired.get_untracked();
            constrained_scale(desired, fit_w)
        } else {
            // Container not measured yet: a zero width would "fit" nothing and
            // slam the page to the minimum scale.
            return;
        };

        // --- the sidebar slide ------------------------------------------------
        // The `<aside>` animates its width over 300ms, so `container_size`
        // arrives as a burst of per-frame values. FREEZING the scale through
        // that burst (an earlier approach) keeps the page host wider than the
        // content box it now has to fit in — and because the host is a flex
        // child, the browser SQUISHES it: width shrinks, the inline height
        // doesn't, and a letter page goes from a 0.77 aspect to 0.61. The page
        // is visibly distorted, then snaps back at the end.
        //
        // Following the slide in `display_scale` ALONE is just as wrong: the
        // stretch effect re-sizes every mounted host to the new scale in the
        // same frame, but the virtualizer's heights — and therefore every
        // item top — would still be at the OLD scale, so pages gap and
        // overlap for the whole slide and only snap flush at the commit
        // render. The layout must move in LOCKSTEP with the stretched DOM:
        // same frame, same factor, through the same relayout a zoom gesture
        // uses.
        //
        // The rapid-fire rescales that implies are safe: `zoom_animating` is
        // held true for the whole slide (the cheap stretched raster shows,
        // renders are suspended, the DOM scroll echo is gated), and
        // `relayout_to` re-asserts its scroll write on the next animation
        // frame — once the spacer has actually been laid out at the new
        // height — so the stale-scrollHeight clamp cannot compound across
        // the burst.
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
            let cur = state.viewer.zoom.display.get_untracked();
            if (target - cur).abs() >= 0.0005 {
                // Sidebar slide / window resize / refit: one dance. Display
                // leads (the stretch effect follows it), the layout moves in
                // the same frame, and the debounce below commits the crisp
                // render once the size settles.
                state.viewer.zoom_animating.set(true);
                relayout_to(state, target / cur, &virtualizer);
                state.viewer.zoom.display.set(target);
            }
        }

        // Nothing to do? Then do NOTHING — do not arm the timer.
        //
        // This effect TRACKS `zoom_animating`, and the timer's quiet path below
        // writes it. Arming a timer when the layout is already settled
        // therefore fed the effect its own output: timer fires -> writes
        // `zoom_animating` -> effect re-runs -> arms another timer -> forever,
        // a self-sustaining 180ms loop that re-rendered the page endlessly.
        //
        // It showed up after zooming in past the fit in a NARROW window and
        // then widening it: the page ends up at a scale that already fits and
        // is already rendered, so every run took the quiet path. The page never
        // moved, which is why the width looked stable while the reader saw
        // constant flicker. A zoom click "fixed" it only because a gesture ends
        // in `commit_scale`, whose echo makes the next run return early and
        // breaks the cycle.
        let settled = (target - state.viewer.zoom.render.get_untracked()).abs() < 0.0005
            && (target - state.viewer.zoom.display.get_untracked()).abs() < 0.0005;
        if settled && !state.viewer.zoom_animating.get_untracked() {
            return;
        }

        // Debounce: each `container_size` change re-runs this effect and clears
        // the previous timer, so the commit fires once the size has been stable
        // for ~180ms — one render per slide or per resize drag, at the end.
        let timer_virtualizer = virtualizer.clone();
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
                let cur = state.viewer.zoom.display.get_untracked();
                if (target - cur).abs() >= 0.0005 {
                    relayout_to(state, target / cur, &timer_virtualizer);
                    state.viewer.zoom.display.set(target);
                }
                let prev = state.viewer.zoom.render.get_untracked();
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
