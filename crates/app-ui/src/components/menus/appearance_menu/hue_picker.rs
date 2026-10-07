//! Hue selector: a spectrum strip plus one-click swatches.

use leptos::prelude::*;

use crate::components::primitives::form::range_input::RangeInput;

/// Named landmarks on the hue circle.
const HUE_SWATCHES: [(u16, &str); 7] = [
    (34, "Sepia"),
    (14, "Rose"),
    (104, "Green"),
    (160, "Mint"),
    (200, "Sky"),
    (240, "Blue"),
    (290, "Violet"),
];

#[component]
pub fn HuePicker(hue: ReadSignal<f64>, on_change: impl Fn(f64) + 'static + Clone) -> impl IntoView {
    let on_change_strip = on_change.clone();
    view! {
        <div class="flex w-full flex-col gap-2">
            // Titled for the hue, its value read out in degrees.
            <span class="flex items-baseline justify-between text-xs text-muted">
                <span>"Hue"</span>
                <span class="tabular-nums text-ink">
                    {move || format!("{}°", hue.get().round())}
                </span>
            </span>

            // The track is painted with the hue circle it previews.
            <RangeInput
                value=hue.into()
                min=Signal::derive(|| 0.0)
                max=Signal::derive(|| 359.0)
                step=Signal::derive(|| 1.0)
                on_input=on_change_strip
                aria_label="Hue"
                class="hue-strip h-4 w-full cursor-pointer appearance-none rounded-full border border-line"
            />

            <div class="flex flex-wrap gap-1.5">
                {HUE_SWATCHES
                    .iter()
                    .map(|(h, name)| {
                        let h = *h;
                        let cb = on_change.clone();
                        let active = move || (hue.get().round() as u16) == h;
                        view! {
                            <button
                                type="button"
                                title=*name
                                aria-label=*name
                                aria-pressed=move || active().to_string()
                                on:click=move |_| cb(h as f64)
                                // hsl, not oklch: the hue the filter produces.
                                style=format!("background-color: hsl({h} 60% 55%)")
                                class=move || {
                                    // A ring, so the row cannot twitch.
                                    if active() {
                                        "h-6 w-6 rounded-full border border-line ring-2 ring-accent ring-offset-1 ring-offset-surface"
                                    } else {
                                        "h-6 w-6 rounded-full border border-line hover:ring-2 hover:ring-line"
                                    }
                                }
                            />
                        }
                    })
                    .collect_view()}
            </div>
        </div>
    }
}
