//! Thumbnail grid: low-scale canvases (no text layer) for every page, rendered
//! through the engine's cached thumbnail lane. Clicking jumps to that page.
//!
//! Scroll-windowed by `virtual-list-leptos`: the virtualizer owns the row
//! window, spacer extent, scroll coalescing, and container measurement. This
//! component keeps the UX policy layered on top: healing, generation guards,
//! drive listeners, and the `live` gate that drops every cell once the close
//! slide finishes.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use leptos::html;
use leptos::prelude::*;
use virtual_list::{Budget, GridSpec, Viewport};
use virtual_list_leptos::{ScrollMode, VirtualRow, VirtualizerOptions, use_virtualizer};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::Event;

use app_chrome::hooks::use_resize_observer::use_resize_observer;
use app_chrome::hooks::use_timeout::use_debounce;
use crate::state::ReaderState;
use crate::state::ui::SidebarMode;

use super::auto_center::AutoCenter;
use super::geometry::{CELL_W, GAP_CROSS, MIN_VIEWPORT_H, PAD, ROW_BUFFER, row_height};
use super::thumbnail_cell::{ThumbCell, ThumbRegistry};

/// The drive `Closure`, kept alive for the panel's lifetime: dropping it
/// frees the wasm shim the listeners reference, so it may only go once its
/// listeners are removed (see the cleanup below).
type DriveClosureSlot = StoredValue<Option<Closure<dyn FnMut(web_sys::Event)>>, LocalStorage>;
/// The drive listeners' `js_sys::Function` plus the element they are bound
/// to, parked so `on_cleanup` can remove them with the same function that
/// was added. A listener left on a detached node would keep firing, and the
/// rail remounts when it moves between the docked and overlay layouts —
/// without the removal, every remount accumulated three more listeners.
type DriveFnSlot = StoredValue<Option<(js_sys::Function, web_sys::Element)>, LocalStorage>;

/// The three drive events, listed once so bind and unbind cannot drift apart.
const DRIVE_EVENTS: [&str; 3] = ["wheel", "pointerdown", "touchstart"];

#[component]
pub fn ThumbnailsPanel(
    state: ReaderState,
    #[prop(into)] live: Signal<bool>,
    sidebar: RwSignal<SidebarMode>,
) -> impl IntoView {
    let num_pages = state.document.num_pages;
    let page1_size = state.document.page1_size;

    let count = Signal::derive(move || num_pages.get() as usize);
    let layout_epoch = RwSignal::new(0u64);
    let estimate = {
        let document = state.document;
        move |_index: usize| row_height(document.page1_aspect_untracked())
    };
    let v = use_virtualizer(
        VirtualizerOptions::grid(count, estimate, GridSpec::fixed(2, GAP_CROSS))
            .budget(Budget::items(ROW_BUFFER, 64))
            .padding(PAD, PAD)
            .initial(Viewport::new(MIN_VIEWPORT_H, 2.0 * CELL_W + GAP_CROSS), 0.0)
            .epoch(layout_epoch.into()),
    );
    let rows = v.rows();
    let total_size = v.total_size();

    // A cell that fails to paint (a cache or cancellation race) sets
    // `needs_heal`; only then is a sweep scheduled. The scroll stream no
    // longer re-arms the debounce on every tick — the sweep is a recovery
    // path for stale cells, not a heartbeat.
    let needs_heal = RwSignal::new(false);
    let heal = RwSignal::new(0u64);
    let heal_debounce = use_debounce(Duration::from_millis(500), move || {
        heal.update(|n| *n += 1);
    });
    Effect::new(move |_| {
        let stale = needs_heal.get();
        if !live.get() {
            heal_debounce.cancel();
            return;
        }
        if !stale {
            return;
        }
        needs_heal.set(false);
        heal_debounce.trigger();
    });

    let generation = Arc::new(AtomicU32::new(0));
    let doc_key = RwSignal::new(0u32);
    let bound: ThumbRegistry = Arc::new(Mutex::new(HashSet::new()));
    let doc_seen = StoredValue::new_local(false);
    let gen_doc = generation.clone();
    let bound_reset = bound.clone();
    let v_reset = v.clone();
    Effect::new(move |_| {
        let count = num_pages.get();
        let size = page1_size.get();
        if count > 0 && size.is_some() {
            if doc_seen.get_value() {
                gen_doc.fetch_add(1, Ordering::Relaxed);
                doc_key.update(|key| *key += 1);
                layout_epoch.update(|epoch| *epoch += 1);
                // The registry is cleared wholesale on a document switch
                // instead of drained one retired page at a time.
                if let Ok(mut guard) = bound_reset.lock() {
                    guard.clear();
                }
                v_reset.scroll_to_offset(-PAD, ScrollMode::Instant);
            } else {
                doc_seen.set_value(true);
            }
        }
    });

    let scroll_ref: NodeRef<html::Div> = NodeRef::new();
    let auto = AutoCenter::new(v.clone());
    let drive_owner = auto.last_user_drive.clone();
    let drive_closure_slot: DriveClosureSlot = StoredValue::new_local(None);
    let drive_fn_slot: DriveFnSlot = StoredValue::new_local(None);
    let v_bind = v.clone();

    Effect::new(move |_| {
        let Some(div) = scroll_ref.get() else {
            return;
        };
        let el: web_sys::Element = div.clone().unchecked_into();
        v_bind.bind_container(el.clone());
        // Measure NOW so the auto-center glide has a true viewport on its
        // first run. The container ResizeObserver only fires on size
        // CHANGES, and the panel's height is constant across the sidebar
        // slide, so a seed taken before layout settled would never
        // self-correct and the glide would compute against a placeholder.
        v_bind.remeasure_container();
        if drive_fn_slot.with_value(|slot| slot.is_some()) {
            return;
        }
        let drive = {
            let drive_last = drive_owner.clone();
            move |_: Event| {
                drive_last.set(js_sys::Date::now());
            }
        };
        let drive_closure = Closure::new(drive);
        let drive_fn: js_sys::Function = drive_closure
            .as_ref()
            .unchecked_ref::<js_sys::Function>()
            .clone();
        for event in DRIVE_EVENTS {
            // A failed bind means the drive listeners silently never fire;
            // say so in debug instead of swallowing the Result.
            debug_assert!(
                el.add_event_listener_with_callback(event, &drive_fn).is_ok(),
                "drive listener bind failed for {event} on #thumb-scroll"
            );
        }
        drive_fn_slot.set_value(Some((drive_fn, el.clone())));
        drive_closure_slot.set_value(Some(drive_closure));
    });

    // Remove the drive listeners with the same function that was added, and
    // only THEN drop the Closure: freeing the wasm shim while a listener
    // still references it would abort on the next event. Emptying the slots
    // releases both the Closures and the parked references.
    let drive_closure_cleanup = drive_closure_slot;
    let drive_fn_cleanup = drive_fn_slot;
    on_cleanup(move || {
        if let Some((f, el)) = drive_fn_cleanup.try_get_value().flatten() {
            for event in DRIVE_EVENTS {
                let _ = el.remove_event_listener_with_callback(event, &f);
            }
        }
        let _ = drive_fn_cleanup.try_set_value(None);
        let _ = drive_closure_cleanup.try_set_value(None);
    });

    // Size changes are the ResizeObserver's job alone (the virtualizer also
    // observes the container from `bind_container`); the `live` gate drops or
    // restores cells but never changes the container's size, so a manual
    // remeasure on it was a duplicate read.
    let v_resize = v.clone();
    use_resize_observer(scroll_ref, move |_| {
        v_resize.remeasure_container();
    });

    auto.install(state, sidebar);

    view! {
        <div
            id="thumb-scroll"
            node_ref=scroll_ref
            class="relative flex-1 overflow-y-auto p-3"
        >
            <div
                aria-hidden="true"
                style:height=move || format!("{}px", total_size.get())
            ></div>
            <For
                each=move || {
                    if !live.get() {
                        return Vec::new();
                    }
                    // Tracked: a document change bumps `doc_key`, which
                    // rebuilds the keys below and remounts every row even
                    // when the rebuilt rows compare equal.
                    let _ = doc_key.get();
                    rows.get()
                }
                key=move |row: &VirtualRow| (doc_key.get_untracked(), row.row)
                children=move |row: VirtualRow| {
                    let p1 = (row.items.start + 1) as u32;
                    let p2 = (row.items.start + 2) as u32;
                    let page_count = num_pages.get();
                    view! {
                        <div class="absolute inset-x-3" style:top=format!("{}px", row.start)>
                            <div class="grid grid-cols-2 gap-3">
                                <ThumbCell
                                    state=state
                                    page=p1
                                    generation=generation.clone()
                                    bound=bound.clone()
                                    heal=heal
                                    needs_heal=needs_heal
                                />
                                {if p2 <= page_count {
                                    view! {
                                        <ThumbCell
                                            state=state
                                            page=p2
                                            generation=generation.clone()
                                            bound=bound.clone()
                                            heal=heal
                                            needs_heal=needs_heal
                                        />
                                    }
                                    .into_any()
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
