//! Range slider atom bound to a numeric signal.

use leptos::prelude::*;

#[component]
pub fn Slider(
    value: ReadSignal<f64>,
    min: f64,
    max: f64,
    step: f64,
    on_change: impl Fn(f64) + 'static,
    #[prop(optional)] label: Option<String>,
) -> impl IntoView {
    view! {
        <label class="flex w-full flex-col gap-1">
            {label.map(|l| view! { <span class="text-xs text-muted">{l}</span> })}
            <input
                type="range"
                min=min.to_string()
                max=max.to_string()
                step=step.to_string()
                prop:value=move || value.get().to_string()
                on:input=move |ev| {
                    let v = event_target_value(&ev);
                    if let Ok(n) = v.parse::<f64>() {
                        on_change(n);
                    }
                }
                class="h-2 w-full cursor-pointer appearance-none rounded-full bg-line accent-accent"
            />
        </label>
    }
}
