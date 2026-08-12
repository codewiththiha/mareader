//! A single thumbnail cell.
//!
//! Split out of `thumbnails_panel.rs`: the cell owns its own render lifecycle
//! (engine registration, the cached-blit fast path, the skeleton crossfade and
//! cancellation on unmount) and is the only place that talks to the engine's
//! thumbnail lane. The panel above it only decides WHICH cells exist.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use leptos::html;
use wasm_bindgen::JsCast;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::engine;
use crate::core::state::{AppState, SidebarMode};

use super::geometry::{CELL_W, PULSE_STOP_MS, THUMB_SCALE};

/// One thumbnail cell: a fully-opaque, fully-blended `.thumb-canvas` under a
/// themed `.thumb-skeleton` cover that fades out once the engine render
/// resolves. Registers its canvas on mount and unregisters it on `on_cleanup`
/// (which fires when the cell scrolls out of the window, the document changes,
/// or the app tears down).
#[component]
pub fn ThumbCell(
    state: AppState,
    /// 1-based page number this cell renders.
    page: u32,
    /// Document generation guard, bumped on document change so a stale
    /// in-flight render can't paint into a fresh document's canvas.
    generation: Arc<AtomicU32>,
    /// Registry of pages whose canvases are currently engine-bound, kept so a
    /// cell can remove itself from it on unmount.
    bound: Arc<Mutex<Vec<u32>>>,
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
    // Whether this cell is the reader's current page (drives the accent ring
    // and the number-band highlight). Reads `viewer.page` so every cell
    // re-evaluates as the reader turns pages in the main viewer. The number
    // band is painted UNDER the absolute `.thumb-canvas` once a thumbnail
    // resolves, so the current page's accent number lifts above it (relative +
    // z-index on the band, plus a translucent pill behind the digit — see the
    // view below) to stay visible over the loaded page.
    let is_current = move || state.viewer.page.get() == page;
    let cid = format!("thumb-{page}");

    // Page-1 aspect drives the fixed cell geometry; falls back to a 3:4
    // portrait default if page1_size isn't populated yet.
    let aspect = move || {
        state
            .doc
            .page1_size
            .get()
            .map(|s| if s.width > 0.0 { s.height / s.width } else { 0.75 })
            .unwrap_or(0.75)
    };
    // Fixed card height for this cell's lifetime. Cells remount on document
    // change (doc_key re-key), so reading aspect once here can't go stale.
    let cell_h = CELL_W * aspect();

    // Release the engine binding when this cell unmounts (scrolled out of the
    // window, document switch, or app teardown). `cancel_thumb` aborts an
    // in-flight render but deliberately KEEPS the cached bitmap, so scrolling
    // this row back into view repaints it instantly instead of re-rendering.
    let cid_cleanup = cid.clone();
    let page_cleanup = page;
    let bound_cleanup = bound.clone();
    on_cleanup(move || {
        engine::cancel_thumb(&cid_cleanup);
        if let Ok(mut guard) = bound_cleanup.lock() {
            guard.retain(|&p| p != page_cleanup);
        }
        // Cancel a pending pulse-removal so it can't fire on a detached node.
        if let Some(h) = pulse_timer.get_value() {
            h.clear();
        }
    });

    // Render on mount through the engine's CACHED thumbnail lane.
    //
    // The cache is what kills the residual scroll flicker. Previously every
    // remount (a row re-entering the virtualization window) restarted a full
    // pdf.js render: the cell mounted an opaque skeleton, pulsed, then
    // crossfaded to the painted canvas — visible as a brightness blip on the
    // rows re-entering view. Now:
    //   * `has_thumb` is probed SYNCHRONOUSLY while the view is built, so a
    //     cached cell mounts with `loaded = true`: no opaque cover, no pulse,
    //     no crossfade — there is nothing to flicker.
    //   * `render_thumb` blits the cached bitmap before it ever suspends, so
    //     the canvas is painted in the same task the cell mounts in.
    // Only genuinely NEW pages take the slow path and show the skeleton once.
    let cid_render = cid.clone();
    let gen = generation.clone();
    let bound_render = bound.clone();
    Effect::new(move || {
        let gen_now = gen.load(Ordering::Relaxed);
        let cid2 = cid_render.clone();
        let gen_async = gen.clone();
        let bound_async = bound_render.clone();
        spawn_local(async move {
            if let Ok(mut guard) = bound_async.lock() {
                if !guard.contains(&page) {
                    guard.push(page);
                }
            }
            match engine::render_thumb(&cid2, page, THUMB_SCALE).await {
                Ok(r) => {
                    // A newer document superseded this render.
                    if gen_async.load(Ordering::Relaxed) != gen_now {
                        return;
                    }
                    // Settle the number band to the real card height instead
                    // of deleting its fixed height (which collapsed it for a
                    // frame and made the backdrop color snap more obvious).
                    if let Some(canvas_el) = web_sys::window()
                        .and_then(|w| w.document())
                        .and_then(|d| d.get_element_by_id(&cid2))
                    {
                        if let Some(card) = canvas_el.parent_element() {
                            if let Ok(Some(num)) = card.query_selector(".thumb-num") {
                                if let Some(num_el) = num.dyn_ref::<web_sys::HtmlElement>()
                                {
                                    let _ = num_el
                                        .style()
                                        .set_property("height", &format!("{}px", cell_h));
                                }
                            }
                        }
                    }
                    // Already-painted (cache hit): `loaded` was seeded true at
                    // build time, so this is a no-op write and NO transition
                    // runs. Only a fresh render actually flips it.
                    if r.cached {
                        return;
                    }
                    loaded.set(true);
                    // FREEZE the pulse for the duration of the fade. The pulse
                    // swings a long way (measured in sepia: 54 luminance units
                    // over its 1.6s cycle) and a render can resolve at ANY
                    // phase of it, so without this each cell begins its reveal
                    // from a different brightness AND keeps oscillating while
                    // it fades. Row-mates resolve a few ms apart, so they
                    // shimmer against each other — the residual flicker on
                    // freshly rendered thumbnails during virtual scrolling.
                    //
                    // Pausing (not removing) is what keeps this safe: the
                    // computed background stays exactly where the animation
                    // left it, so nothing snaps. The class is only REMOVED
                    // below, once the cover is fully transparent.
                    if let Some(el) = cover_ref.get() {
                        let _ = el.class_list().add_1("thumb-skeleton-settling");
                    }
                    // Deterministic two-phase stop: the pulse stays live (now
                    // paused) through the fade — resolve never cancels a
                    // running animation — and ~PULSE_STOP_MS later, once the
                    // fade has run its full duration, the cover is fully
                    // transparent, so removing the classes snaps only an
                    // invisible background. A timer (not `transitionend`) so
                    // the removal can't be lost to event-delivery edge cases
                    // (WebKit <13.1 fires only `webkitTransitionEnd`, a
                    // bubbled descendant opacity transition could remove the
                    // class mid-fade, and a throttled renderer may never
                    // dispatch the event). The handle is parked so on_cleanup
                    // cancels it if the cell unmounts mid-fade. If the render
                    // errors, `loaded` stays false and the pulsing skeleton
                    // persists as the intended fallback.
                    if let Some(h) = set_timeout_with_handle(
                        move || {
                            if let Some(el) = cover_ref.get() {
                                let _ =
                                    el.class_list().remove_1("thumb-skeleton-loading");
                                let _ =
                                    el.class_list().remove_1("thumb-skeleton-settling");
                            }
                        },
                        Duration::from_millis(PULSE_STOP_MS),
                    )
                    .ok()
                    {
                        if let Some(prev) = pulse_timer.get_value() {
                            prev.clear();
                        }
                        pulse_timer.set_value(Some(h));
                    }
                }
                Err(e) => {
                    // Cancellations are the normal eviction path (unmount
                    // aborts an in-flight render while scrolling); the skeleton
                    // is the intended fallback, so only genuine failures log.
                    if e.name != "cancelled" {
                        web_sys::console::log_1(
                            &format!("[thumbnails] render page {page}: {e}").into(),
                        );
                    }
                }
            }
        });
    });

    view! {
        <button
            type="button"
            class="flex w-full cursor-pointer flex-col items-center"
            on:click=move |_| {
                state.viewer.page.set(page);
                state.sidebar.set(SidebarMode::None);
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
                // Current page gets an accent ring; other pages the quiet line
                // ring. Single-token conditional classes only (see the sidebar.rs
                // classList gotcha — a space-separated token would throw).
                class=("ring-2", is_current)
                class=("ring-accent", is_current)
                class=("ring-1", move || !is_current())
                class=("ring-line", move || !is_current())
            >
                <div
                    class="thumb-num flex w-full items-center justify-center"
                    // The current page's number must read OVER the loaded
                    // thumbnail (the absolute `.thumb-canvas` paints above an
                    // in-flow band), so the band stacks on top of it.
                    class=("relative", is_current)
                    class=("z-10", is_current)
                    style:height=format!("{}px", cell_h)
                >
                    <span
                        class="text-sm font-bold"
                        class=("text-accent", is_current)
                        class=("text-muted", move || !is_current())
                        // Translucent surface pill behind the accent digit so
                        // it stays legible over the rendered page.
                        class=("bg-surface/70", is_current)
                        class=("rounded-full", is_current)
                        class=("px-1.5", is_current)
                    >{page}</span>
                </div>
                // `thumb-canvas-blank` until this cell has a bitmap. A canvas
                // with no width/height attributes defaults to 300x150 — a 2:1
                // box stretched over a ~3:4 card — and `.thumb-canvas` carries
                // `mix-blend-mode` + the theme filter, so that empty, wrong
                // aspect surface was still a compositing layer underneath the
                // fading cover. In the multiply themes (sepia/green especially)
                // it tinted the cell for the first frames of the reveal and
                // then changed shape when the real 153x198 bitmap arrived,
                // which is the residual flicker on newly rendered thumbs. Same
                // invariant as the page canvas: never blend a layer that has no
                // real pixels in it yet.
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
                // page number is already announced by the in-flow `.thumb-num`
                // band (which also reads through the transparent canvas
                // pre-resolve), so the cover's copy must not double-announce;
                // for the current page the cover's number is also hidden
                // visually so it can't ghost behind the z-10 accent pill.
                // A cell that mounts with a CACHED bitmap gets neither the
                // pulse animation nor the opacity transition: it is already
                // painted, so there is nothing to cover and nothing to fade.
                // Attaching either would make a re-entering row blip — the
                // subtle scroll flicker. Only a genuinely new render mounts
                // the animated cover.
                <div
                    node_ref=cover_ref
                    class="thumb-skeleton absolute inset-0 flex items-center justify-center"
                    aria-hidden="true"
                    class=("thumb-skeleton-loading", move || !starts_cached)
                    class=("transition-opacity", move || !starts_cached)
                    class=("duration-300", move || !starts_cached)
                    class=("opacity-100", move || !loaded.get())
                    class=("opacity-0", move || loaded.get())
                >
                    <span
                        class="text-sm font-bold text-muted"
                        class=("invisible", is_current)
                    >{page}</span>
                </div>
            </div>
        </button>
    }
}
