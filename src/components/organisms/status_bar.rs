//! Bottom status bar (page position only). OWNED BY branch B (viewer/chrome).
//! Phase 1 redesign: the filename and zoom readouts leave this bar (they move
//! to the toolbar and the zoom controls) — it now shows just the centered
//! `x / y` page counter.
//!
//! While `DocStatus::Ready`, the counter reads the live `viewer.page` /
//! `doc.num_pages`; any other status shows `– / –`.
//!
//! Mid-session open errors are surfaced by the toast system (U10): the
//! open-flow in `molecules/toolbar.rs` emits an error toast and the transient
//! `doc.error` span that used to live here was removed. When no document is
//! open, the ReaderView placeholder still shows `doc.error` directly.
//!
//! Rendered as an overlay: the `<footer>` in ReaderView positions it
//! absolutely over the bottom of the viewer (no layout space of its own) and
//! makes it click-through. The bar has NO background — just the white text
//! floating over the page.
//!
//! `mix-blend-mode: difference` lives on the FOOTER (ReaderView), not here: a
//! blend only mixes with content inside its own stacking context, and the
//! footer (absolute + z-50) creates one — if the blend were on this div, it
//! would be isolated inside the footer and never reach the PDF pages. On the
//! footer, the white text inverts against whatever is beneath it, so it stays
//! readable (pops out) over any document color.

use leptos::prelude::*;

use crate::core::document::DocStatus;
use crate::core::state::AppState;

#[component]
pub fn StatusBar(state: AppState) -> impl IntoView {
    view! {
        <div class="flex h-8 items-center justify-center gap-3 px-3 text-xs text-white">
            <span class="whitespace-nowrap">
                {move || match state.doc.status.get() {
                    DocStatus::Ready => format!(
                        "{} / {}",
                        state.viewer.page.get(),
                        state.doc.num_pages.get()
                    ),
                    _ => "– / –".to_string(),
                }}
            </span>
        </div>
    }
}
