//! Low-level reusable range control: parsing, styling and accessibility
//! in one place.

use leptos::prelude::*;

/// A controlled native range input.
#[component]
pub fn RangeInput(
    value: Signal<f64>,
    min: Signal<f64>,
    max: Signal<f64>,
    step: Signal<f64>,
    on_input: impl Fn(f64) + 'static,
    /// Extra classes (track styling; the default is the shared slider look).
    #[prop(optional, into)]
    class: Option<String>,
    /// Accessible name. Prefer passing it; a fallback generic label is used
    /// when missing.
    #[prop(optional, into)]
    aria_label: Option<String>,
    #[prop(into, default = Signal::derive(|| false))] disabled: Signal<bool>,
) -> impl IntoView {
    let class = class.unwrap_or_else(|| {
        "h-2 w-full cursor-pointer appearance-none rounded-full bg-line accent-accent".to_string()
    });
    let aria_label = aria_label.unwrap_or_else(|| "slider".to_string());

    view! {
        <input
            type="range"
            min=move || min.get().to_string()
            max=move || max.get().to_string()
            step=move || step.get().to_string()
            aria-label=aria_label
            prop:disabled=move || disabled.get()
            prop:value=move || value.get().to_string()
            on:input=move |ev| {
                if let Ok(n) = event_target_value(&ev).parse::<f64>() {
                    on_input(n);
                }
            }
            class=class
        />
    }
}
