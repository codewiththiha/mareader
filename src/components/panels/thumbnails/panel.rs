//! Thumbnail grid: low-scale canvases (no text layer) for every page, rendered
//! through the engine's cached thumbnail lane. Clicking jumps to that page.
//! Scroll-windowed: a spacer spans the full grid height while only the rows
//! overlapping the viewport (plus `ROW_BUFFER`) are mounted, each cell
//! cancelling its in-flight render on unmount. The engine's LRU cache lets a
//! scrolled-out-and-back row be blitted synchronously, so cache-hit cells mount
//! without a skeleton. A generation counter aborts renders from a previous
//! document. The auto-center glide lives in the sibling `auto_center` module.

type FrameHandlerSlot = leptos::prelude::StoredValue<
    Option<wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)>>,
    leptos::prelude::LocalStorage,
>;
type ResizeHandlerSlot = leptos::prelude::StoredValue<
    Option<wasm_bindgen::closure::Closure<dyn FnMut(Vec<web_sys::ResizeObserverEntry>)>>,
    leptos::prelude::LocalStorage,
>;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::Event;
use web_sys::ResizeObserverEntry;

use pdf_core::layout::visible_grid_rows;
use crate::state::ReaderState;
use crate::state::ui::SidebarMode;
use leptos::prelude::RwSignal;

use super::auto_center::AutoCenter;
use super::cell::ThumbCell;
use super::geometry::{row_count, row_height, MIN_VIEWPORT_H, PAD, ROW_BUFFER};

#[component]
pub fn ThumbnailsPanel(
    state: ReaderState,
    /// False once a close slide from Thumbs has finished. The `<For>` then
    /// emits no rows, every cell unmounts, and `cancelThumb` zeros the live
    /// canvases. True while the panel is showing, while Outline is showing
    /// (instant tab switch), and while the 300ms Thumbs outro is in flight
    /// (a quick reopen must not remount).
    #[prop(into)]
    live: Signal<bool>,
    /// Which sidebar panel is open (app chrome state passed in explicitly).
    sidebar: RwSignal<SidebarMode>,
) -> impl IntoView {
    let num_pages = state.document.num_pages;
    let page1_size = state.document.page1_size;

    // Reactive geometry: page-1 aspect drives the fixed row height; the row
    // count follows the page count (2 columns per row).
    let aspect = move || {
        page1_size
            .get()
            .map(|s| if s.width > 0.0 { s.height / s.width } else { 0.75 })
            .unwrap_or(0.75)
    };
    let row_height = move || row_height(aspect());
    let rows = move || row_count(num_pages.get() as usize);

    // Scroll window of the container. `scroll_top` is written ONLY by the
    // `on:scroll` view attribute on the scroller below; `viewport_h` is
    // seeded with the generous floor and tightened by the mount effect +
    // ResizeObserver (both self-correcting). Both are TRACKED reads in the
    // window memo — never get_untracked — so a scroll write always re-runs it.
    let scroll_ref: NodeRef<html::Div> = NodeRef::new();
    let scroll_top = RwSignal::new(0.0);
    let viewport_h = RwSignal::new(MIN_VIEWPORT_H);
    let container_el = StoredValue::new_local(None::<web_sys::Element>);

    // Re-poke visible cells after opening and after scroll settles. This heals
    // a render that lost a prefetch/cache race or a stale cancellation without
    // re-rendering cells that are already loaded or in flight.
    let heal = RwSignal::new(0u64);
    let heal_timer = StoredValue::new_local(None::<TimeoutHandle>);
    Effect::new(move |_| {
        if !live.get() {
            if let Some(h) = heal_timer.get_value() {
                h.clear();
                heal_timer.set_value(None);
            }
            return;
        }
        _ = scroll_top.get();
        if let Some(h) = heal_timer.get_value() {
            h.clear();
        }
        let handle = set_timeout_with_handle(
            move || {
                heal_timer.set_value(None);
                heal.update(|n| *n += 1);
            },
            Duration::from_millis(500),
        )
        .ok();
        heal_timer.set_value(handle);
    });
    on_cleanup(move || {
        if let Some(h) = heal_timer.get_value() {
            h.clear();
        }
        heal_timer.set_value(None);
    });

    // Auto-center machinery (glide / grace / debounce) lives in
    // `super::auto_center`; the bundle below carries the panel-lifetime
    // handles it shares with this component (`container_el` is also written
    // by the mount effect below, `viewport_h` by the size tracker).
    let auto = AutoCenter::new(container_el, viewport_h);
    // Keeps the user-drive listener Closure alive for the panel's lifetime.
    let drive_slot: FrameHandlerSlot =
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

    // The virtualization window. BOTH inputs are tracked reads (scroll_top and
    // viewport_h are RwSignals, never get_untracked) so every scroll write
    // re-runs this memo and the <For> below diffs the row window. `viewport_h`
    // is seeded with MIN_VIEWPORT_H (not 0) so the first compute is already
    // generous; the mount effect + ResizeObserver tighten it to the real
    // panel height.
    let visible = Memo::new(move |_| {
        // The rows live inside the container's 12px top padding, so the window
        // is computed against the content box (`scroll_top - PAD`, clamped).
        let st = (scroll_top.get() - PAD).max(0.0);
        visible_grid_rows(st, viewport_h.get(), rows(), row_height(), ROW_BUFFER)
    });

    // Self-healing measurement: re-seed the window the moment the panel
    // becomes visible. The seed below can land before the routed layout has
    // settled, and the ResizeObserver only fires on size CHANGES — the
    // container's height is constant across the sidebar's open/close slide
    // (only its width is clipped), so a stale zero would never self-correct
    // and the grid would stay stuck at the buffer rows. Re-reading the real
    // client height on every open guarantees a correct window.
    Effect::new(move |_| {
        if !live.get() {
            return;
        }
        let el = container_el.get_value().or_else(|| {
            web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("thumb-scroll"))
        });
        if let Some(el) = el {
            container_el.set_value(Some(el.clone()));
            viewport_h.set(el.client_height() as f64);
            scroll_top.set(el.scroll_top() as f64);
        }
    });

    // --- size tracking ------------------------------------------------------
    // Scroll writes live on the `on:scroll` view attribute (below); this block
    // owns the viewport-height measurement (mount seed + ResizeObserver), the
    // user-drive listeners, and the element handle the auto-center effects
    // read. The JS handles are parked in StoredValues so they stay alive for
    // the panel's lifetime; the observer is disconnected on cleanup.
    let observer_handle: StoredValue<Option<web_sys::ResizeObserver>, _> =
        StoredValue::new_local(None);
    let callback_handle: ResizeHandlerSlot =
        StoredValue::new_local(None);

    on_cleanup(move || {
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

    // The drive tracker is also read by the auto-center effect, so this
    // effect takes its own clone instead of moving the bundle's Rc.
    let drive_owner = auto.last_user_drive.clone();

    // Mount-time wiring, once the scroller node exists (`scroll_ref.get()` is
    // a tracked read, so this effect re-runs the instant the node mounts):
    //   * seed viewport_h / scroll_top from the real geometry (guarded > 0 so
    //     a pre-layout zero can't clobber the MIN_VIEWPORT_H floor),
    //   * remember the element for auto-center + document-change scroll resets,
    //   * attach the user-drive listeners (wheel / pointerdown / touchstart)
    //     that feed the auto-center grace,
    //   * observe the container so a real height lands even if it was 0 at
    //     mount (self-correcting).
    //
    // Scroll writes themselves live on the `on:scroll` view attribute below —
    // declaratively bound to the SAME element that has `overflow-y-auto`, so
    // there is no wrong-element or NodeRef-timing class of bug.
    Effect::new(move |_| {
        let Some(div) = scroll_ref.get() else { return };
        // NodeRef gives the HtmlDivElement; `container_el` holds an Element.
        let el: web_sys::Element = div.clone().unchecked_into();
        container_el.set_value(Some(el.clone()));
        let h = el.client_height() as f64;
        if h > 0.0 {
            viewport_h.set(h);
        }
        scroll_top.set(el.scroll_top() as f64);

        // Drive listeners: attach once.
        if drive_slot.with_value(|d| d.is_some()) {
            return;
        }
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
            _ = el.add_event_listener_with_callback(ev, &drive_fn);
        }
        drive_slot.set_value(Some(drive_closure));

        // Container size -> viewport height (fires initially too).
        let callback: Closure<dyn FnMut(Vec<ResizeObserverEntry>)> = Closure::wrap(
            Box::new(move |entries: Vec<ResizeObserverEntry>| {
                if let Some(entry) = entries.first()
                    && let Some(el) = entry.target().dyn_ref::<web_sys::HtmlElement>()
                {
                    let h = el.client_height() as f64;
                    if h > 0.0 {
                        viewport_h.set(h);
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
    // The glide / grace / debounce machinery (and the "reveal-active" listener)
    // lives in `super::auto_center`; install it with the shared handles.
    auto.install(state, sidebar);

    view! {
        // on:scroll MUST live on the SAME element that has overflow-y-auto —
        // this div. Leptos binds view-attribute listeners at mount, so the
        // write below is the only scroll_top writer (no window/document-level
        // listener: inner-scroller scroll events do not bubble to window).
        <div
            id="thumb-scroll"
            node_ref=scroll_ref
            class="relative flex-1 overflow-y-auto p-3"
            on:scroll=move |ev: web_sys::Event| {
                let target: web_sys::Node = event_target(&ev);
                let Some(el) = target.dyn_ref::<web_sys::HtmlElement>() else { return };
                scroll_top.set(el.scroll_top() as f64);
            }
        >
            // Spacer: makes the scrollbar span the whole grid.
            <div
                aria-hidden="true"
                style:height=move || format!("{}px", rows() as f64 * row_height())
            ></div>
            <For
                each=move || {
                    // Drop every cell once the close slide has finished so
                    // WKWebView releases the live backing stores. The cache
                    // is untouched, so the next open is a sync blit.
                    if !live.get() {
                        return Vec::new();
                    }
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
                                    heal=Signal::derive(move || heal.get())
                                />
                                {if p2 <= n {
                                    view! {
                                        <ThumbCell
                                            state=state
                                            page=p2
                                            generation=generation.clone()
                                            bound=bound.clone()
                                            heal=Signal::derive(move || heal.get())
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
