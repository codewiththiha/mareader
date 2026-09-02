use leptos::children::ChildrenFn;
use leptos::prelude::*;

use crate::components::document::layouts::layout_chrome;
use app_chrome::hooks::use_resize_observer::observe_content_size;
use crate::components::viewer_controls::overlay_scrollbar::OverlayScrollbar;
use crate::components::viewer_controls::progress_strip::ProgressStrip;
use crate::state::ReaderState;

/// Shared shell for Single & Spread. The child is centered with `margin:auto`
/// in the true viewport, which degrades to start-alignment on overflow —
/// so both axes scroll exactly to the page edge, never clipped, never with
/// phantom space.
///
/// `progress_visible` wires the same reading-progress setting the scroll modes
/// use: when on, a thin strip along the bottom advances with the current page
/// (which in these modes IS the reading position — there is no scroll offset
/// to divide).
#[component]
pub fn PageShell(
    state: ReaderState,
    scroller_id: &'static str,
    #[prop(into)]
    progress_visible: Signal<bool>,
    children: ChildrenFn,
) -> impl IntoView {
    observe_content_size(scroller_id, state.viewer.container_size);
    let chrome = layout_chrome(state, progress_visible);
    let fraction = Signal::derive(move || {
        let n = state.document.num_pages.get();
        if n == 0 {
            return 0.0;
        }
        // The current page over the total; page 1 is just off the start, the
        // last page is 100%.
        (state.viewer.page.get() as f64 / n as f64).clamp(0.0, 1.0)
    });
    view! {
        <div class="relative h-full w-full">
            <div
                id=scroller_id
                class="paginated-scroller scrollbar-none flex h-full w-full overflow-auto bg-surface"
            >
                <div
                    class="m-auto"
                    style:padding-inline=move || format!("{}px", chrome.inset.get())
                >
                    {children()}
                </div>
            </div>
            <OverlayScrollbar scroller_id=scroller_id />
            <Show when=move || progress_visible.get()>
                <ProgressStrip fraction=fraction />
            </Show>
        </div>
    }
}
