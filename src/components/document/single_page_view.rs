//! Single-page (page-at-a-time) view.
//! One centered PageCanvas in a scroll container. Container size is tracked via
//! ResizeObserver; fit modes (Width/Page) are recomputed reactively.
//!
//! `page` is remounted per page by a keyed `<For>` (`PageCanvas` keys
//! on ids, not the page number — a fresh wrapper + host is mounted per page).
//! Each remount gets a direction-aware entrance animation (`page-enter-right`
//! for next page, `page-enter-left` for previous) driven by a non-reactive
//! `StoredValue` of the previously shown page.
//!
//! The `<For>` keying is load-bearing: a plain reactive block would patch the
//! wrapper div in place (tachys `value.rebuild`), so the CSS animation would not
//! restart on consecutive same-direction turns. Only a keyed remount inserts a
//! fresh node for the animation to replay on — and the unchanged key on a
//! same-page set leaves the existing node untouched (no spurious animation).

use leptos::prelude::*;

use crate::components::document::PageCanvas;
use crate::state::TextureSignal;
use crate::state::ReaderState;
use crate::components::document::dom_helpers::{observe_content_size, SINGLE_PAGE_CONTAINER_ID};

#[component]
pub fn SinglePageView(state: ReaderState) -> impl IntoView {
    // App-provided texture context, resolved once per view and passed to the
    // PageCanvas as an explicit prop (same pattern as PageList).
    let texture = use_context::<TextureSignal>()
        .expect("TextureSignal must be provided by app bootstrap");
    let display_scale = state.viewer.zoom.display.read_only();

    // Previously shown page, for the direction-aware page-turn animation.
    // Non-reactive on purpose: reading it inside the render closure doesn't
    // subscribe, so the wrapper only re-renders on the reactive `page` signal.
    // 0 = no history (first render) → default to "right" (forward).
    let prev_page = StoredValue::new(0u32);

    // --- Container-size tracking (ResizeObserver) ---------------------------
    // The observer + callback live in local-storage StoredValues so the JS
    // references stay alive for the component's lifetime and are dropped when
    // the component's reactive owner is disposed (no manual cleanup needed).
    // Container-size tracking: reports the #single-page-container content box
    // into viewer.container_size so fit modes use the real dimensions.
    observe_content_size(SINGLE_PAGE_CONTAINER_ID, state.viewer.container_size);

    // Fit-width/fit-page scale computation now lives in the app-root
    // `effects::fit::fit_effect` (shared with the continuous view).

    view! {
        <div
            id=SINGLE_PAGE_CONTAINER_ID
            class="flex h-full w-full items-start justify-center overflow-auto bg-surface"
        >
            // `pt-18` = the 1.5rem the page always had plus the 3rem toolbar,
            // so the sheet clears the glass header while the scroller still
            // spans the full window and lets the page slide under the bar.
            <div class="px-6 pb-6 pt-18">
                <For
                    // Single-item keyed list: the key IS the page, so a page
                    // change unmounts the old wrapper and mounts a fresh one
                    // (replaying the entrance animation), while a same-page
                    // set leaves the mounted node untouched.
                    each=move || std::iter::once(state.viewer.page.get())
                    key=|p: &u32| *p
                    children=move |page: u32| {
                        // Direction: "right" when moving forward (or first
                        // render), "left" when going back. Record this page as
                        // `prev` BEFORE the view so the next render sees the
                        // new history. Static literal classes only (repo rule).
                        let prev = prev_page.get_value();
                        let dir = if prev == 0 || page > prev {
                            "page-enter-right"
                        } else {
                            "page-enter-left"
                        };
                        prev_page.set_value(page);
                        view! {
                            <div class=dir>
                                <PageCanvas
                                    page=page
                                    scale=display_scale
                                    render_scale=state.viewer.zoom.render
                                    zoom_animating=state.viewer.zoom_animating
                                    texture=texture
                                    canvas_id=format!("sp-{page}-cv")
                                    host_id=format!("sp-{page}-pg")
                                    render_text=true
                                />
                            </div>
                        }
                    }
                />
            </div>
        </div>
    }
}
