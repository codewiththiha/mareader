//! The workspace's thumbnail panel: the reader's grid geometry and
//! auto-center, fed by data the host pulls from the ACTIVE reader instead of
//! a document this realm never opened. The panel knows only the page count,
//! the current page and the bitmap it is handed — it cannot render a PDF
//! itself, and it should not.

use leptos::prelude::*;
use serde_json::json;
use virtual_list::{Budget, GridSpec, Viewport};
use virtual_list_leptos::{ScrollMode, VirtualRow, VirtualizerOptions, use_virtualizer};
use wasm_bindgen::JsCast;

use app_chrome::hooks::use_resize_observer::use_resize_observer;

use crate::components::shell::sidebar::panels::thumbnails::geometry::{
    CELL_W, GAP_CROSS, MIN_VIEWPORT_H, PAD, ROW_BUFFER, row_height,
};
use crate::runtime::workspace::WorkspaceBridge;
use crate::state::AppState;

#[component]
pub fn WorkspaceThumbnails(state: AppState, bridge: WorkspaceBridge) -> impl IntoView {
    // The grid only exists for the format that can produce one: the host
    // answers thumbnail requests from the PDF engine and nothing else.
    let is_pdf = Signal::derive(move || state.reader.document.format.get() == reader_core::format::Format::Pdf);
    view! {
        <div class="sidebar-panel absolute inset-0 flex flex-col">
            <Show when=move || is_pdf.get() fallback=move || {
                view! { <div class="workspace-thumb-empty">Thumbnails are available for PDF documents.</div> }
            }>
                <ThumbGrid state=state bridge=bridge />
            </Show>
        </div>
    }
}

/// A page jump from a cell: the workspace's own viewer moves, and the
/// existing chrome relay carries the change to the active pane — the same
/// path every other control takes, so no second command channel exists.
fn jump(state: AppState, page: u32) {
    let total = state.reader.document.num_pages.get_untracked().max(1);
    let page = page.clamp(1, total);
    if state.reader.viewer.page.get_untracked() != page {
        state.reader.viewer.page.set(page);
    }
}

#[component]
fn ThumbGrid(state: AppState, bridge: WorkspaceBridge) -> impl IntoView {
    let num_pages = state.reader.document.num_pages;
    let count = Signal::derive(move || num_pages.get() as usize);
    // The window re-keys against the book, not just the page count: two
    // documents with the same count are not the same grid.
    let doc_key = Signal::derive(move || state.reader.document.book_id.get().clone().unwrap_or_default());
    let epoch = bridge.thumb_epoch;
    let estimate = move |_index: usize| row_height(state.reader.document.page1_aspect_now());
    let v = use_virtualizer(
        VirtualizerOptions::grid(count, estimate, GridSpec::fixed(2, GAP_CROSS))
            .budget(Budget::items(ROW_BUFFER, 64))
            .padding(PAD, PAD)
            .initial(Viewport::new(MIN_VIEWPORT_H, 2.0 * CELL_W + GAP_CROSS), 0.0)
            .epoch(epoch.into()),
    );
    let rows = v.rows();
    let total_size = v.total_size();

    let scroll_ref: NodeRef<leptos::html::Div> = NodeRef::new();
    let v_bind = v.clone();
    Effect::new(move |_| {
        let Some(div) = scroll_ref.get() else {
            return;
        };
        let el: web_sys::Element = div.clone().unchecked_into();
        v_bind.bind_container(el);
        v_bind.remeasure_container();
    });
    let v_resize = v.clone();
    use_resize_observer(scroll_ref, move |_| v_resize.remeasure_container());

    // A different book behind the pane: the old window's scroll position and
    // the old bitmaps are both wrong for the new page ladder, so the window
    // re-keys (epoch above) and lands at the top.
    let v_doc = v.clone();
    let doc_change = doc_key;
    Effect::new(move |_| {
        let _ = doc_change.get();
        let _ = epoch.get();
        // The frame callback owns its copy: moving the effect's capture in
        // would make the effect one-shot.
        let raf = v_doc.clone();
        request_animation_frame(move || {
            raf.remeasure_container();
            raf.scroll_to_offset(-PAD, ScrollMode::Instant);
        });
    });

    // The reader moved: the workspace learns from the snapshot sync, and the
    // grid answers by centering the new page — no scroll event required.
    let v_center = v.clone();
    let page_signal = state.reader.viewer.page;
    let document = state.reader.document;
    let num_center = num_pages;
    let doc_center = doc_key;
    Effect::new(move |_| {
        let page = page_signal.get();
        let _ = doc_center.get();
        let total = num_center.get();
        if page < 1 || total < 1 {
            return;
        }
        // The frame callback owns its copy: moving the effect's capture in
        // would make the effect one-shot.
        let raf = v_center.clone();
        request_animation_frame(move || {
            raf.remeasure_container();
            let viewport = raf.viewport().get_untracked().main;
            if viewport <= 1.0 {
                return;
            }
            let row_h = row_height(document.page1_aspect_now());
            let target = raf.offset_of((page - 1) as usize) + row_h / 2.0 - viewport / 2.0;
            let max = (raf.total_size().get_untracked() - viewport).max(0.0);
            raf.scroll_to_offset(target.clamp(0.0, max), ScrollMode::Instant);
        });
    });

    // The remote half of the grid: request the nearest unloaded visible page,
    // one at a time — the host's renderer is sequential, and a request in
    // flight is a page the panel does not ask for again.
    let pending = RwSignal::new(false);
    let last_epoch = RwSignal::new(0u64);
    let last_count = RwSignal::new(0usize);
    let request_rows = rows;
    let request_num = num_pages;
    Effect::new(move |_| {
        let epoch_now = epoch.get();
        let thumbs_count = bridge.thumbnails.with(|thumbs| thumbs.len());
        // The bitmap arrived (or the book changed and the map emptied): the
        // request in flight is over, so the next visible page may be asked.
        if epoch_now != last_epoch.get() || thumbs_count != last_count.get() {
            last_epoch.set(epoch_now);
            last_count.set(thumbs_count);
            pending.set(false);
        }
        if pending.get() {
            return;
        }
        let visible: Vec<u32> = request_rows
            .get()
            .iter()
            .flat_map(|row| [row.items.start + 1, row.items.start + 2])
            .map(|p| p as u32)
            .filter(|p| *p >= 1 && *p <= request_num.get())
            .collect();
        let current = state.reader.viewer.page.get();
        let target: Option<u32> = bridge.thumbnails.with(|thumbs| {
            visible
                .into_iter()
                .filter(|p| !thumbs.contains_key(p))
                .min_by_key(|p| p.abs_diff(current))
        });
        if let Some(page) = target {
            pending.set(true);
            crate::runtime::emit(json!({"type":"thumbnail-request","page":page}));
        }
    });

    view! {
        <div id="workspace-thumb-scroll" node_ref=scroll_ref class="relative flex-1 overflow-y-auto p-3">
            <div aria-hidden="true" style:height=move || format!("{}px", total_size.get())></div>
            <For
                each=move || {
                    let _ = doc_key.get();
                    rows.get()
                }
                key=move |row: &VirtualRow| (doc_key.get_untracked(), row.row)
                children=move |row: VirtualRow| {
                    let p1 = (row.items.start + 1) as u32;
                    let p2 = (row.items.start + 2) as u32;
                    let total = num_pages.get();
                    view! {
                        <div class="absolute inset-x-3" style:top=format!("{}px", row.start)>
                            <div class="grid grid-cols-2 gap-3">
                                <WorkspaceThumbCell state=state bridge=bridge page=p1 />
                                {if p2 <= total {
                                    view! {
                                        <WorkspaceThumbCell state=state bridge=bridge page=p2 />
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

#[component]
fn WorkspaceThumbCell(state: AppState, bridge: WorkspaceBridge, page: u32) -> impl IntoView {
    let is_current = move || state.reader.viewer.page.get() == page;
    let src = Signal::derive(move || bridge.thumbnails.with(|thumbs| thumbs.get(&page).cloned()));
    // The cell box is the geometry invariant: the virtualizer estimates rows
    // from CELL_W and the page-1 aspect (the grid's `estimate`), so the
    // mounted card must be exactly that box — the bitmap letterboxes inside
    // it instead of driving the row height.
    let cell_h = move || CELL_W * state.reader.document.page1_aspect_now();
    view! {
        <button
            class="workspace-thumb-cell"
            data-page=page
            class=("is-current", is_current)
            style:height=move || format!("{}px", cell_h())
            on:click=move |_| jump(state, page)
        >
            <Show when=move || src.get().is_some() fallback=move || {
                view! { <div class="thumb-canvas thumb-skeleton"></div> }
            }>
                <img class="thumb-canvas" alt=format!("Page {page}") src=move || src.get().unwrap_or_default() />
            </Show>
            <span class="thumb-num" aria-hidden="true">{page}</span>
        </button>
    }
}
