//! Bottom status bar (page x/y, zoom %, doc title/error). OWNED BY branch B
//! (viewer/chrome). Falls back to "No document" when status != Ready.
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
        <div class="flex h-8 items-center gap-3 px-3 text-xs text-white">
            <span class="min-w-0 flex-1 truncate">
                {move || match state.doc.status.get() {
                    DocStatus::Ready => state
                        .doc
                        .title
                        .get()
                        .unwrap_or_else(|| "Untitled".to_string()),
                    DocStatus::Opening => "Opening…".to_string(),
                    DocStatus::Error => state
                        .doc
                        .error
                        .get()
                        .unwrap_or_else(|| "Could not open PDF".to_string()),
                    DocStatus::Idle => "No document".to_string(),
                }}
            </span>
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
            <span class="whitespace-nowrap">
                {move || {
                    if state.doc.status.get() == DocStatus::Ready {
                        format!(
                            "{}%",
                            (state.viewer.render_scale.get() * 100.0).round() as u32
                        )
                    } else {
                        String::new()
                    }
                }}
            </span>
        </div>
    }
}
