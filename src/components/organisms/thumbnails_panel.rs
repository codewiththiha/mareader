//! Thumbnail grid. OWNED BY branch D (panels/settings).
//!
//! Renders a low-scale canvas for every page (no text layer) by talking to the
//! engine directly — `PageCanvas` always builds a text layer, thumbnails must
//! not. Clicking a thumbnail jumps to that page and closes the panel. The panel
//! self-cleans: every canvas unregisters when its cell unmounts, which keeps
//! WKWebView memory in check. The panel itself stays permanently mounted (the
//! sidebar toggles it with `visibility`, not a remount), so its visible
//! virtualization window of canvases remains engine-bound while the sidebar is
//! collapsed or on Outline — bounded to the window (never the whole document),
//! and released on scroll-out, document change, or app teardown. That is the
//! accepted trade-off for instant thumbnails on every open.
//!
//! Rendering is scroll-windowed: an in-flow spacer spans the full grid height
//! (so the scrollbar covers every page) while only the rows overlapping the
//! visible viewport — plus `ROW_BUFFER` on each side — are mounted as a keyed
//! `<For>`, each row absolutely positioned at its fixed offset. This mirrors the
//! proven `page_list.rs` pattern and means a 200-page document mounts only a
//! handful of canvases instead of 200.
//!
//! Each cell is its own `ThumbCell`, which renders its canvas on mount through
//! the engine's CACHED thumbnail lane (`renderThumb` / `hasThumb` /
//! `cancelThumb`) and cancels any in-flight render in `on_cleanup`; `<For>`
//! evicts out-of-window cells automatically, so renders are started lazily and
//! aborted as you scroll.
//!
//! The engine keeps every rendered thumbnail bitmap in an LRU cache, so a row
//! that scrolls out and back is BLITTED synchronously rather than re-rendered.
//! `ThumbCell` probes that cache (`engine::has_thumb`) while it builds its view
//! and, on a hit, mounts with `loaded = true` and no animation classes at all:
//! no opaque cover, no pulse, no crossfade. That is what removes the last
//! subtle scroll flicker — previously each remounted row replayed the whole
//! skeleton→crossfade sequence over a bitmap that was about to be painted
//! instantly, a faint brightness blip on both columns of the row entering view.
//! A static `thumb-skeleton-loading` (a background-tint pulse) covers the
//! still-empty canvas while `render_page` is in flight and keeps pulsing
//! through the fade-out, so resolve never cancels a running animation; once
//! the fade has run its full duration the cover is invisible and a short
//! timer removes the pulse. The canvas itself stays fully opaque and blended
//! from its first painted frame, so the crossfade interpolates between two
//! same-family themed colors instead of between the raw un-blended canvas and
//! the multiply result — the sepia/green "neon flash" settling to muted. The
//! pulse 50% keyframe DARKENS (color-mix toward black), never brightens: a
//! brightening pulse was the residual sepia/green scroll flicker — in those
//! themes the old lighter 50% tint was partially visible over the darker
//! multiply-blended canvas mid-fade, spiking the visible color brighter
//! before settling. Darker pulse keeps both ends of the crossfade on the
//! dark side of base, so no theme peaks bright during the fade.
//!
//! A generation counter (`Arc<AtomicU32>`, bumped on document change) aborts
//! in-flight renders from a previous document so they can never paint into a
//! fresh document's canvases.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::Event;
use web_sys::ResizeObserverEntry;

use crate::api::engine;
use crate::core::layout::visible_grid_rows;
use crate::core::state::{AppState, SidebarMode};

/// Render scale for thumbnails (CSS px per PDF unit).
const THUMB_SCALE: f64 = 0.25;
/// Fixed CSS-px width of each thumbnail cell. Fits two abreast in the w-72
/// sidebar: 288 - 2*12 padding - 12 gap = 252; 2 * 120 + 12 = 252.
const CELL_W: f64 = 120.0;
/// CSS-px gap between rows (the page-number band lives inside each cell).
const ROW_GAP: f64 = 8.0;
/// Extra rows rendered above/below the visible window (pre-render margin).
///
/// Two, not one. With a single buffer row the row that scrolls into view is the
/// one mounted an instant earlier, so on a fast scroll it is still mid-render
/// when the user first sees it — the row appears, then settles. Two rows of lead
/// time means a genuinely new row has finished (and been cached) before it
/// reaches the viewport edge. It costs nothing on revisits: cached rows blit
/// synchronously.
const ROW_BUFFER: usize = 2;
/// CSS-px padding on the scroll container (`p-3`). Rows are inset by this and
/// positioned from the content box, so the virtualization math stays exact.
const PAD: f64 = 12.0;
/// Debounce for the auto-center glide: the scroll fires once this long after
/// page writes have settled. In continuous mode the reader writes
/// `viewer.page` at every row boundary, so an immediate smooth scroll per
/// write would keep re-starting the in-flight glide and churn the
/// virtualization window; cancel-and-reschedule (debounce) yields exactly one
/// glide, shortly after the reader pauses.
const GLIDE_DEBOUNCE_MS: u64 = 150;
/// User-drive grace window: while the user has interacted with the thumb grid
/// within this many ms, auto-center defers (and re-checks) instead of yanking
/// the panel away from the row they are browsing. A page change that lands
/// inside the grace is NOT dropped — it is centered once the grace lapses.
const GRACE_MS: f64 = 1500.0;
/// Delay (ms) before the skeleton pulse is removed after a thumbnail render
/// resolves. The cover's opacity fade runs `duration-300` (300 ms); this sits
/// just past it so the pulse keeps running through the whole fade and the
/// class is only dropped once the cover is fully transparent — removing it
/// earlier would CANCEL the running `background-color` animation and snap the
/// cover back to base mid-fade (the flicker this code eliminates). Bounded
/// one-shot, not a forever-animation, so idle cells don't pulse indefinitely.
const PULSE_STOP_MS: u64 = 400;

/// One thumbnail cell: a fully-opaque, fully-blended `.thumb-canvas` under a
/// themed `.thumb-skeleton` cover that fades out once the engine render
/// resolves. Registers its canvas on mount and unregisters it on `on_cleanup`
/// (which fires when the cell scrolls out of the window, the document changes,
/// or the app tears down).
#[component]
fn ThumbCell(
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
                    // Deterministic two-phase stop: the pulse stays live
                    // through the fade (resolve never cancels a running
                    // animation), and ~PULSE_STOP_MS later — once the fade
                    // has run its full duration — the cover is fully
                    // transparent, so removing the pulse class snaps only an
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
                <canvas
                    id=cid
                    class="thumb-canvas absolute inset-0 block h-full w-full"
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

#[component]
pub fn ThumbnailsPanel(state: AppState) -> impl IntoView {
    let num_pages = state.doc.num_pages;
    let page1_size = state.doc.page1_size;

    // Reactive geometry: page-1 aspect drives the fixed row height; the row
    // count follows the page count (2 columns per row).
    let aspect = move || {
        page1_size
            .get()
            .map(|s| if s.width > 0.0 { s.height / s.width } else { 0.75 })
            .unwrap_or(0.75)
    };
    let row_height = move || CELL_W * aspect() + ROW_GAP;
    let rows = move || (num_pages.get() as usize).div_ceil(2);

    // Scroll window of the container (populated by the listener/observer below).
    let scroll_top = RwSignal::new(0.0);
    let viewport_h = RwSignal::new(0.0);

    // Last time the USER physically scrolled/dragged the thumb panel. The
    // auto-center effect yields to it for a grace period so the grid never
    // fights someone who is browsing the thumbs themselves. NEG_INFINITY = the
    // user has never driven it (auto-center always allowed).
    let last_user_drive: Rc<Cell<f64>> = Rc::new(Cell::new(f64::NEG_INFINITY));
    // (was-this-panel-open, last-centered page) — auto-center only acts on a
    // panel open or a real page change, never on churn. Kept in a StoredValue so
    // reading/writing it never registers a reactive dependency.
    let centered = StoredValue::new_local((false, 0u32));
    // Handle for the debounced auto-center glide. Parked in a StoredValue so
    // the effect can cancel a pending glide when it re-runs or the panel
    // closes, and a fired glide that finds the user-drive grace still active
    // can re-arm itself.
    let glide_timer = StoredValue::new_local(None::<TimeoutHandle>);
    // The self-re-arming glide step lives in an `Rc` parked HERE (component-
    // scoped, not an effect-run local) so a fired step can upgrade its Weak
    // back-reference and re-arm itself while the user is still driving the
    // grid. The step is replaced on every effect run; on_cleanup cancels the
    // pending timer, so the old step is dropped with its timer.
    let glide_slot = StoredValue::new_local(None::<Rc<RefCell<Option<Rc<dyn Fn()>>>>>);
    // Keeps the user-drive listener Closure alive for the panel's lifetime.
    let drive_slot: StoredValue<Option<Closure<dyn FnMut(Event)>>, _> =
        StoredValue::new_local(None);

    // Generation guard: bumped whenever the document identity changes (page
    // count / page-1 size), so in-flight renders from an older document abort.
    // `doc_key` mirrors it reactively to re-key the row
    // `<For>`: a document switch forces every mounted cell to remount
    // (re-register + re-render) — otherwise a new file with the same page count
    // would leave the previous document's canvases painted.
    //
    // Because the panel is permanently mounted, this effect is now the ONLY
    // path that re-renders thumbnails for a new document — every document-open
    // path must write `num_pages`/`page1_size` (RwSignal::set always notifies,
    // so a write always re-keys the grid).
    let generation = Arc::new(AtomicU32::new(0));
    let doc_key = RwSignal::new(0u32);
    // Registry of engine-bound thumbnail canvases, kept so a cell can remove
    // itself on unmount.
    let bound: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));

    // `started` is set once the effect has seen the document it mounts with;
    // only document CHANGES after that bump the guard (and re-key the `<For>`).
    // Skipping the mount-time bump avoids re-mounting the visible window (and
    // re-rendering every visible thumbnail) the moment the panel opens.
    //
    // On a real document change the container scroll is also reset to the top:
    // a shorter document could otherwise leave the window past the end of the
    // grid (visible_grid_rows returns None -> blank panel until the browser
    // clamps scrollTop and fires a scroll event, which Safari historically
    // does not).
    let doc_seen = StoredValue::new_local(false);
    let gen_doc = generation.clone();
    let container_el = StoredValue::new_local(None::<web_sys::Element>);
    let scroll_top_reset = scroll_top;
    Effect::new(move || {
        let n = num_pages.get();
        let size = page1_size.get();
        if n > 0 && size.is_some() {
            if doc_seen.get_value() {
                gen_doc.fetch_add(1, Ordering::Relaxed);
                doc_key.update(|k| *k += 1);
                if let Some(el) = container_el.get_value() {
                    el.set_scroll_top(0);
                }
                scroll_top_reset.set(0.0);
            } else {
                doc_seen.set_value(true);
            }
        }
    });

    // Visible row window [first, last], expanded by ROW_BUFFER on each side.
    let visible = Memo::new(move |_| {
        // The rows live inside the container's 12px top padding, so the window
        // is computed against the content box (`scroll_top - PAD`, clamped).
        let st = (scroll_top.get() - PAD).max(0.0);
        visible_grid_rows(st, viewport_h.get(), rows(), row_height(), ROW_BUFFER)
    });

    // --- scroll / size tracking ----------------------------------------------
    // Attached once per mount (deferred to a microtask so the container node
    // exists). The scroll listener updates the window on scroll; the
    // ResizeObserver keeps the viewport height fresh on container resizes and
    // reports the initial height. The JS handles are parked in StoredValues so
    // they stay alive for the panel's lifetime, and the scroll listener is
    // detached on cleanup.
    let cleanup_slot: StoredValue<Option<(web_sys::Element, js_sys::Function)>> =
        StoredValue::new(None);
    let listener_slot: StoredValue<Option<Closure<dyn FnMut(Event)>>, _> =
        StoredValue::new_local(None);
    let observer_handle: StoredValue<Option<web_sys::ResizeObserver>, _> =
        StoredValue::new_local(None);
    let callback_handle: StoredValue<Option<Closure<dyn FnMut(Vec<ResizeObserverEntry>)>>, _> =
        StoredValue::new_local(None);

    on_cleanup(move || {
        if let Some((el, cb)) = cleanup_slot.get_value() {
            let _ = el.remove_event_listener_with_callback("scroll", &cb);
        }
        // Disconnect BEFORE the Closure is dropped: the browser keeps its own
        // reference to the wasm-bindgen shim, so a resize notification queued
        // during teardown would invoke a freed closure and abort the runtime
        // ("closure invoked recursively or after being dropped").
        if let Some(observer) = observer_handle.try_get_value().flatten() {
            observer.disconnect();
        }
        observer_handle.try_set_value(None);
        callback_handle.try_set_value(None);
    });

    // The drive tracker lives past the spawn_local (the auto-center effect
    // reads it), so hand the listener a clone instead of moving it in.
    let drive_owner = last_user_drive.clone();
    spawn_local(async move {
        if listener_slot.with_value(|l| l.is_some()) {
            return; // already set up (component body re-run)
        }
        let Some(el) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("thumb-scroll"))
        else {
            return;
        };

        // Seed the window from the real geometry before any scroll/resize, and
        // remember the element so document changes can reset its scrollTop.
        scroll_top.set(el.scroll_top() as f64);
        viewport_h.set(el.client_height() as f64);
        container_el.set_value(Some(el.clone()));

        // Scroll -> scroll_top. (clientHeight stays authoritative for the
        // window height; it is tracked by the ResizeObserver below.)
        let handler = {
            let el = el.clone();
            move |_: Event| {
                scroll_top.set(el.scroll_top() as f64);
            }
        };
        let closure = Closure::new(handler);
        let cb: js_sys::Function = closure.as_ref().unchecked_ref::<js_sys::Function>().clone();
        if el.add_event_listener_with_callback("scroll", &cb).is_ok() {
            cleanup_slot.set_value(Some((el.clone(), cb)));
            listener_slot.set_value(Some(closure));
        }

        // User-drive tracker: any wheel / pointerdown / touchstart on the panel
        // stamps the current time so the auto-center effect stands down for a
        // grace period (the Closure is parked in `drive_slot` to stay alive).
        let drive = {
            let drive_last = drive_owner.clone();
            move |_: Event| {
                drive_last.set(js_sys::Date::now());
            }
        };
        let drive_closure = Closure::new(drive);
        let drive_fn: js_sys::Function =
            drive_closure.as_ref().unchecked_ref::<js_sys::Function>().clone();
        for ev in ["wheel", "pointerdown", "touchstart"] {
            let _ = el.add_event_listener_with_callback(ev, &drive_fn);
        }
        drive_slot.set_value(Some(drive_closure));

        // Container size -> viewport height (fires initially too).
        let callback: Closure<dyn FnMut(Vec<ResizeObserverEntry>)> = Closure::wrap(
            Box::new(move |entries: Vec<ResizeObserverEntry>| {
                if let Some(entry) = entries.first() {
                    if let Some(el) = entry.target().dyn_ref::<web_sys::HtmlElement>() {
                        viewport_h.set(el.client_height() as f64);
                    }
                }
            }) as Box<dyn FnMut(Vec<ResizeObserverEntry>)>,
        );
        let fn_ref: &js_sys::Function = callback.as_ref().unchecked_ref();
        if let Ok(observer) = web_sys::ResizeObserver::new(fn_ref) {
            observer.observe(&el);
            observer_handle.set_value(Some(observer));
            callback_handle.set_value(Some(callback));
        }
    });

    // --- auto-center the current page ---------------------------------------
    // Follows the reader's page (main-viewer scroll in continuous mode, page
    // turns in single mode) by gliding the current page's row to the vertical
    // middle of the panel. Reads every dependency it subscribes to
    // UNCONDITIONALLY at the top (see the page_tracking.rs gotcha), and leaves
    // the "panel open" flag UNSET until geometry is real — otherwise the first
    // mount run (element/viewport not seeded yet) would consume the "just
    // opened" transition and the panel would then never center on open. The geo
    // math needs no DOM measurement: the spacer gives the container its full
    // height, so the scroll lands before the virtualization window mounts the
    // row. The scroll we write fires the existing scroll listener, which drives
    // the virtualization window as usual.
    //
    // The glide is DEBOUNCED and grace-aware. In continuous mode the reader
    // writes `viewer.page` at every row boundary, so an immediate smooth
    // scroll per write would keep re-starting the in-flight glide — the panel
    // would vibrate behind the reader and churn the virtualization window.
    // Every run therefore cancels the previous glide and re-arms it (the same
    // set_timeout_with_handle + on_cleanup pattern as effects/fit.rs), so it
    // fires ONCE, GLIDE_DEBOUNCE_MS after page writes settle. Opening the
    // panel is exempt — it is a one-shot, not churn, so it snaps next tick
    // instead of waiting out the debounce. A page change that lands inside
    // the user-drive grace is NOT dropped: the timer waits out the remaining
    // grace and, if the user is still driving when it fires, re-checks and
    // defers again (re-arming itself) — so the skipped page still ends up
    // centered once the panel (and the reader) have been still for the window.
    Effect::new(move |_| {
        let in_thumbs = state.sidebar.get() == SidebarMode::Thumbs;
        let p = state.viewer.page.get();
        let vh = viewport_h.get();
        let rh = row_height();
        let cell_h = CELL_W * aspect();
        let total_rows = rows();

        let (was_open, _prev_p) = centered.get_value();
        if !in_thumbs {
            centered.set_value((false, 0));
            return;
        }
        // Element/geometry not ready yet (fresh mount): stay "unopened" so the
        // first run with real geometry counts as the panel just opening.
        let Some(el) = container_el.get_value() else {
            return;
        };
        if vh <= 0.0 || rh <= 0.0 || total_rows == 0 {
            return;
        }

        let just_opened = !was_open;
        // Record the page this run intends to center. Kept honest even while a
        // grace/debounce defers the actual scroll, so a page change that lands
        // inside the grace is remembered instead of permanently skipped.
        centered.set_value((true, p));

        // Row containing page p (2 columns per row, 0-based).
        let row = (p.saturating_sub(1) / 2) as f64;
        let cell_center_y = PAD + row * rh + cell_h / 2.0;
        let max_scroll = (PAD * 2.0 + total_rows as f64 * rh - vh).max(0.0);
        let target = (cell_center_y - vh / 2.0).clamp(0.0, max_scroll);

        let cur = el.scroll_top() as f64;
        if (target - cur).abs() <= 1.0 {
            // Already centered: cancel any pending glide (it targets an older
            // geometry/page) and stop.
            if let Some(h) = glide_timer.get_value() {
                h.clear();
                glide_timer.set_value(None);
            }
            return;
        }
        // User is browsing the thumb grid themselves -> don't yank (GRACE_MS).
        // The deferred glide below waits out the grace and re-checks at fire
        // time, so a page turned inside the grace still gets centered after.
        let in_grace = !just_opened && js_sys::Date::now() - last_user_drive.get() < GRACE_MS;

        // Instant (explicitly — NOT Auto, which would defer to the element's
        // CSS scroll-behavior) on panel open or far jumps; smooth for nearby
        // page turns.
        let behavior = if just_opened || (target - cur).abs() > 2.0 * vh {
            web_sys::ScrollBehavior::Instant
        } else {
            web_sys::ScrollBehavior::Smooth
        };

        // Self-re-arming glide step: performs the scroll once the grace has
        // fully lapsed and the panel is still showing thumbs; while the user
        // keeps driving, it defers by re-checking after the grace lapses. The
        // step's back-reference to its own holder is a Weak, and the holder's
        // strong `Rc` lives in the component-scoped `glide_slot` StoredValue —
        // NOT an effect-run local (that would drop when this callback returns,
        // permanently breaking the upgrade). So a fired step can always find
        // itself and re-arm; the deferral survives the effect callback.
        let step_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
        let step_self = Rc::downgrade(&step_slot);
        let step_state = state;
        let step_el = el;
        let step_drive = last_user_drive.clone();
        let step_timer = glide_timer;
        let step_page = p;
        let step: Rc<dyn Fn()> = Rc::new(move || {
            let now = js_sys::Date::now();
            let elapsed = now - step_drive.get();
            let in_thumbs_now = step_state.sidebar.get_untracked() == SidebarMode::Thumbs;
            let page_now = step_state.viewer.page.get_untracked();
            let cur_now = step_el.scroll_top() as f64;
            // The world moved on since the glide was armed (panel closed, the
            // reader turned past this page, or the row is already centered):
            // drop the deferred glide.
            if !in_thumbs_now || page_now != step_page || (target - cur_now).abs() <= 1.0 {
                step_timer.set_value(None);
                return;
            }
            if elapsed < GRACE_MS {
                // User still driving the grid — re-check once the grace lapses.
                let next = step_self.upgrade().and_then(|slot| slot.borrow().clone());
                let h = next
                    .map(|next| {
                        set_timeout_with_handle(
                            move || next(),
                            Duration::from_millis((GRACE_MS - elapsed + 50.0) as u64),
                        )
                        .ok()
                    })
                    .flatten();
                step_timer.set_value(h);
                return;
            }
            let opts = web_sys::ScrollToOptions::new();
            opts.set_top(target);
            opts.set_behavior(behavior);
            step_el.scroll_to_with_scroll_to_options(&opts);
            step_timer.set_value(None);
        });
        *step_slot.borrow_mut() = Some(step.clone());
        glide_slot.set_value(Some(step_slot));

        // Debounce: cancel any pending glide and re-arm. Sustained page writes
        // keep re-running this effect, so each run clears the previous timer
        // and re-arms — exactly one glide, after the writes settle.
        if let Some(h) = glide_timer.get_value() {
            h.clear();
            glide_timer.set_value(None);
        }
        let delay = if just_opened {
            // Panel opening is a one-shot event, not churn: snap next tick
            // (the old synchronous behavior), no debounce lag on open.
            0
        } else if in_grace {
            // Wait out the remaining grace (+ a settle buffer) before gliding.
            (GRACE_MS - (js_sys::Date::now() - last_user_drive.get()) + 60.0) as u64
        } else {
            GLIDE_DEBOUNCE_MS
        };
        let fire = step.clone();
        let h = set_timeout_with_handle(move || fire(), Duration::from_millis(delay)).ok();
        glide_timer.set_value(h);
        on_cleanup(move || {
            if let Some(h) = glide_timer.get_value() {
                h.clear();
                glide_timer.set_value(None);
            }
            glide_slot.set_value(None);
        });
    });

    view! {
        <div id="thumb-scroll" class="relative flex-1 overflow-y-auto p-3">
            // Spacer: makes the scrollbar span the whole grid.
            <div
                aria-hidden="true"
                style:height=move || format!("{}px", rows() as f64 * row_height())
            ></div>
            <For
                each=move || {
                    let k = doc_key.get();
                    visible
                        .get()
                        .map(|(first, last)| (first..=last).map(move |r| (k, r)).collect::<Vec<_>>())
                        .unwrap_or_default()
                }
                key=|(k, r): &(u32, usize)| (*k, *r)
                children=move |(_k, row): (u32, usize)| {
                    let p1 = (row * 2 + 1) as u32;
                    let p2 = (row * 2 + 2) as u32;
                    let n = num_pages.get();
                    // Reactive per-row offset so a row-height change (new doc,
                    // new page-1 aspect) repositions rows even before a re-key.
                    view! {
                        <div
                            class="absolute inset-x-3"
                            style:top=move || format!("{}px", PAD + row as f64 * row_height())
                        >
                            <div class="grid grid-cols-2 gap-3">
                                <ThumbCell
                                    state=state
                                    page=p1
                                    generation=generation.clone()
                                    bound=bound.clone()
                                />
                                {if p2 <= n {
                                    view! {
                                        <ThumbCell
                                            state=state
                                            page=p2
                                            generation=generation.clone()
                                            bound=bound.clone()
                                        />
                                    }.into_any()
                                } else {
                                    ().into_any()
                                }}
                            </div>
                        </div>
                    }
                }
            />
        </div>
    }
}
