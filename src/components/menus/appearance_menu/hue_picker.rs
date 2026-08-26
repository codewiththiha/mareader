//! Hue selector: a spectrum strip plus a row of one-click swatches.
//!
//! WHY NOT `<input type="color">`. The native picker returns an arbitrary RGB
//! triple, but the tint pipeline is driven by a HUE ANGLE alone — strength is
//! its own control, and lightness must stay fixed or the page stops being
//! readable paper. Handing the user a full RGB picker would let them choose
//! "very dark navy" and get something quite different from what they picked,
//! because only the hue of their choice survives. A hue strip promises exactly
//! what it delivers.
//!
//! The swatches exist because "warm paper" is the common request and hunting
//! for 34° on a gradient is a poor way to ask for it.

use leptos::prelude::*;

use crate::components::primitives::form::range_input::RangeInput;

/// Named landmarks on the hue circle. These are the hues behind the classic
/// reading modes plus the obvious cool/neutral choices, so the presets are
/// reachable in one click and hand-tuning starts from somewhere sensible.
pub const HUE_SWATCHES: [(u16, &str); 7] = [
    (34, "Sepia"),
    (14, "Rose"),
    (104, "Green"),
    (160, "Mint"),
    (200, "Sky"),
    (240, "Blue"),
    (290, "Violet"),
];

#[component]
pub fn HuePicker(
    hue: ReadSignal<f64>,
    on_change: impl Fn(f64) + 'static + Clone,
) -> impl IntoView {
    let on_change_strip = on_change.clone();
    view! {
        <div class="flex w-full flex-col gap-2">
            <span class="flex items-baseline justify-between text-xs text-muted">
                <span>"Colour"</span>
                <span class="tabular-nums text-ink">
                    {move || format!("{}°", hue.get().round())}
                </span>
            </span>

            // The track is painted with the actual hue circle so the control
            // previews its own output. `--tw-*` utilities cannot express this,
            // so the gradient rides the RangeInput's class pass-through — the
            // single place a colour value is written outside the theme tokens.
            <RangeInput
                value=hue.into()
                min=Signal::derive(|| 0.0)
                max=Signal::derive(|| 359.0)
                step=Signal::derive(|| 1.0)
                on_input=on_change_strip
                aria_label="Tint colour"
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
                                // hsl, not oklch: the swatch must show the hue
                                // the filter will actually produce (hue-rotate
                                // works in sRGB). See Appearance::ui_overrides.
                                style=format!("background-color: hsl({h} 60% 55%)")
                                class=move || {
                                    // A ring, not a border swap: changing the
                                    // border width would resize the swatch and
                                    // make the row twitch as you click along it.
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
