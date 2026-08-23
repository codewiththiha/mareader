//! A single thumbnail cell.
//!
//! Split out of `thumbnails_panel.rs`: the cell owns its own render lifecycle
//! (engine registration, the cached-blit fast path, the skeleton crossfade and
//! cancellation on unmount) and is the only place that talks to the engine's
//! thumbnail lane. The panel above it only decides WHICH cells exist.

// The registry + generation guard are shared with `on_cleanup` callbacks,
// which Leptos stores in a `Send + Sync` slot - so these stay `Arc` +
// `Mutex`/`AtomicU32`. This is single-threaded UI code, but the owner's
// cleanup contract demands thread-safe handles; `Rc` would not compile here.
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;

use pdf_engine::api as engine;
use crate::state::ReaderState;

use super::geometry::{CELL_W, PULSE_STOP_MS, THUMB_SCALE};

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
    bound: Arc<Mutex<Vec<u32>>>,
    /// Bumped after the panel settles so a cell whose render lost a cache or
    /// cancellation race can retry without remounting.
    #[prop(into)]
    heal: Signal<u64>,
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
    // Two-phase skeleton-stop state: a NodeRef onto the cover (the timer
    // removes the pulse class from the real DOM node) and the timer handle,
    // parked in a StoredValue so on_cleanup can cancel a pending removal if
    // this cell unmounts mid-fade.
    let cover_ref: NodeRef<html::Div> = NodeRef::new();
    let pulse_timer = StoredValue::new_local(None::<TimeoutHandle>);
    // Async work can outlive a virtualized cell. Keep a local mount guard and
    // separate "engine painted" from `loaded`: cached cells start visually
    // loaded but still need one render call to blit their bitmap.
    let mounted = Arc::new(AtomicBool::new(true));
    let in_flight = Arc::new(AtomicBool::new(false));
    let settled = Arc::new(AtomicBool::new(false));
    let attempts = Arc::new(AtomicU8::new(0));
    // Current page drives the accent ring + badge. The badge is a z-10 overlay
    // so it stays visible on every card, not just the active one.
    let is_current = move || state.viewer.page.get() == page;
    let cid = format!("thumb-{page}");

    // Page-1 aspect drives the fixed cell geometry; falls back to a 3:4
    // portrait default if page1_size isn't populated yet.
    let aspect = move || {
        state
            .document
            .page1_size
            .get()
            .map(|s| if s.width > 0.0 { s.height / s.width } else { 0.75 })
            .unwrap_or(0.75)
    };
    let cell_h = move || CELL_W * aspect();

    // Release the engine binding when this cell unmounts (scrolled out of the
    // window, document switch, or app teardown). `cancel_thumb` aborts an
    // in-flight render but deliberately KEEPS the cached bitmap, so scrolling
    // this row back into view repaints it instantly instead of re-rendering.
    let cid_cleanup = cid.clone();
    let page_cleanup = page;
    let bound_cleanup = bound.clone();
    let mounted_cleanup = mounted.clone();
    on_cleanup(move || {
        mounted_cleanup.store(false, Ordering::Relaxed);
        engine::cancel_thumb(&cid_cleanup);
        if let Ok(mut guard) = bound_cleanup.lock() {
            guard.retain(|&p| p != page_cleanup);
        }
        // Cancel a pending pulse-removal so it can't fire on a detached node.
        if let Some(h) = pulse_timer.get_value() {
            h.clear();
        }
    });

    // Render on mount and after a settle sweep. A prefetch may populate the
    // cache after `starts_cached` was sampled; every successful engine reply
    // therefore reveals the cover, cached or fresh.
    let cid_render = cid.clone();
    let doc_gen = generation.clone();
    let bound_render = bound.clone();
    let try_render = {
        let mounted = mounted.clone();
        let in_flight = in_flight.clone();
        let settled = settled.clone();
        let attempts = attempts.clone();
        move || {
            if settled.load(Ordering::Relaxed)
                || in_flight.load(Ordering::Relaxed)
                || attempts.load(Ordering::Relaxed) >= 3
                || !mounted.load(Ordering::Relaxed)
            {
                return;
            }
            attempts.fetch_add(1, Ordering::Relaxed);
            in_flight.store(true, Ordering::Relaxed);
            let gen_now = doc_gen.load(Ordering::Relaxed);
            let cid2 = cid_render.clone();
            let gen_async = doc_gen.clone();
            let bound_async = bound_render.clone();
            let mounted_async = mounted.clone();
            let in_flight_async = in_flight.clone();
            let settled_async = settled.clone();
            spawn_local(async move {
                if let Ok(mut guard) = bound_async.lock()
                    && !guard.contains(&page)
                {
                    guard.push(page);
                }
                let result = engine::render_thumb(&cid2, page, THUMB_SCALE).await;
                in_flight_async.store(false, Ordering::Relaxed);
                if !mounted_async.load(Ordering::Relaxed)
                    || gen_async.load(Ordering::Relaxed) != gen_now
                {
                    return;
                }
                match result {
                    Ok(_) => {
                        // A concurrent prefetch can turn this request into a
                        // cache hit after mount. The engine has painted either
                        // way, so cached must not leave the cover opaque.
                        settled_async.store(true, Ordering::Relaxed);
                        if !loaded.get_untracked() {
                            loaded.set(true);
                            if let Some(el) = cover_ref.get() {
                                _ = el.class_list().add_1("thumb-skeleton-settling");
                            }
                            if let Ok(h) = set_timeout_with_handle(
                                move || {
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
                        // A stale cancellation against a recycled canvas id is
                        // retried by the next heal sweep. Keep genuine errors
                        // visible without turning cancellation into noise.
                        if e.name != "cancelled" {
                            web_sys::console::warn_1(
                                &format!("[thumbnails] render page {page}: {e}").into(),
                            );
                        }
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
                // partially visible over the multiply-blended canvas (which
                // is DARKER — multiply darkens a white page toward the
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
