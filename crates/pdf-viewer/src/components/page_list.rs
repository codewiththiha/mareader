//! Virtualized scroll container for the continuous layout. OWNED BY branch A
//! (viewer/continuous).
//!
//! A scroll container (`#page-list`) holding:
//!  - an in-flow spacer whose height equals the full document height, so the
//!    scrollbar spans the whole column,
//!  - a keyed `<For>` over the mounted page window (what is on screen plus a
//!    `RenderBudget` read-ahead, measured in screenfuls so it means the same
//!    thing at every zoom). Each page is an absolutely-positioned wrapper
//!    centered in the column, containing a shared foundation
//!    `PageCanvas`. Evicted pages are
//!    unmounted by `<For>` (their `on_cleanup` unregisters them with the
//!    engine, which zeros the canvas backing store — WKWebView will not
//!    release it on DOM removal alone); entering pages render via
//!    PageCanvas's own `scale` effect.

use leptos::prelude::*;

use crate::components::page_canvas::PageCanvas;
use pdf_core::layout::{
    page_top_css, render_range, total_height_css, PAGE_GAP, RENDER_BUDGET,
};
use crate::state::ViewerState;

#[component]
pub fn PageList(state: ViewerState) -> impl IntoView {
    // Seed every page's height from its OWN intrinsic size, at the scale in
    // force when the document is first laid out.
    //
    // Seeding all pages from page 1 (as this once did) is only correct for a
    // uniform document. In a mixed-size PDF every page that had not yet
    // rendered carried page 1's height until `on_geometry` corrected it, so
    // the document's total height — and every page offset below the viewport
    // — changed as pages were measured. Zooming out pulls a batch of
    // never-measured pages into view at once; correcting them all shifts the
    // content under the scroll anchor, and the reader lands on a different
    // page than the one they left.
    //
    // Seeds ONLY when the vector does not match the document, i.e. on open or
    // document change. It must not re-seed on scale changes: the zoom
    // coordinator rescales this same vector frame by frame and re-anchors
    // scroll against it, so a competing write mid-gesture would fight the
    // anchor and reintroduce the very drift this fixes.
    Effect::new(move || {
        let n = state.doc.num_pages.get() as usize;
        let scale = state.viewer.render_scale.get();
        let sizes = state.doc.page_sizes.get();
        let fallback = state.doc.page1_size.get().map(|s| s.height).unwrap_or(0.0);
        if n == 0 || scale <= 0.0 || (sizes.is_empty() && fallback <= 0.0) {
            return;
        }
        state.doc.page_heights.update(|v| {
            if v.len() == n {
                return; // already laid out for this document
            }
            *v = (0..n)
                .map(|i| {
                    sizes.get(i).copied().filter(|h| *h > 0.0).unwrap_or(fallback) * scale
                })
                .collect();
        });
    });

    // Mounted page window [first, last] (0-based, inclusive). `None` => no
    // pages to render yet.
    //
    // The read-ahead is measured in SCREENFULS, not pages (see RenderBudget),
    // so it means the same thing at 50% and at 500%. That is what keeps a
    // zoomed-in reader from holding six multi-megabyte off-screen rasters
    // they cannot reach for many seconds of scrolling.
    //
    // SELECTION PINNING. The window is extended to include every page the
    // reader currently has selected text on. Without this, scrolling would
    // evict the page the selection started on — orphaning the selection's DOM
    // nodes (the `<For>` unmounts `PageCanvas`, whose `on_cleanup` calls
    // `unregisterPage`, which zeros the canvas and removes the `.textLayer`
    // the selection was anchored to). The browser's copy command then reads a
    // stale/empty selection. Pinning the selected pages keeps them — and
    // their text layers — mounted so the selection's DOM nodes stay valid
    // and copy of multi-page selections works through any scroll.
    let visible = Memo::new(move |_| {
        let heights = state.doc.page_heights.get();
        let scroll_top = state.viewer.scroll_top.get();
        let vh = state.viewer.container_size.get().1;
        let mut range = render_range(
            scroll_top,
            vh,
            &heights,
            PAGE_GAP,
            RENDER_BUDGET,
        );
        // FIX B — pin the dominant page during a layout animation. During a
        // sidebar slide, `scroll_top` is re-anchored by `relayout_to()` AND
        // clamped back by the browser's own scroll clamp (the spacer height
        // is applied one Leptos pass later), so for one or two frames
        // `scroll_top + viewport` can fall inside a gap or past the shrunken
        // extent, `render_range` returns a window that no longer contains
        // the dominant page, and `<For>` unmounts it — the page under the
        // reader's eyes vanishes mid-slide, killing the stretch animation
        // (the node that was supposed to stretch no longer exists, and its
        // replacement has no bitmap). Pinning the dominant page for the
        // duration of any layout animation keeps the SAME DOM node alive
        // through the whole slide so the stretch effect visibly rescales it.
        if state.viewer.zoom_animating.get() && !heights.is_empty() {
            let dom = pdf_core::layout::dominant_page(scroll_top, vh, &heights, PAGE_GAP) as usize;
            let dom = dom.saturating_sub(1); // 1-based → 0-based
            range = match range {
                Some((f, l)) => Some((f.min(dom), l.max(dom))),
                None => Some((dom, dom)),
            };
        }
        // Extend to include the reader's current text-selection page range.
        if let Some((sel_first, sel_last)) = state.viewer.selected_pages.get() {
            // 1-based → 0-based.
            let sel_first = (sel_first.saturating_sub(1)) as usize;
            let sel_last = (sel_last.saturating_sub(1)) as usize;
            match &mut range {
                Some((first, last)) => {
                    if sel_first < *first {
                        *first = sel_first;
                    }
                    if sel_last > *last {
                        *last = sel_last;
                    }
                }
                None => {
                    // No visible pages yet, but the selection needs pages
                    // mounted — open a window just for them.
                    range = Some((sel_first, sel_last));
                }
            }
        }
        range
    });

    // The scale the layout is DRAWN at. During a zoom this moves every frame
    // and the canvases CSS-stretch to follow it; `render_scale` (what the
    // bitmaps were rasterised at) is read by PageCanvas itself and only changes
    // once, when the gesture settles.
    let display_scale = state.viewer.display_scale.read_only();

    // Store the real rendered height back into page_heights (0-based index).
    let on_geometry = Callback::new(move |(p, _w, h): (u32, f64, f64)| {
        // While a zoom animation is running the coordinator owns page_heights:
        // it rescales the whole vector per frame. A render that resolves
        // mid-flight would write ONE page's height at the old scale into that
        // vector, shifting every page below it and yanking the scroll — the
        // teleport bug in miniature. The post-settle render reports the true
        // height a moment later, so nothing is lost by skipping here.
        if state.viewer.zoom_animating.get_untracked() {
            return;
        }
        let idx = p.saturating_sub(1) as usize;
        state.doc.page_heights.update(|v| {
            while v.len() <= idx {
                v.push(0.0);
            }
            v[idx] = h;
        });
    });

    view! {
        <div id="page-list" class="h-full w-full overflow-y-auto outline-none" tabindex="0">
        // Inner column, offset by the toolbar height so the first page starts
        // below the glass header while the scrollport itself still runs the
        // full height of the window — that is what lets pages travel UNDER the
        // bar and give the backdrop-filter something to refract.
        //
        // The offset is a margin on this wrapper, NOT padding on the scroller,
        // and this wrapper (not #page-list) is the positioned ancestor. Both
        // details are load-bearing for the scroll maths:
        //   * pages are absolutely positioned against THIS box, so `offsetTop`
        //     still equals `page_top_css(i)` exactly, as effect 4 assumes;
        //   * a page's top in scroll coordinates becomes `48 + page_top_css(i)`,
        //     and landing it just under a 48px bar needs
        //     `scrollTop = page_top_css(i)` — precisely what page_tracking and
        //     search_effects already write. No offset arithmetic anywhere.
        <div class="relative mt-12">
            // Spacer: makes the scrollbar span the whole column.
            <div
                aria-hidden="true"
                style:height=move || {
                    let heights = state.doc.page_heights.get();
                    format!("{}px", total_height_css(&heights, PAGE_GAP))
                }
            ></div>
            <For
                each=move || {
                    visible
                        .get()
                        .map(|(first, last)| (first..=last).collect::<Vec<usize>>())
                        .unwrap_or_default()
                }
                key=|i: &usize| *i
                children=move |i: usize| {
                    let style = move || {
                        let heights = state.doc.page_heights.get();
                        let top = page_top_css(i, &heights, PAGE_GAP);
                        format!(
                            "position:absolute;top:{top}px;left:0;right:0;display:flex;justify-content:center"
                        )
                    };
                    view! {
                        <div id=format!("cont-{i}-wrap") style=style>
                            <PageCanvas
                                page={(i + 1) as u32}
                                scale=display_scale
                                canvas_id=format!("cont-{i}-cv")
                                host_id=format!("cont-{i}-pg")
                                render_text=true
                                on_geometry=on_geometry
                            />
                        </div>
                    }
                }
            />
        </div>
        </div>
    }
}
