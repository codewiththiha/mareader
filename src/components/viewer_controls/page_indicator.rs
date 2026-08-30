//! Corner page counter ("25 / 300" or "42%"). Solid translucent backdrop
//! instead of the old mix-blend-difference footer: the reference shows a
//! rounded badge sitting on the page corner, readable over any document color.
//!
//! `bg-black/60 text-white` is deliberately theme-independent (same reason the
//! old footer used white + difference): it must read on light paper, dark
//! paper, and every tint.
//!
//! Takes plain signals, not `AppState`: the indicator is reusable UI and
//! knows nothing about the app's state shape. The caller gates on
//! `DocStatus::Ready` and positions the badge.

use leptos::prelude::*;

use pdf_core::settings::PageIndicatorStyle;

#[component]
pub fn PageIndicator(
    #[prop(into)]
    current: Signal<u32>,
    #[prop(into)]
    total: Signal<u32>,
    #[prop(into)]
    style: Signal<PageIndicatorStyle>,
    /// Fade out while a bottom overlay (gloss selection bar) is up, so the
    /// two never stack over each other.
    #[prop(optional, into)]
    hidden: Option<Signal<bool>>,
) -> impl IntoView {
    let hidden = hidden.unwrap_or_else(|| Signal::derive(|| false));
    view! {
        <span
            class="rounded-md bg-black/60 px-2 py-0.5 text-[11px] font-medium \
                   tabular-nums text-white/90 backdrop-blur-sm \
                   transition-opacity duration-150"
            class=("opacity-0", move || hidden.get())
        >
            {move || match style.get() {
                PageIndicatorStyle::Percentage => {
                    let (p, n) = (current.get(), total.get());
                    if n == 0 {
                        "–".to_string()
                    } else {
                        format!("{}%", ((p as f64 / n as f64) * 100.0).round() as u32)
                    }
                }
                PageIndicatorStyle::PageNumber => {
                    format!("{} / {}", current.get(), total.get())
                }
            }}
        </span>
    }
}
