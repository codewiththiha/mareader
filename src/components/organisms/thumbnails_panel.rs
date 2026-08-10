//! Thumbnail grid. OWNED BY branch D (panels/settings).
//!
//! Renders a low-scale canvas for every page (no text layer) by talking to the
//! engine directly — `PageCanvas` always builds a text layer, thumbnails must
//! not. Clicking a thumbnail jumps to that page and closes the panel. The panel
//! self-cleans: every canvas unregisters when its cell unmounts, which keeps
//! WKWebView memory in check.
//!
//! Rendering is scroll-windowed: an in-flow spacer spans the full grid height
//! (so the scrollbar covers every page) while only the rows overlapping the
//! visible viewport — plus `ROW_BUFFER` on each side — are mounted as a keyed
//! `<For>`, each row absolutely positioned at its fixed offset. This mirrors the
//! proven `page_list.rs` pattern and means a 200-page document mounts only a
//! handful of canvases instead of 200.
//!
//! Each cell is its own `ThumbCell`, which registers + renders its canvas on
//! mount and unregisters it in `on_cleanup`; `<For>` evicts out-of-window cells
//! automatically, so canvases are created lazily and released as you scroll.
//! While `render_page` is in flight the cell shows a gray skeleton with the
//! page number (`animate-pulse`); the canvas fades in over it once resolved.
//!
//! A generation counter (`Arc<AtomicU32>`, bumped on document change and on
//! panel hide) aborts in-flight renders from a previous document so they can
//! never paint into a fresh document's canvases.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

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
const ROW_BUFFER: usize = 1;
/// CSS-px padding on the scroll container (`p-3`). Rows are inset by this and
/// positioned from the content box, so the virtualization math stays exact.
const PAD: f64 = 12.0;

/// One thumbnail cell: a skeleton box with the page number, the `.thumb-canvas`
/// fading in over it when the engine render resolves. Registers its canvas on
/// mount and unregisters it on `on_cleanup` (which fires when the cell scrolls
/// out of the window, the document changes, or the panel unmounts).
#[component]
fn ThumbCell(
    state: AppState,
    /// 1-based page number this cell renders.
    page: u32,
    /// Document generation guard, bumped on document change / panel hide so a
    /// stale in-flight render can't paint into a fresh document's canvas.
    generation: Arc<AtomicU32>,
    /// Registry of pages whose canvases are currently engine-bound, used by the
    /// panel's defensive unregister when the sidebar leaves Thumbs while the
    /// panel is still mounted.
    bound: Arc<Mutex<Vec<u32>>>,
) -> impl IntoView {
    let loaded = RwSignal::new(false);
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
    // window, document switch, or panel hide).
    let cid_cleanup = cid.clone();
    let page_cleanup = page;
    let bound_cleanup = bound.clone();
    on_cleanup(move || {
        engine::unregister_page(&cid_cleanup);
        if let Ok(mut guard) = bound_cleanup.lock() {
            guard.retain(|&p| p != page_cleanup);
        }
    });

    // Register + render on mount. `render_page` awaits engine work; by the time
    // it resolves the document may have been replaced, so the generation is
    // re-checked before painting.
    let cid_render = cid.clone();
    let gen = generation.clone();
    let bound_render = bound.clone();
    Effect::new(move || {
        let gen_now = gen.load(Ordering::Relaxed);
        let cid2 = cid_render.clone();
        let gen_async = gen.clone();
        let bound_async = bound_render.clone();
        spawn_local(async move {
            engine::register_page(page, &cid2, None);
            if let Ok(mut guard) = bound_async.lock() {
                if !guard.contains(&page) {
                    guard.push(page);
                }
            }
            match engine::render_page(&cid2, THUMB_SCALE, false).await {
                Ok(r) => {
                    // A newer document or a panel hide superseded this render.
                    if gen_async.load(Ordering::Relaxed) != gen_now {
                        return;
                    }
                    if let Some(canvas_el) = web_sys::window()
                        .and_then(|w| w.document())
                        .and_then(|d| d.get_element_by_id(&cid2))
                    {
                        // Set individual properties; NEVER replace the whole
                        // style attribute (a full replace would nuke any inline
                        // custom property the theme/CSS relies on — the same
                        // trap PageCanvas documents for --scale-factor).
                        if let Some(html_el) = canvas_el.dyn_ref::<web_sys::HtmlElement>()
                        {
                            let style = html_el.style();
                            let _ = style.set_property("width", &format!("{}px", r.width));
                            let _ = style.set_property("max-width", "100%");
                            let _ = style.set_property("height", &format!("{}px", cell_h));
                        }
                        // Settle the number band to the real card height instead
                        // of deleting its fixed height (which collapsed it for a
                        // frame and made the backdrop color snap more obvious).
                        if let Some(card) = canvas_el.parent_element() {
                            if let Ok(Some(num)) = card.query_selector(".thumb-num") {
                                if let Some(num_el) = num.dyn_ref::<web_sys::HtmlElement>()
                                {
                                    let _ =
                                        num_el.style().set_property("height", &format!("{}px", cell_h));
                                }
                            }
                        }
                    }
                    loaded.set(true);
                }
                Err(e) => {
                    // Cancellations are the normal eviction path (unregister
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
            // Skeleton: gray placeholder with the page number while the render
            // is in flight. ONE permanent themed backdrop class (`thumb-card`)
            // serves both states — the old code swapped `bg-line/60` (themed
            // skeleton tint) for `bg-surface` (neutral) at resolve, so the
            // backdrop cross-faded to neutral the instant the canvas faded in.
            // The skeleton only pulses on top of the same tint; `.thumb-canvas`
            // mix-blends against it in every state.
            <div
                class="thumb-card relative w-[120px] rounded-md ring-1 ring-line"
                class=("animate-pulse", move || !loaded.get())
            >
                <div
                    class="thumb-num flex w-full items-center justify-center"
                    style:height=format!("{}px", cell_h)
                >
                    <span class="text-xs text-muted">{page}</span>
                </div>
                <canvas
                    id=cid
                    class="thumb-canvas absolute inset-0 block h-full w-full transition-opacity duration-300"
                    class=("opacity-0", move || !loaded.get())
                    class=("opacity-100", move || loaded.get())
                />
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

    // Generation guard: bumped whenever the document identity changes (page
    // count / page-1 size) or the panel is hidden, so in-flight renders from an
    // older document abort. `doc_key` mirrors it reactively to re-key the row
    // `<For>`: a document switch forces every mounted cell to remount
    // (re-register + re-render) — otherwise a new file with the same page count
    // would leave the previous document's canvases painted.
    let generation = Arc::new(AtomicU32::new(0));
    let doc_key = RwSignal::new(0u32);
    // Registry of engine-bound thumbnail canvases, for the defensive unregister.
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

    // Defensive: if the panel is somehow still mounted while the sidebar leaves
    // Thumbs, abort in-flight renders and unregister every canvas still bound.
    // Normally the panel unmounts with the mode and each ThumbCell's on_cleanup
    // unregisters its own canvas; this is belt-and-suspenders.
    let gen_defensive = generation.clone();
    let bound_defensive = bound.clone();
    Effect::new(move || {
        if state.sidebar.get() != SidebarMode::Thumbs {
            gen_defensive.fetch_add(1, Ordering::Relaxed);
            let pages: Vec<u32> = bound_defensive
                .lock()
                .map(|mut guard| guard.drain(..).collect())
                .unwrap_or_default();
            for p in pages {
                engine::unregister_page(&format!("thumb-{p}"));
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
    });

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
