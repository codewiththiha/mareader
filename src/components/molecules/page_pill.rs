//! Corner page counter pill. Solid translucent backdrop instead of the old
//! mix-blend-difference footer: the reference shows a rounded badge sitting
//! on the page corner, readable over any document color.
//!
//! `bg-black/60 text-white` is deliberately theme-independent (same reason the
//! old footer used white + difference): it must read on light paper, dark
//! paper, and every tint.

use leptos::prelude::*;

use pdf_engine::types::DocStatus;
use crate::core::state::AppState;

#[component]
pub fn PagePill(state: AppState) -> impl IntoView {
    view! {
        <Show when=move || state.doc.status.get() == DocStatus::Ready>
            <div class="pointer-events-none absolute bottom-3 right-3 z-30">
                <span class="rounded-md bg-black/60 px-2 py-0.5 text-[11px] font-medium tabular-nums text-white/90 backdrop-blur-sm">
                    {move || format!("{} / {}", state.viewer.page.get(), state.doc.num_pages.get())}
                </span>
            </div>
        </Show>
    }
}
