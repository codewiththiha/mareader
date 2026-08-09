//! Bottom status bar (page x/y, zoom %, doc title/error). OWNED BY branch B
//! (viewer/chrome). Falls back to "No document" when status != Ready.

use leptos::prelude::*;

use crate::core::document::DocStatus;
use crate::core::state::AppState;

#[component]
pub fn StatusBar(state: AppState) -> impl IntoView {
    view! {
        <div class="flex h-8 items-center gap-3 border-t border-line bg-surface px-3 text-xs text-muted">
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
