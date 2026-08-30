//! The labeled slider: range input + optional label — with a live numeric
//! readout and unit. Built on [`RangeInput`], so the low-level mechanics
//! (parsing, a11y, thumb sync) live exactly once.
//!
//! A slider with no numeric feedback can only be dialled by eye, which is
//! fine for "a bit more grain" but not for reproducing a look or reporting
//! one.

use leptos::prelude::*;

use super::range_input::RangeInput;

/// Labeled range slider bound to a numeric signal.
#[component]
pub fn Slider(
    #[prop(into)]
    value: Signal<f64>,
    min: f64,
    max: f64,
    step: f64,
    on_change: impl Fn(f64) + 'static,
    #[prop(into, optional)] label: Option<String>,
    /// Unit appended to the live readout (e.g. "%").
    #[prop(into, optional)]
    unit: Option<String>,
    #[prop(into, optional)]
    class: Option<String>,
    #[prop(default = false)]
    disabled: bool,
) -> impl IntoView {
    let unit_s = unit.unwrap_or_default();
    let label_for_aria = label.clone().unwrap_or_else(|| "slider".to_string());
    let range_class = match class {
        Some(c) if c.contains("h-") => c,
        Some(c) => format!("{c} h-2.5 w-full cursor-pointer appearance-none rounded-full"),
        None => "h-2 w-full cursor-pointer appearance-none rounded-full bg-line accent-accent".to_string(),
    };

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
            <RangeInput
                value=value
                min=Signal::derive(move || min)
                max=Signal::derive(move || max)
                step=Signal::derive(move || step)
                on_input=on_change
                aria_label=label_for_aria
                class=range_class
                disabled=disabled
            />
        </label>
    }
}
