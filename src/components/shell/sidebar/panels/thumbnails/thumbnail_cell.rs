//! A single thumbnail cell.
//!
//! Split out of `panel.rs`: the cell owns its own render lifecycle
//! (engine registration, the cached-blit fast path, the skeleton crossfade and
//! cancellation on unmount) and is the only place that talks to the engine's
//! thumbnail lane. The panel above it only decides WHICH cells exist.

// The registry, generation guard, and render slot are shared with
// `on_cleanup` callbacks and the spawned render task, which Leptos stores in
// a `Send + Sync` slot — so these stay `Arc` + `Mutex`/atomics. This is
// single-threaded UI code, but the owner's cleanup contract demands
// thread-safe handles; `Rc` would not compile here.
use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use pdf_engine::api as engine;
use app_chrome::hooks::use_timeout::use_timeout_slot;
use crate::state::ReaderState;

use super::geometry::{CELL_W, THUMB_SCALE};

/// Delay (ms) before the skeleton pulse is removed after a thumbnail render
/// resolves.
const PULSE_STOP_MS: u64 = 400;

/// Registry of pages whose canvases are currently engine-bound. A `HashSet`
/// keeps the per-cell mount/unmount bookkeeping O(1); `Arc<Mutex>` because
/// the handles cross into `Send + Sync` `on_cleanup` slots (see the note at
/// the top of this file).
pub type ThumbRegistry = Arc<Mutex<HashSet<u32>>>;

/// The render lifecycle of one cell, as a pure state machine: the DOM side
/// (canvas blit, cover crossfade, pulse timer) reacts to the transitions,
/// while the machine itself stays free of web types so its rules are
/// unit-testable in the native test runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThumbRenderState {
    /// Mounted, and no render has completed yet. A failed render returns
    /// here, and the next heal sweep retries it while attempts remain.
    Pending,
    /// A `render_thumb` is in flight; no second render may start.
    Rendering,
    /// The engine painted this page; the cell never renders again.
    Settled,
    /// Unmounted (scrolled out, document switch, teardown): terminal.
    Unmounted,
}

/// How many times a cell may (re)start a render before it gives up and the
/// pulsing skeleton persists as the fallback.
const MAX_RENDER_ATTEMPTS: u8 = 3;

impl ThumbRenderState {
    /// A render may only start from `Pending`, and only while attempts
    /// remain: `Rendering` blocks a concurrent render, `Settled` is a
    /// terminal success, and `Unmounted` is terminal for everything.
    fn may_start(self, attempts: u8) -> bool {
        self == Self::Pending && attempts < MAX_RENDER_ATTEMPTS
    }

    fn start(self) -> Self {
        Self::Rendering
    }

    /// A successful paint is final; the cover crossfade runs, then stops.
    fn paint(self) -> Self {
        Self::Settled
    }

    /// A failed paint goes back to `Pending`; the next heal sweep retries it
    /// while attempts remain.
    fn fail(self) -> Self {
        Self::Pending
    }

    /// Unmounting wins over every other state, including a render still in
    /// flight when the cell leaves the window.
    fn unmount(self) -> Self {
        Self::Unmounted
    }

    /// Atomic storage: the cell keeps the machine in one `AtomicU8` so the
    /// render task and the cleanup callback share it without a lock.
    fn as_u8(self) -> u8 {
        self as u8
    }

    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Pending,
            1 => Self::Rendering,
            2 => Self::Settled,
            _ => Self::Unmounted,
        }
    }
}

/// One thumbnail cell: a fully-opaque, fully-blended `.thumb-canvas` under a
/// themed `.thumb-skeleton` cover that fades out once the engine render
/// resolves. Registers its canvas on mount and unregisters it on `on_cleanup`
/// (which fires when the cell scrolls out of the window, the document changes,
/// or the app tears down).
#[component]
pub fn ThumbCell(
    state: ReaderState,
    /// 1-based page number this cell renders.
    page: u32,
    /// Document generation guard, bumped on document change so a stale
    /// in-flight render can't paint into a fresh document's canvas.
    generation: Arc<AtomicU32>,
    /// Registry of pages whose canvases are currently engine-bound, kept so a
    /// cell can remove itself from it on unmount.
    bound: ThumbRegistry,
    /// Bumped when the panel's heal sweep runs, so a cell whose render lost a
    /// cache or cancellation race can retry without remounting.
    #[prop(into)]
    heal: Signal<u64>,
    /// Panel-wide staleness flag: this cell sets it when its render fails so
    /// the panel schedules a heal sweep. The sweep — not the scroll stream —
    /// is what drives healing.
    needs_heal: RwSignal<bool>,
) -> impl IntoView {
    // SYNCHRONOUS cache probe, read while the view is being built — BEFORE the
    // cell's first frame is composited. When this page's bitmap is already in
    // the engine's thumbnail cache the render will blit it in the same task the
    // cell mounts in, so the cell must mount ALREADY "loaded": cover
    // transparent, no pulse animation, no opacity transition.
    //
    // This is the fix for the residual subtle scroll flicker. The grid is
    // virtualized, so scrolling up re-mounts rows that were rendered moments
    // earlier. Every such remount used to replay the full skeleton→crossfade
    // sequence over a bitmap that was about to be painted instantly, which read
    // as a faint brightness blip on the row entering view — most visible on the
    // 2nd row from the scroll edge (buffer row 1 mounts off-screen, so the row
    // the user actually watches appear is the one that flickers) and on BOTH
    // columns of it, because both cells remount together in the same row node.
    // A cached cell now has no cover state to animate at all.
    let starts_cached = engine::has_thumb(page, THUMB_SCALE);
    let loaded = RwSignal::new(starts_cached);
    // A NodeRef onto the cover (the timer removes the pulse class from the
    // real DOM node). The pending removal is parked in a scope-owned timer
    // slot, so on_cleanup cancels it and the timer can never fire on a
    // detached cover.
    let cover_ref: NodeRef<html::Div> = NodeRef::new();
    let pulse_timer = use_timeout_slot();
    // Async work can outlive a virtualized cell: the render slot and attempt
    // counter are the machine's state, shared between the spawned render task
    // and the cleanup callback. They keep "engine painted" separate from
    // `loaded` — cached cells start visually loaded but still need one render
    // call to blit their bitmap.
    let render = Arc::new(AtomicU8::new(ThumbRenderState::Pending.as_u8()));
    let attempts = Arc::new(AtomicU8::new(0));
    // Current page drives the accent ring + badge. The badge is a z-10 overlay
    // so it stays visible on every card, not just the active one.
    let is_current = move || state.viewer.page.get() == page;
    let cid = format!("thumb-{page}");

    // Page-1 aspect drives the fixed cell geometry; the shared helper falls
    // back to a 3:4 portrait default if page1_size isn't populated yet.
    let aspect = move || state.document.page1_aspect();
    let cell_h = move || CELL_W * aspect();

    // Release the engine binding when this cell unmounts (scrolled out of the
    // window, document switch, or app teardown). `cancel_thumb` aborts an
    // in-flight render but deliberately KEEPS the cached bitmap, so scrolling
    // this row back into view repaints it instantly instead of re-rendering.
    let cid_cleanup = cid.clone();
    let page_cleanup = page;
    let bound_cleanup = bound.clone();
    let render_cleanup = render.clone();
    on_cleanup(move || {
        // Terminal for the machine: the in-flight task (if any) sees
        // `Unmounted` when it next wakes and drops its result. Routed
        // through the transition (not a raw store) so every state change
        // flows through the machine.
        let current = ThumbRenderState::from_u8(render_cleanup.load(Ordering::Relaxed));
        render_cleanup.store(current.unmount().as_u8(), Ordering::Relaxed);
        engine::cancel_thumb(&cid_cleanup);
        // WKWebView does not release a canvas backing store on DOM removal
        // alone — every close/open cycle would otherwise leak a batch of
        // IOSurfaces until GC gets around to it. Zero the backing store so
        // the panel's close costs a constant, never growth.
        if let Some(el) = app_chrome::hooks::dom::by_id(&cid_cleanup)
            && let Some(cv) = el.dyn_ref::<web_sys::HtmlCanvasElement>()
        {
            cv.set_width(0);
            cv.set_height(0);
            // Symmetry with the engine's `releaseCanvas`: it zeroes the
            // dimensions AND clears the context, so do both here.
            if let Ok(Some(ctx)) = cv.get_context("2d")
                && let Some(ctx2d) = ctx.dyn_ref::<web_sys::CanvasRenderingContext2d>()
            {
                ctx2d.clear_rect(0.0, 0.0, 0.0, 0.0);
            }
        }
        if let Ok(mut guard) = bound_cleanup.lock() {
            guard.remove(&page_cleanup);
        }
    });

    // Render on mount and after a heal sweep. A prefetch may populate the
    // cache after `starts_cached` was sampled; every successful engine reply
    // therefore reveals the cover, cached or fresh.
    let cid_render = cid.clone();
    let doc_gen = generation.clone();
    let bound_render = bound.clone();
    let try_render = {
        let render = render.clone();
        let attempts = attempts.clone();
        move || {
            let slot = ThumbRenderState::from_u8(render.load(Ordering::Relaxed));
            if !slot.may_start(attempts.load(Ordering::Relaxed)) {
                return;
            }
            attempts.fetch_add(1, Ordering::Relaxed);
            render.store(slot.start().as_u8(), Ordering::Relaxed);
            let gen_now = doc_gen.load(Ordering::Relaxed);
            let cid2 = cid_render.clone();
            let gen_async = doc_gen.clone();
            let bound_async = bound_render.clone();
            let render_async = render.clone();
            spawn_local(async move {
                if let Ok(mut guard) = bound_async.lock() {
                    guard.insert(page);
                }
                let result = engine::render_thumb(&cid2, page, THUMB_SCALE).await;
                // The cell may have unmounted (or the document changed) while
                // the render was in flight: `Unmounted` is terminal, and the
                // generation double-guard keeps a stale paint out of a fresh
                // document's canvas.
                let slot_now = ThumbRenderState::from_u8(render_async.load(Ordering::Relaxed));
                if slot_now == ThumbRenderState::Unmounted
                    || gen_async.load(Ordering::Relaxed) != gen_now
                {
                    return;
                }
                match result {
                    Ok(_) => {
                        // A concurrent prefetch can turn this request into a
                        // cache hit after mount. The engine has painted either
                        // way, so cached must not leave the cover opaque.
                        render_async.store(slot_now.paint().as_u8(), Ordering::Relaxed);
                        if !loaded.get_untracked() {
                            loaded.set(true);
                            if let Some(el) = cover_ref.get() {
                                _ = el.class_list().add_1("thumb-skeleton-settling");
                            }
                            let render_pulse = render_async.clone();
                            if let Ok(h) = set_timeout_with_handle(
                                move || {
                                    // The scope cleanup clears the handle, but
                                    // the guard keeps even a leaked timer from
                                    // touching a detached cover.
                                    if render_pulse.load(Ordering::Relaxed)
                                        == ThumbRenderState::Unmounted.as_u8()
                                    {
                                        return;
                                    }
                                    if let Some(el) = cover_ref.get() {
                                        let _ = el.class_list().remove_1("thumb-skeleton-loading");
                                        let _ = el.class_list().remove_1("thumb-skeleton-settling");
                                    }
                                },
                                Duration::from_millis(PULSE_STOP_MS),
                            ) {
                                if let Some(prev) = pulse_timer.get_value() {
                                    prev.clear();
                                }
                                pulse_timer.set_value(Some(h));
                            }
                        }
                    }
                    Err(e) => {
                        // A cache probe can have seeded `loaded` before a stale
                        // cancellation prevents the actual canvas blit. Put the
                        // cover back in that case; the next sweep retries it.
                        if loaded.get_untracked() {
                            loaded.set(false);
                        }
                        render_async.store(slot_now.fail().as_u8(), Ordering::Relaxed);
                        // A stale cancellation against a recycled canvas id is
                        // retried by the next heal sweep. Keep genuine errors
                        // visible without turning cancellation into noise.
                        if e.name != "cancelled" {
                            web_sys::console::warn_1(
                                &format!("[thumbnails] render page {page}: {e}").into(),
                            );
                        }
                        // Tell the panel a cell is stale so it schedules a
                        // sweep — nothing else should trigger one.
                        needs_heal.set(true);
                    }
                }
            });
        }
    };
    Effect::new(move |_| {
        _ = heal.get();
        try_render();
    });

    view! {
        <button
            type="button"
            class="group flex w-full cursor-pointer flex-col items-center"
            // Jumping to a page does NOT close the panel: browsing thumbnails
            // is a navigation loop (jump, look, jump again), and closing the
            // sidebar on every click forces the reader to reopen it each time.
            on:click=move |_| {
                state.viewer.page.set(page);
            }
        >
            // The card holds ONE permanent themed backdrop (`thumb-card`) under
            // an always-opaque, always-blended `.thumb-canvas`. The crossfade
            // is inverted: fading the canvas itself in would interpolate
            // between the raw un-blended canvas and the multiply result — the
            // sepia/green "neon flash" settling to muted. Instead a plain
            // themed `.thumb-skeleton` cover sits ABOVE the canvas and fades
            // OUT once the render resolves, so the crossfade interpolates
            // between two same-family themed colors. The cover pulses while
            // the render is in flight; the canvas is transparent until the
            // engine paints it (so the cover shows through), and the first
            // painted frame is already fully filtered + multiply-blended.
            <div
                class="thumb-card relative w-[120px] rounded-md"
                class=("ring-2", is_current)
                class=("ring-accent", is_current)
                class=("ring-1", move || !is_current())
                class=("ring-line", move || !is_current())
                style:height=move || format!("{}px", cell_h())
            >
                <canvas
                    id=cid
                    class="thumb-canvas absolute inset-0 block h-full w-full"
                    class=("thumb-canvas-blank", move || !loaded.get())
                />
                // The fade-out cover: plain themed tint (no filter, no blend),
                // mounted after the canvas so it stacks above it. It pulses
                // (background-tint, see .thumb-skeleton-loading) — a STATIC
                // class, NOT gated on `loaded` — and fully covers the card
                // while the render is in flight, then fades to transparent
                // once `loaded` flips — interpolating between two same-family
                // themed colors, never between the raw and the blended
                // canvas. The pulse is deliberately NOT dropped at resolve:
                // removing the class on the same frame the fade starts would
                // CANCEL the running `background-color` animation mid-flight,
                // and a cancelled CSS animation snaps its property back to
                // the base value in one frame — the cover jumps from its
                // mid-pulse tint to base `--thumb-bg` just as the fade
                // begins (a residual snap). Left alive, the pulse continues
                // smoothly under the fade and is simply invisible at opacity
                // 0.
                //
                // The pulse 50% keyframe DARKENS (color-mix toward black),
                // never brightens. A brightening pulse was the root cause of
                // the sepia/green scroll flicker: in those themes --color-line
                // is darker than --color-surface, so the old "lighter line-mix"
                // 50% keyframe produced a tint LIGHTER than the base --thumb-bg,
                // and during the 300ms opacity fade-out that lighter tint was
                // partially visible over the multiply-blended canvas (which is
                // DARKER — multiply darkens a white page toward the
                // backdrop tint), spiking the visible color brighter mid-fade
                // before settling to the canvas result — the "high brightness
                // then fall back to normal" flicker the user observed on
                // scroll. Darker pulse keeps both ends of the crossfade on
                // the dark side of base, so no theme peaks bright during the
                // fade. Two-phase stop: the pulse stays live THROUGH the fade
                // so the background is continuous at resolve, and
                // `PULSE_STOP_MS` (~400ms) later — once the opacity
                // transition has run its full duration — the cover is fully
                // invisible, so the timer drops the class to halt the
                // now-invisible infinite animation (a deterministic timer
                // instead of `transitionend`, which WebKit <13.1 never fires,
                // bubbles from descendant transitions, and throttled renderers
                // can drop). If the render never resolves, `loaded` stays
                // false — no fade, no timer, no removal — and the pulsing
                // skeleton persists as the intended fallback. aria-hidden: the
                // page number is announced by the permanent `.thumb-num`
                // badge, so the cover must not carry a second copy.
                // A cell that mounts with a CACHED bitmap gets neither the
                // pulse animation nor the opacity transition: it is already
                // painted, so there is nothing to cover and nothing to fade.
                // Attaching either would make a re-entering row blip — the
                // subtle scroll flicker. Only a genuinely new render mounts
                // the animated cover.
                <div
                    node_ref=cover_ref
                    class="thumb-skeleton absolute inset-0 flex items-center justify-center pointer-events-none"
                    aria-hidden="true"
                    class=("thumb-skeleton-loading", move || !starts_cached)
                    class=("transition-opacity", move || !starts_cached)
                    class=("duration-300", move || !starts_cached)
                    class=("opacity-100", move || !loaded.get())
                    class=("opacity-0", move || loaded.get())
                />
                // Permanent page badge. The skeleton used to own the only
                // number and faded to opacity-0 once loaded (and hid it with
                // `invisible` on the current page), so every card except the
                // active one lost its label. This overlay stays at z-10 on
                // every cell after the cover is gone.
                <div
                    class="thumb-num pointer-events-none absolute bottom-1.5 inset-x-0 z-10 flex justify-center"
                    class=("is-current", is_current)
                >
                    <span
                        class="rounded px-1.5 py-0.5 text-[11px] font-semibold tabular-nums shadow-sm transition-colors"
                        class=("bg-accent", is_current)
                        class=("text-white", is_current)
                        class=("bg-surface/90", move || !is_current())
                        class=("text-ink", move || !is_current())
                        class=("border", move || !is_current())
                        class=("border-line/60", move || !is_current())
                    >
                        {page}
                    </span>
                </div>
            </div>
        </button>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_pending_cells_with_attempts_left_may_start() {
        assert!(ThumbRenderState::Pending.may_start(0));
        assert!(ThumbRenderState::Pending.may_start(2));
        assert!(!ThumbRenderState::Pending.may_start(MAX_RENDER_ATTEMPTS));
        // A render already in flight blocks a second one...
        assert!(!ThumbRenderState::Rendering.may_start(0));
        // ...and a painted or unmounted cell never renders again.
        assert!(!ThumbRenderState::Settled.may_start(0));
        assert!(!ThumbRenderState::Unmounted.may_start(0));
    }

    #[test]
    fn a_paint_is_final_a_failure_retries_and_unmount_is_terminal() {
        assert_eq!(ThumbRenderState::Pending.start(), ThumbRenderState::Rendering);
        assert_eq!(ThumbRenderState::Rendering.paint(), ThumbRenderState::Settled);
        assert_eq!(ThumbRenderState::Rendering.fail(), ThumbRenderState::Pending);
        // Unmounting wins over every state and is terminal.
        assert_eq!(ThumbRenderState::Pending.unmount(), ThumbRenderState::Unmounted);
        assert_eq!(ThumbRenderState::Rendering.unmount(), ThumbRenderState::Unmounted);
        assert_eq!(ThumbRenderState::Unmounted.unmount(), ThumbRenderState::Unmounted);
    }

    #[test]
    fn the_slot_round_trips_through_its_atomic_encoding() {
        for slot in [
            ThumbRenderState::Pending,
            ThumbRenderState::Rendering,
            ThumbRenderState::Settled,
            ThumbRenderState::Unmounted,
        ] {
            assert_eq!(ThumbRenderState::from_u8(slot.as_u8()), slot);
        }
    }
}
