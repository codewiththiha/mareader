//! The reading-progress strip: a thin accent bar along the bottom edge.

use leptos::prelude::*;

use app_chrome::layers::CONTROLS;

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
