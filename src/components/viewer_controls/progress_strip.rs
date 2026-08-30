//! The reading-progress strip: a thin accent bar along the bottom edge that
//! fills toward the end as the reader advances. Shared by every view mode so
//! the setting that turns it on means the same thing everywhere — it is NOT
//! vertical-scroll-only.
//!
//! (Do not confuse this with `effects::reader::reading_progress`, which is a
//! different concern: that module PERSISTS the reader's page so the next open
//! resumes there. This file is the visual bar, nothing else.)
//!
//! Position + elevation come from the caller (a `Show` gate); this component
//! only renders the strip and its width. The fraction is caller-supplied
//! because each mode computes it differently: a scroll mode divides the strip
//! offset by the axis-appropriate extent, a paged mode divides the current
//! page by the page count. Both are clamped to [0, 1] here so no caller can
//! paint past the edge or backwards.

use leptos::prelude::*;

use crate::components::primitives::floating::types::z::CONTROLS;

#[component]
pub fn ProgressStrip(
    /// 0..1 along the book. Clamped defensively on read.
    #[prop(into)]
    fraction: Signal<f64>,
) -> impl IntoView {
    view! {
        <div class=format!("pointer-events-none absolute inset-x-0 bottom-0 {CONTROLS} h-0.5")>
            <div
                class="h-full bg-accent/80 transition-[width] duration-100 ease-out"
                style:width=move || format!("{}%", fraction.get().clamp(0.0, 1.0) * 100.0)
            ></div>
        </div>
    }
}
