//! Range slider bound to a numeric signal.

use leptos::prelude::*;

#[component]
pub fn Slider(
    value: ReadSignal<f64>,
    min: f64,
    max: f64,
    step: f64,
    on_change: impl Fn(f64) + 'static,
    #[prop(into, optional)] label: Option<String>,
    /// Unit appended to the live readout (e.g. "%"). A slider with no numeric
    /// feedback can only be dialled by eye, which is fine for "a bit more
    /// grain" but not for reproducing a look or reporting one.
    #[prop(into, optional)]
    unit: Option<String>,
) -> impl IntoView {
    let unit_s = unit.unwrap_or_default();
    let has_label = label.is_some();
    view! {
        <label class="flex w-full flex-col gap-1">
            {label.map(|l| {
                view! {
                    <span class="flex items-baseline justify-between text-xs text-muted">
                        <span>{l}</span>
                        <span class="tabular-nums text-ink">
                            {move || format!("{}{}", value.get().round(), unit_s)}
                        </span>
                    </span>
                }
            })}
            <input
                type="range"
                min=min.to_string()
                max=max.to_string()
                step=step.to_string()
                aria-label=has_label.then_some("").is_none().then_some("slider")
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
