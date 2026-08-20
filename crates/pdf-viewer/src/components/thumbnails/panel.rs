//! Thumbnail grid: low-scale canvases (no text layer) for every page, rendered
//! through the engine's cached thumbnail lane. Clicking jumps to that page.
//! Scroll-windowed: a spacer spans the full grid height while only the rows
//! overlapping the viewport (plus `ROW_BUFFER`) are mounted, each cell
//! cancelling its in-flight render on unmount. The engine's LRU cache lets a
//! scrolled-out-and-back row be blitted synchronously, so cache-hit cells mount
//! without a skeleton. A generation counter aborts renders from a previous
//! document.

use std::cell::{Cell, RefCell};

type FrameHandlerSlot = leptos::prelude::StoredValue<
    Option<wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)>>,
    leptos::prelude::LocalStorage,
>;
type ResizeHandlerSlot = leptos::prelude::StoredValue<
    Option<wasm_bindgen::closure::Closure<dyn FnMut(Vec<web_sys::ResizeObserverEntry>)>>,
    leptos::prelude::LocalStorage,
>;
type RevealSlot =
    std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>>>;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::Event;
use web_sys::ResizeObserverEntry;

use pdf_core::layout::visible_grid_rows;
use crate::state::{ViewerState, SidebarMode};

use super::cell::ThumbCell;
use super::geometry::{
    row_count, row_height, CELL_W, GLIDE_DEBOUNCE_MS, GRACE_MS, PAD, ROW_BUFFER,
};

#[component]
pub fn ThumbnailsPanel(
    state: ViewerState,
    /// False once a close slide from Thumbs has finished. The `<For>` then
    /// emits no rows, every cell unmounts, and `cancelThumb` zeros the live
    /// canvases. True while the panel is showing, while Outline is showing
    /// (instant tab switch), and while the 300ms Thumbs outro is in flight
    /// (a quick reopen must not remount).
    #[prop(into)]
    live: Signal<bool>,
) -> impl IntoView {
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
    let row_height = move || row_height(aspect());
    let rows = move || row_count(num_pages.get() as usize);

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
    let listener_slot: FrameHandlerSlot =
        StoredValue::new_local(None);
    let observer_handle: StoredValue<Option<web_sys::ResizeObserver>, _> =
        StoredValue::new_local(None);
    let callback_handle: ResizeHandlerSlot =
        StoredValue::new_local(None);

    on_cleanup(move || {
        if let Some((el, cb)) = cleanup_slot.get_value() {
            _ = el.remove_event_listener_with_callback("scroll", &cb);
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
            _ = el.add_event_listener_with_callback(ev, &drive_fn);
        }
        drive_slot.set_value(Some(drive_closure));

        // Container size -> viewport height (fires initially too).
        let callback: Closure<dyn FnMut(Vec<ResizeObserverEntry>)> = Closure::wrap(
            Box::new(move |entries: Vec<ResizeObserverEntry>| {
                if let Some(entry) = entries.first()
                    && let Some(el) = entry.target().dyn_ref::<web_sys::HtmlElement>()
                {
                    viewport_h.set(el.client_height() as f64);
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
    // "Take me to where I am": re-clicking the active Thumbs tab.
    //
    // The auto-centre effect below already follows the reader, but it
    // deliberately yields to the user-drive grace — if you have been scrolling
    // the grid yourself it will not yank the view back. That is right for
    // passive following and wrong for an explicit request, so this clears the
    // grace timestamp and centres immediately.
    {
        let reveal_drive = last_user_drive.clone();
        Effect::new(move |_| {
            let reveal_drive = reveal_drive.clone();
            let handle = window_event_listener(
                leptos::ev::Custom::new("pdfreader:reveal-active"),
                move |_: web_sys::CustomEvent| {
                    if state.sidebar.get_untracked() != SidebarMode::Thumbs {
                        return;
                    }
                    let Some(el) = container_el.get_value() else { return };
                    let vh = el.client_height() as f64;
                    let rh = row_height();
                    let total_rows = rows();
                    if vh <= 0.0 || rh <= 0.0 || total_rows == 0 {
                        return;
                    }
                    // Clear the grace so the request is honoured even if the
                    // reader was just scrolling the grid — that IS how they got
                    // lost, so refusing to move would defeat the gesture.
                    reveal_drive.set(f64::NEG_INFINITY);

                    let p = state.viewer.page.get_untracked();
                    let row = (p.saturating_sub(1) / 2) as f64;
                    let cell_h = CELL_W * aspect();
                    let cell_center_y = PAD + row * rh + cell_h / 2.0;
                    let max_scroll = (PAD * 2.0 + total_rows as f64 * rh - vh).max(0.0);
                    let target = (cell_center_y - vh / 2.0).clamp(0.0, max_scroll);

                    let opts = web_sys::ScrollToOptions::new();
                    opts.set_top(target);
                    opts.set_behavior(web_sys::ScrollBehavior::Smooth);
                    el.scroll_to_with_scroll_to_options(&opts);
                },
            );
            on_cleanup(move || handle.remove());
        });
    }

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
        let step_slot: RevealSlot = Rc::new(RefCell::new(None));
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
                let h = next.and_then(|next| {
                    set_timeout_with_handle(
                        move || next(),
                        Duration::from_millis((GRACE_MS - elapsed + 50.0) as u64),
                    )
                    .ok()
                });
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
