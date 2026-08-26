//! Thumbnail grid: low-scale canvases (no text layer) for every page, rendered
//! through the engine's cached thumbnail lane. Clicking jumps to that page.
//!
//! Scroll-windowed by `virtual-list-leptos`: the virtualizer owns the row
//! window, spacer extent, scroll coalescing, and container measurement. This
//! component keeps the UX policy layered on top: healing, generation guards,
//! drive listeners, and the `live` gate that drops every cell once the close
//! slide finishes.

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

use crate::components::primitives::hooks::use_resize_observer::use_resize_observer;
use crate::components::primitives::hooks::use_timeout::use_debounce;
use crate::state::ReaderState;
use crate::state::ui::SidebarMode;

use super::auto_center::AutoCenter;
use super::geometry::{CELL_W, GAP_CROSS, MIN_VIEWPORT_H, PAD, ROW_BUFFER, row_height};
use super::thumbnail_cell::ThumbCell;

type DriveSlot = StoredValue<Option<Closure<dyn FnMut(web_sys::Event)>>, LocalStorage>;

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

    let heal = RwSignal::new(0u64);
    let heal_debounce = use_debounce(Duration::from_millis(500), move || {
        heal.update(|n| *n += 1);
    });
    let v_heal = v.clone();
    Effect::new(move |_| {
        if !live.get() {
            heal_debounce.cancel();
            return;
        }
        _ = v_heal.scroll_offset().get();
        heal_debounce.trigger();
    });

    let generation = Arc::new(AtomicU32::new(0));
    let doc_key = RwSignal::new(0u32);
    let bound: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));
    let doc_seen = StoredValue::new_local(false);
    let gen_doc = generation.clone();
    let v_reset = v.clone();
    Effect::new(move |_| {
        let count = num_pages.get();
        let size = page1_size.get();
        if count > 0 && size.is_some() {
            if doc_seen.get_value() {
                gen_doc.fetch_add(1, Ordering::Relaxed);
                doc_key.update(|key| *key += 1);
                layout_epoch.update(|epoch| *epoch += 1);
                v_reset.scroll_to_offset(-PAD, ScrollMode::Instant);
            } else {
                doc_seen.set_value(true);
            }
        }
    });

    let scroll_ref: NodeRef<html::Div> = NodeRef::new();
    let auto = AutoCenter::new(v.clone());
    let drive_owner = auto.last_user_drive.clone();
    let drive_slot: DriveSlot = StoredValue::new_local(None);
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
        if drive_slot.with_value(|slot| slot.is_some()) {
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
        for event in ["wheel", "pointerdown", "touchstart"] {
            _ = el.add_event_listener_with_callback(event, &drive_fn);
        }
        drive_slot.set_value(Some(drive_closure));
    });

    let v_remeasure = v.clone();
    Effect::new(move |_| {
        if live.get() {
            v_remeasure.remeasure_container();
        }
    });
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
                    let key = doc_key.get();
                    rows.get()
                        .into_iter()
                        .map(move |row| (key, row))
                        .collect::<Vec<_>>()
                }
                key=|(key, row): &(u32, VirtualRow)| (*key, row.row)
                children=move |(_key, row): (u32, VirtualRow)| {
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
                                    heal=Signal::derive(move || heal.get())
                                />
                                {if p2 <= page_count {
                                    view! {
                                        <ThumbCell
                                            state=state
                                            page=p2
                                            generation=generation.clone()
                                            bound=bound.clone()
                                            heal=Signal::derive(move || heal.get())
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
