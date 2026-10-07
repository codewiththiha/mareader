//! Corner page counter ("25 / 300" or "42%") on a translucent badge.

use leptos::prelude::*;

use reader_core::settings::PageIndicatorStyle;

#[component]
pub fn PageIndicator(
    #[prop(into)] current: Signal<u32>,
    #[prop(into)] total: Signal<u32>,
    #[prop(into)] style: Signal<PageIndicatorStyle>,
    /// Fade out while a bottom overlay is up, so the two never stack.
    hidden: Signal<bool>,
) -> impl IntoView {
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
